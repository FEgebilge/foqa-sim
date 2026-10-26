//! Ground-station debrief. A `Station` holds what grading needs (rule set
//! and type profiles, loaded once); an `Analysis` is one flight run through
//! it, and turns into the objects of `docs/debrief-api.md` (`wire`): the
//! flight list entry, the full debrief and the post-landing notice. `fdm`
//! and the desktop app share it, so both show the same numbers.

#![forbid(unsafe_code)]

mod wire;

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use analyzer::{Flight, Phase, Span, Track, phases, thresholds, touchdowns};
use fdm_core::geo::distance_m;
use fdm_core::{Lfl, SimTime};
use recorder::{COMPACT_EXT, Recorder, ReplaySource, SimEvent, TOUCHDOWN_WINDOW_US};
use rules::{LoadError, ProfileError, ProfileField, ProfileSet, Rule, Selection};

pub use wire::*;

pub type Error = Box<dyn std::error::Error>;

const CORE_RULES: &str = include_str!("../../../rules/core.yaml");
const RAW_EXT: &str = ".raw.jsonl.gz";
const JOURNAL_EXT: &str = ".fqj";
const COMPACT_SUFFIX: &str = ".fqz";
/// Stored flights, the compact form first: it wins where both exist, as
/// while a journal is being compacted.
const FLIGHT_EXTS: [&str; 2] = [COMPACT_EXT, "fqj"];
/// The series keeps every `BASE_STEP`th track point (2 Hz at 8 Hz): enough
/// for a whole-flight profile, light enough for the app.
const BASE_STEP: usize = 4;
/// `Event.rule` of a pilot report.
pub const PILOT_REPORT: &str = "pilot-report";

/// The built-in core rule set (`rules/core.yaml`).
pub fn core_rules() -> Result<Vec<Rule>, LoadError> {
    rules::load(CORE_RULES)
}

/// Reads a journal (`.fqj`), a compact flight (`.fqz`), or a raw capture after recording it through the
/// on-board chain in memory, the way the aircraft would have.
pub fn load(path: &Path) -> io::Result<Flight> {
    if path.to_string_lossy().ends_with(RAW_EXT) {
        let mut recorder = Recorder::new(&Lfl::V1, Vec::new())?;
        recorder.run(&mut ReplaySource::open(path, recorder.acquisition())?)?;
        return Flight::read(&recorder.into_inner()[..]);
    }
    Flight::read(BufReader::new(File::open(path)?))
}

/// A flight's id: its file name without `.fqj` / `.fqz` / `.raw.jsonl.gz`.
pub fn flight_id(path: &Path) -> String {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    [RAW_EXT, JOURNAL_EXT, COMPACT_SUFFIX]
        .iter()
        .find_map(|ext| name.strip_suffix(ext))
        .unwrap_or(&name)
        .to_owned()
}

/// The stored flights in `dir` (journals and compact flights, one file per
/// flight), in no particular order.
pub fn flight_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    let stored = |p: &PathBuf| {
        FLIGHT_EXTS
            .iter()
            .position(|x| p.extension().is_some_and(|e| e == *x))
    };
    paths
        .iter()
        .filter(|p| {
            stored(p).is_some_and(|rank| rank == 0 || !p.with_extension(COMPACT_EXT).exists())
        })
        .cloned()
        .collect()
}

/// The uncompacted journals in `dir`.
pub fn journals(dir: &Path) -> Vec<PathBuf> {
    let entries = fs::read_dir(dir).into_iter().flatten().flatten();
    let paths = entries.map(|e| e.path());
    paths
        .filter(|p| p.extension().is_some_and(|e| e == "fqj"))
        .collect()
}

/// Compacts each journal (see `recorder::compact_file`), reporting on stderr.
/// A journal that fails or is cut off stays as it is. Returns how many did
/// not compact.
pub fn compact_all(journals: impl IntoIterator<Item = PathBuf>) -> usize {
    let mut left = 0;
    for journal in journals {
        match recorder::compact_file(&journal) {
            Ok(Some(dest)) => eprintln!("compacted: {}", dest.display()),
            Ok(None) => {
                left += 1;
                eprintln!("{}: cut off, left as is", journal.display());
            }
            Err(e) => {
                left += 1;
                eprintln!("{}: compaction failed: {e}", journal.display());
            }
        }
    }
    left
}

/// The stored file of flight `id`, if there is one.
pub fn flight_path(dir: &Path, id: &str) -> Option<PathBuf> {
    FLIGHT_EXTS
        .iter()
        .map(|ext| dir.join(format!("{id}.{ext}")))
        .find(|p| p.is_file())
}

/// What the flights folder takes up: every file in it, raw captures included.
pub fn storage_usage(dir: &Path) -> StorageUsage {
    let bytes = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum();
    StorageUsage {
        flights: flight_files(dir).len(),
        bytes,
        folder: dir.display().to_string(),
    }
}

/// Deletes the stored files of flight `id` in `dir`: its journal, compact
/// flight and raw capture, those exact names and nothing else. Returns how
/// many were there.
pub fn delete_flight(dir: &Path, id: &str) -> io::Result<usize> {
    let mut deleted = 0;
    for ext in [JOURNAL_EXT, COMPACT_SUFFIX, RAW_EXT] {
        match fs::remove_file(dir.join(format!("{id}{ext}"))) {
            Ok(()) => deleted += 1,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(deleted)
}

/// The rule set and type profiles flights are graded with.
pub struct Station {
    rules: Vec<Rule>,
    profiles: ProfileSet,
}

impl Station {
    pub fn new(rules: Vec<Rule>) -> Result<Self, ProfileError> {
        Ok(Self {
            rules,
            profiles: ProfileSet::shipped()?,
        })
    }

    /// The core rules and the shipped profiles.
    pub fn core() -> Result<Self, Error> {
        Ok(Self::new(core_rules()?)?)
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn profiles(&self) -> &ProfileSet {
        &self.profiles
    }

    /// `None` when the flight has no air/ground data to analyse.
    pub fn analyse(&self, id: String, flight: Flight) -> Option<Analysis<'_>> {
        let track = Track::new(&flight)?;
        let selection = self.profiles.select(&flight);
        Some(Analysis {
            events: rules::evaluate(&self.rules, &selection, &flight, &track),
            touchdowns: touchdowns(&flight, &track),
            phases: phases(&track),
            id,
            rules: &self.rules,
            selection,
            flight,
            track,
        })
    }

    /// Loads and analyses the flight at `path`.
    pub fn open(&self, path: &Path) -> Result<Analysis<'_>, Error> {
        Ok(self
            .analyse(flight_id(path), load(path)?)
            .ok_or("no air/ground data in this flight")?)
    }
}

/// One flight run through a `Station`.
pub struct Analysis<'a> {
    pub id: String,
    pub flight: Flight,
    pub track: Track,
    pub selection: Selection<'a>,
    pub phases: Vec<Span>,
    pub touchdowns: Vec<analyzer::Touchdown>,
    /// In time order, the most severe first at equal times.
    pub events: Vec<rules::Event>,
    rules: &'a [Rule],
}

impl Analysis<'_> {
    pub fn summary(&self) -> FlightSummary {
        let t = &self.track.t;
        let events = self.events();
        let mut event_counts: BTreeMap<_, usize> = rules::Level::ALL.map(|l| (Level(l), 0)).into();
        for e in &events {
            *event_counts.entry(e.level).or_default() += 1;
        }
        FlightSummary {
            started_utc: started_utc(&self.id),
            id: self.id.clone(),
            duration_s: t
                .first()
                .zip(t.last())
                .map_or(0.0, |(a, b)| secs(*b) - secs(*a)),
            aircraft: self.flight.aircraft().map(|id| Aircraft {
                icao: id.icao_type(),
                title: id.title.clone(),
            }),
            departure: self.departure(),
            arrival: self
                .touchdowns
                .last()
                .and_then(|td| Some(td.runway.as_ref()?.icao.clone())),
            landings: self.touchdowns.iter().filter(|td| !td.bounce).count(),
            worst_level: events.iter().map(|e| e.level).max(),
            event_counts,
        }
    }

    pub fn debrief(&self) -> Debrief {
        Debrief {
            flight: FlightHeader {
                summary: self.summary(),
                lfl_version: self.flight.lfl_version(),
                profile: self.selection.to_string(),
                class: self.selection.class.map(|c| c.to_string()),
                damaged_at: self.flight.damaged_at,
            },
            phases: self
                .phases
                .iter()
                .map(|s| PhaseSpan {
                    phase: s.phase.name(),
                    start_s: secs(s.start),
                    end_s: secs(s.end),
                })
                .collect(),
            touchdowns: self.touchdowns.iter().map(Touchdown::from).collect(),
            events: self.events(),
            series: self.series(),
            airports: self.airports(),
        }
    }

    /// Every event of the flight in time order, the most severe first at
    /// equal times: the rules' events and the pilot's reports.
    pub fn events(&self) -> Vec<Event> {
        let mut events: Vec<Event> = self.events.iter().map(|e| self.event(e)).collect();
        let reports = self.flight.events.iter().filter_map(|(t, e)| match e {
            SimEvent::PilotReport(note) => Some(self.report(*t, note)),
            _ => None,
        });
        events.extend(reports);
        // Stable: reports go after rule events at the same time.
        events.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
        events
    }

    /// The latest landing (see `landings`). `None` before the first touchdown.
    pub fn landing(&self) -> Option<LandingNotice> {
        self.landings().pop()
    }

    /// Every landing in time order: its first touchdown (a bounce belongs
    /// to the landing before it) and the events from the start of its
    /// approach through the touchdown window, which takes in its touchdown
    /// rules and any bounce.
    pub fn landings(&self) -> Vec<LandingNotice> {
        let events = self.events();
        let tds = self.touchdowns.iter().filter(|td| !td.bounce);
        tds.map(|td| {
            let from = self
                .phases
                .iter()
                .rev()
                .find(|s| s.phase == Phase::Approach && s.start <= td.t)
                .map_or(td.t, |s| s.start);
            let window = secs(from)..=secs(td.t.add_us(TOUCHDOWN_WINDOW_US));
            LandingNotice {
                flight_id: self.id.clone(),
                touchdown: td.into(),
                events: events
                    .iter()
                    .filter(|e| window.contains(&e.start_s))
                    .cloned()
                    .collect(),
            }
        })
        .collect()
    }

    /// An event with its rule's texts: the description (the "why" behind its
    /// threshold), why it matters and what to do about it.
    fn event(&self, e: &rules::Event) -> Event {
        let rule = self.rules.iter().find(|r| r.id == e.rule);
        Event {
            rule: e.rule.clone(),
            version: e.version,
            title: e.title.clone(),
            description: rule.map_or_else(String::new, |r| r.description.trim().to_owned()),
            level: Level(e.level),
            start_s: secs(e.start),
            end_s: secs(e.end),
            peak_s: secs(e.peak),
            measure: e.measure.clone(),
            value: e.value,
            threshold: e.threshold,
            unit: e.unit.map(|u| u.symbol()).filter(|s| !s.is_empty()),
            why_it_matters: rule.and_then(|r| text(r.why_it_matters.as_deref())),
            advice: rule.and_then(|r| text(r.advice.as_deref())),
            context: self.context(e.peak),
        }
    }

    /// A pilot report (a moment marked from the in-sim panel) as an
    /// advisory event; its note is the description.
    fn report(&self, t: SimTime, note: &str) -> Event {
        Event {
            rule: PILOT_REPORT.into(),
            version: 1,
            title: "Pilot report".into(),
            description: note.trim().to_owned(),
            level: Level(rules::Level::Advisory),
            start_s: secs(t),
            end_s: secs(t),
            peak_s: secs(t),
            measure: String::new(),
            value: None,
            threshold: None,
            unit: None,
            why_it_matters: None,
            advice: None,
            context: self.context(t),
        }
    }

    /// IAS, flap angle and the profile's VFE at `t`.
    fn context(&self, t: SimTime) -> Context {
        let flap_deg = self.flight.at("flap.te_angle", t);
        Context {
            ias_kt: self.flight.at("ias", t),
            vfe_kt: self.vfe(flap_deg),
            flap_deg,
        }
    }

    /// The profile's flap-speed limit at `flap_deg`, as the flap-overspeed
    /// rule reads it.
    fn vfe(&self, flap_deg: Option<f64>) -> Option<f64> {
        let profile = self.selection.profile?;
        profile.value(ProfileField::Vfe, || flap_deg.filter(|a| a.is_finite()))
    }

    /// The departure aerodrome: of the snapshots recorded before the first
    /// liftoff (the recorder takes them on the first ground frame), the
    /// one nearest where the flight started. `None` for a flight that
    /// started airborne or without snapshots.
    fn departure(&self) -> Option<String> {
        let start = *self.track.t.first()?;
        if !self.track.on_ground[0] {
            return None;
        }
        let liftoff = self
            .track
            .legs
            .first()
            .map(|l| self.track.t[l.airborne.start]);
        let here = (
            self.flight.at("pos.lat", start)?,
            self.flight.at("pos.lon", start)?,
        );
        let away = |a: &fdm_core::Airport| distance_m(here, (a.lat_deg, a.lon_deg));
        self.flight
            .events
            .iter()
            .filter(|(t, _)| liftoff.is_none_or(|off| *t <= off))
            .filter_map(|(_, e)| match e {
                SimEvent::Airport(a) => Some(a),
                _ => None,
            })
            .min_by(|a, b| away(a).total_cmp(&away(b)))
            .map(|a| a.icao.clone())
    }

    /// The 2 Hz base plus every 8 Hz point of the recorder's full-rate
    /// window either side of each touchdown, where the detail is.
    fn series(&self) -> Series {
        let track = &self.track;
        let near_touchdown = |t: SimTime| {
            self.touchdowns
                .iter()
                .any(|td| t.us_since(td.t).abs() <= TOUCHDOWN_WINDOW_US)
        };
        let points: Vec<usize> = (0..track.len())
            .filter(|&i| i % BASE_STEP == 0 || near_touchdown(track.t[i]))
            .collect();
        let pick = |v: &[f64]| points.iter().map(|&i| v[i]).collect();
        let param = |name| {
            points
                .iter()
                .map(|&i| self.flight.at(name, track.t[i]).unwrap_or(f64::NAN))
                .collect()
        };
        let flap_deg: Vec<f64> = param("flap.te_angle");
        // The profile's flap-speed limit at each point; NaN without a profile band or with the flaps up.
        let vfe_kt = flap_deg
            .iter()
            .map(|&a| self.vfe(Some(a)).unwrap_or(f64::NAN))
            .collect();
        Series {
            t_s: points.iter().map(|&i| secs(track.t[i])).collect(),
            alt_ft: pick(&track.alt_ft),
            aal_ft: pick(&track.aal_ft),
            ias_kt: pick(&track.ias_kt),
            gs_kt: pick(&track.gs_kt),
            vs_fpm: pick(&track.vs_fpm),
            nz_g: pick(&track.nz_g),
            pitch_deg: pick(&track.pitch_deg),
            roll_deg: pick(&track.roll_deg),
            flap_deg,
            vfe_kt,
            lat: param("pos.lat"),
            lon: param("pos.lon"),
        }
    }

    fn airports(&self) -> Vec<Airport> {
        self.flight
            .airports()
            .into_iter()
            .map(|a| Airport {
                runways: a
                    .runways
                    .iter()
                    .map(|r| Runway {
                        width_m: r.width_m,
                        ends: thresholds(r, a.magvar_deg).map(RunwayEnd::from),
                    })
                    .collect(),
                icao: a.icao,
            })
            .collect()
    }
}

fn secs(t: SimTime) -> f64 {
    t.as_secs_f64()
}

/// A rule text as shown: trimmed, `None` when empty.
fn text(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// ISO 8601 UTC from a `flight-<unix seconds>` id, as `foqa-rec` names them.
fn started_utc(id: &str) -> Option<String> {
    let unix: i64 = id.strip_prefix("flight-")?.parse().ok()?;
    let (days, s) = (unix.div_euclid(86_400), unix.rem_euclid(86_400));
    // Days to civil date (H. Hinnant, "chrono-Compatible Low-Level Date
    // Algorithms"), proleptic Gregorian.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        s / 3600,
        s / 60 % 60,
        s % 60
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::*;

    const C172: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/c172-pattern.raw.jsonl.gz"
    );

    fn station() -> &'static Station {
        static STATION: OnceLock<Station> = OnceLock::new();
        STATION.get_or_init(|| Station::core().unwrap())
    }

    fn c172() -> &'static Analysis<'static> {
        static C172_PATTERN: OnceLock<Analysis<'static>> = OnceLock::new();
        C172_PATTERN.get_or_init(|| station().open(Path::new(C172)).unwrap())
    }

    #[test]
    fn built_in_rules_load() {
        assert!(station().rules().len() >= 10, "{}", station().rules().len());
        for r in station().rules() {
            assert!(text(r.why_it_matters.as_deref()).is_some(), "{}", r.id);
            assert!(text(r.advice.as_deref()).is_some(), "{}", r.id);
        }
    }

    #[test]
    fn debrief_of_the_c172_pattern() {
        let d = c172().debrief();
        assert_eq!(d.flight.summary.id, "c172-pattern");
        assert_eq!(d.flight.summary.landings, 1);
        assert_eq!(
            d.flight.summary.worst_level,
            Some(Level(rules::Level::High))
        );
        assert_eq!(d.events.len(), 6);
        assert!(d.events.iter().all(|e| !e.description.is_empty()));
        assert!(d.events.iter().all(|e| e.advice.is_some()));
        assert!(
            d.events
                .iter()
                .all(|e| (e.start_s..=e.end_s).contains(&e.peak_s))
        );
        let [td] = &d.touchdowns[..] else {
            panic!("{:?}", d.touchdowns)
        };
        assert!((td.sink_fpm - 103.0).abs() < 5.0, "{}", td.sink_fpm);

        let s = &d.series;
        let n = s.t_s.len();
        let lengths = [
            &s.alt_ft,
            &s.aal_ft,
            &s.ias_kt,
            &s.gs_kt,
            &s.vs_fpm,
            &s.nz_g,
            &s.pitch_deg,
            &s.roll_deg,
            &s.flap_deg,
            &s.vfe_kt,
            &s.lat,
            &s.lon,
        ]
        .map(Vec::len);
        assert_eq!(lengths, [n; 12]);
        assert!(s.t_s.windows(2).all(|w| w[0] < w[1]));
        // 2 Hz outside the touchdown window, 8 Hz inside it.
        let step = |t: f64| {
            let i = s.t_s.partition_point(|&x| x < t);
            s.t_s[i + 1] - s.t_s[i]
        };
        assert_eq!(step(td.t_s - 60.0), 0.5);
        assert_eq!(step(td.t_s), 0.125);
    }

    #[test]
    fn json_follows_the_contract() {
        let json = serde_json::to_string(&c172().debrief()).unwrap();
        assert!(!json.contains("NaN") && !json.contains("inf"));
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let flight = &v["flight"];
        assert_eq!(flight["profile"], "no aircraft identity, unknown class");
        assert_eq!(flight["worst_level"], "high");

        assert_eq!(
            flight["event_counts"],
            serde_json::json!({"advisory": 0, "low": 1, "medium": 2, "high": 3})
        );
        // Unknown optional fields are left out, not null.
        assert!(
            flight.get("aircraft").is_none()
                && flight.get("class").is_none()
                && flight.get("damaged_at").is_none()
        );
        assert_eq!(v["phases"][0]["phase"], "Standing");
        assert_eq!(v["events"][0]["level"], "high");
    }

    #[test]
    fn landing_notice_takes_the_approach_events() {
        let notice = c172().landing().unwrap();
        assert_eq!(notice.flight_id, "c172-pattern");
        assert!((notice.touchdown.sink_fpm - 103.0).abs() < 5.0);
        // The whole unstable approach was flown in this pattern's final.
        assert_eq!(notice.events.len(), c172().events.len());
    }

    #[test]
    fn a_pilot_report_is_an_advisory_event_with_its_note() {
        let mut flight = load(Path::new(C172)).unwrap();
        let t = SimTime::from_secs_f64(100.0);
        let note = SimEvent::PilotReport(" long final ".into());
        flight.events.push((t, note));
        let a = station().analyse("marked".into(), flight).unwrap();
        let report = a.events().into_iter().find(|e| e.rule == PILOT_REPORT);
        let report = report.unwrap();
        assert_eq!(
            (report.title.as_str(), report.description.as_str()),
            ("Pilot report", "long final")
        );
        assert_eq!(
            (report.level, report.start_s),
            (Level(rules::Level::Advisory), 100.0)
        );
        assert!(report.context.ias_kt.is_some());
        let counts = &a.summary().event_counts;
        assert_eq!(counts[&Level(rules::Level::Advisory)], 1);
    }

    #[test]
    fn deleting_a_flight_takes_its_files_only() {
        let dir = std::env::temp_dir().join(format!("foqa-delete-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let names = [
            "flight-1.fqj",
            "flight-1.raw.jsonl.gz",
            "flight-10.fqz",
            "flight-1.txt",
        ];
        for name in names {
            fs::write(dir.join(name), b"x").unwrap();
        }
        assert_eq!(delete_flight(&dir, "flight-1").unwrap(), 2);
        let left: Vec<_> = names.iter().filter(|n| dir.join(n).exists()).collect();
        assert_eq!(left, [&"flight-10.fqz", &"flight-1.txt"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn start_time_from_the_recorder_file_name() {
        assert_eq!(
            started_utc("flight-1793027213").as_deref(),
            Some("2026-10-26T15:06:53Z")
        );
        assert_eq!(
            started_utc("flight-0").as_deref(),
            Some("1970-01-01T00:00:00Z")
        );
        assert_eq!(started_utc("c172-pattern"), None);
    }
}
