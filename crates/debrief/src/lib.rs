//! Ground-station debrief. A `Station` holds what grading needs (rule set
//! and type profiles, loaded once); an `Analysis` is one flight run through
//! it, and turns into the objects of `docs/debrief-api.md` (`wire`): the
//! flight list entry, the full debrief and the post-landing notice. `fdm`
//! and the desktop app share it, so both show the same numbers.

#![forbid(unsafe_code)]

mod assessment;
pub mod export;
mod index;
mod skills;
mod trends;
mod wire;

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use analyzer::{Basis, Cause, Flight, Phase, Span, Track, phases, thresholds, touchdowns};
use assess::Method;
use fdm_core::geo::distance_m;
use fdm_core::{Lfl, SimTime};
use recorder::{COMPACT_EXT, Recorder, ReplaySource, SimEvent, TOUCHDOWN_WINDOW_US};
use rules::{LoadError, PhaseField, ProfileField, ProfileSet, Rule, Selection, Sourced};

pub use assessment::{POINTS, domain_title};
pub use index::{INDEX_FILE, Index};
pub use skills::{
    Automation, Control, Fuel, PhaseAutomation, Reversals, Ride, Skills, SkillsSummary, skills,
};
pub use trends::{TrendFilter, Trends, trends};
pub use wire::*;

pub type Error = Box<dyn std::error::Error>;

const CORE_RULES: &str = include_str!("../../../rules/core.yaml");
const RAW_EXT: &str = ".raw.jsonl.gz";
const JOURNAL_EXT: &str = "fqj";
/// Stored flights, the compact form first: it wins where both exist, as
/// while a journal is being compacted.
const FLIGHT_EXTS: [&str; 2] = [COMPACT_EXT, JOURNAL_EXT];
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
    if file_name(path).ends_with(RAW_EXT) {
        return Flight::read(&record(path)?[..]);
    }
    Flight::read(BufReader::new(File::open(path)?))
}

/// The journal of a raw capture, recorded through the on-board chain.
fn record(raw: &Path) -> io::Result<Vec<u8>> {
    let mut recorder = Recorder::new(&Lfl::V1, Vec::new())?;
    recorder.run(&mut ReplaySource::open(raw, recorder.acquisition())?)?;
    Ok(recorder.into_inner())
}

/// A flight's id: its file name without `.fqj` / `.fqz` / `.raw.jsonl.gz`.
pub fn flight_id(path: &Path) -> String {
    let name = file_name(path);
    suffixes()
        .iter()
        .find_map(|ext| name.strip_suffix(ext.as_str()))
        .unwrap_or(&name)
        .to_owned()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// The file name endings of a flight's files: raw capture, journal, compact.
fn suffixes() -> [String; 3] {
    [
        RAW_EXT.to_owned(),
        format!(".{JOURNAL_EXT}"),
        format!(".{COMPACT_EXT}"),
    ]
}

fn has_ext(path: &Path, ext: &str) -> bool {
    path.extension().is_some_and(|e| e == ext)
}

fn files(dir: &Path) -> impl Iterator<Item = PathBuf> {
    let entries = fs::read_dir(dir).into_iter().flatten().flatten();
    entries.map(|e| e.path())
}

/// The stored flights in `dir` (journals and compact flights, one file per
/// flight), in no particular order.
pub fn flight_files(dir: &Path) -> Vec<PathBuf> {
    files(dir)
        .filter(|p| {
            has_ext(p, COMPACT_EXT)
                || has_ext(p, JOURNAL_EXT) && !p.with_extension(COMPACT_EXT).exists()
        })
        .collect()
}

/// The uncompacted journals in `dir`.
pub fn journals(dir: &Path) -> Vec<PathBuf> {
    files(dir).filter(|p| has_ext(p, JOURNAL_EXT)).collect()
}

/// Compacts each journal (see `recorder::compact_file`), reporting on stderr.
/// A journal that fails stays as it is. Returns how many did not compact.
pub fn compact_all(journals: impl IntoIterator<Item = PathBuf>) -> usize {
    let mut left = 0;
    for journal in journals {
        match recorder::compact_file(&journal) {
            Ok(dest) => eprintln!("compacted: {}", dest.display()),
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
    for ext in suffixes() {
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
    /// The landing assessment (`scoring/landing-3.yaml`).
    method: Method,
    /// Hash of the rules, the profiles, the scoring method and the analysis
    /// code (`build.rs`): a summary made by another station is stale.
    version: String,
}

impl Station {
    pub fn new(rules: Vec<Rule>) -> Result<Self, Error> {
        let profiles = ProfileSet::shipped()?;
        let method = Method::landing_3()?;
        let mut h = DefaultHasher::new();
        format!("{rules:?}{profiles:?}{method:?}{}", env!("SOURCE_HASH")).hash(&mut h);
        Ok(Self {
            rules,
            profiles,
            method,
            version: format!("{:016x}", h.finish()),
        })
    }

    /// The core rules and the shipped profiles.
    pub fn core() -> Result<Self, Error> {
        Self::new(core_rules()?)
    }

    pub fn method(&self) -> &Method {
        &self.method
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn profiles(&self) -> &ProfileSet {
        &self.profiles
    }

    /// `None` when the flight has no air/ground data to analyse. The type
    /// profile's unreliable signals are read as no data throughout.
    pub fn analyse(&self, id: String, mut flight: Flight) -> Option<Analysis<'_>> {
        let selection = self.profiles.select(&flight);
        selection.mask(&mut flight);
        let track = Track::new(&flight)?;
        Some(Analysis {
            events: rules::evaluate(&self.rules, &selection, &flight, &track),
            touchdowns: touchdowns(&flight, &track),
            phases: phases(&track, &selection.phases),
            id,
            rules: &self.rules,
            method: &self.method,
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
    method: &'a Method,
}

impl Analysis<'_> {
    /// The rules the events come from.
    pub fn rules(&self) -> &[Rule] {
        self.rules
    }

    /// The landing assessment method.
    pub fn method(&self) -> &Method {
        self.method
    }

    pub fn summary(&self) -> FlightSummary {
        let events = self.events();
        let skills = SkillsSummary::from(&skills(self));
        self.summary_of(&events, &self.assessment(&events), skills)
    }

    fn summary_of(
        &self,
        events: &[Event],
        detail: &AssessmentDetail,
        skills: SkillsSummary,
    ) -> FlightSummary {
        let t = &self.track.t;
        let mut event_counts: BTreeMap<_, usize> = Level::ALL.map(|l| (l, 0)).into();
        for e in events {
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
            assessment: detail.into(),
            landings_detail: self.landings_detail(detail),
            skills,
            flown: Flown::of(
                &self.track,
                &self.flight.resample("sim.slew", &self.track.t),
            ),
            rule_counts: detail
                .parts
                .iter()
                .filter(|p| p.level >= Level::Low)
                .map(|p| {
                    let count = RuleCount {
                        level: p.level,
                        count: p.count,
                    };
                    (p.rule.clone(), count)
                })
                .collect(),
        }
    }

    /// Each landing's facts; a bounce counts toward the landing before it.
    fn landings_detail(&self, detail: &AssessmentDetail) -> Vec<LandingFacts> {
        let mut assessed = detail.landings.iter();
        let mut facts: Vec<LandingFacts> = Vec::new();
        for td in &self.touchdowns {
            if !td.bounce {
                let Some(a) = assessed.next() else { break };
                facts.push(LandingFacts::new(&Touchdown::new(td, &self.flight), a));
            } else if let Some(last) = facts.last_mut() {
                last.bounces += 1;
            }
        }
        facts
    }

    pub fn debrief(&self) -> Debrief {
        let events = self.events();
        let assessment_detail = self.assessment(&events);
        let skills = skills(self);
        Debrief {
            flight: FlightHeader {
                summary: self.summary_of(&events, &assessment_detail, (&skills).into()),
                lfl_version: self.flight.lfl_version(),
                profile: self.selection.to_string(),
                class: self.selection.class.map(|c| c.to_string()),
                identification: (&self.selection).into(),
                profile_limits: self.profile_limits(),
                phase_values: PhaseField::ALL
                    .map(|f| (f.name(), self.selection.phase_value(f)))
                    .into(),
                assessment_detail,
                damaged_at: self.flight.damaged_at,
            },
            phases: self
                .phases
                .iter()
                .map(|s| PhaseSpan {
                    phase: s.phase.name(),
                    start_s: secs(s.start),
                    end_s: secs(s.end),
                    trigger: phase_trigger(&s.cause),
                })
                .collect(),
            touchdowns: self
                .touchdowns
                .iter()
                .map(|td| Touchdown::new(td, &self.flight))
                .collect(),
            events,
            series: self.series(),
            skills,

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
                touchdown: Touchdown::new(td, &self.flight),
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
            level: e.level,
            start_s: secs(e.start),
            end_s: secs(e.end),
            peak_s: secs(e.peak),
            measure: e.measure.clone(),
            value: e.value,
            threshold: e.threshold,
            unit: e.unit.map(|u| u.symbol()).filter(|s| !s.is_empty()),
            why_it_matters: rule.and_then(|r| text(r.why_it_matters.as_deref())),
            advice: rule.and_then(|r| text(r.advice.as_deref())),
            detail: e.detail.clone(),
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
            level: Level::Advisory,
            start_s: secs(t),
            end_s: secs(t),
            peak_s: secs(t),
            measure: String::new(),
            value: None,
            threshold: None,
            unit: None,
            why_it_matters: None,
            advice: None,
            detail: None,
        }
    }

    /// The profile's flap-speed limit at `flap_deg`, as the flap-overspeed
    /// rule reads it.
    fn vfe(&self, flap_deg: f64) -> Option<f64> {
        let profile = self.selection.profile?;
        profile.value(ProfileField::Vfe, || {
            Some(flap_deg).filter(|a| a.is_finite())
        })
    }

    /// The profile's values that do not depend on the flight state (all
    /// but VFE, which depends on the flaps).
    fn profile_limits(&self) -> BTreeMap<&'static str, Sourced> {
        let sourced = |f: ProfileField| {
            let value = self.selection.profile?.sourced(f, || None)?;
            Some((f.name(), value.clone()))
        };
        ProfileField::ALL.into_iter().filter_map(sourced).collect()
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
        let times: Vec<SimTime> = points.iter().map(|&i| track.t[i]).collect();
        let param = |name| self.flight.resample(name, &times);
        let flap_deg: Vec<f64> = param("flap.te_angle");
        // The profile's flap-speed limit at each point; NaN without a profile band or with the flaps up.
        let vfe_kt = flap_deg
            .iter()
            .map(|&a| self.vfe(a).unwrap_or(f64::NAN))
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
            hdg_deg: param("heading.mag"),
            ra_ft: pick(&track.ra_ft),
            flap_deg,
            vfe_kt,
            gear_ext: param("gear.extended"),
            ap_engaged: param("ap.engaged"),
            at_engaged: param("at.engaged"),
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

/// What opened a phase span, in words with its value there, e.g. "on the
/// ground above 30 kt (34 kt)"; a value that would read as its threshold
/// gets a decimal.
pub fn phase_trigger(c: &Cause) -> String {
    use analyzer::Trigger::*;
    let th = format!("{:.0}", c.threshold);
    let what = match c.trigger {
        Stopped => format!("stopped, under {th} kt on the ground"),
        Moving => format!("moving on the ground under {th} kt"),
        Roll => format!("on the ground above {th} kt"),
        TouchAndGo => "lowest speed of the touch-and-go roll, takeoff power applied".to_owned(),
        Liftoff => format!("{th} ft above the runway"),
        ClimbEnd => format!("climbing through {th} ft above the aerodrome"),
        TopOfClimb => format!("top of climb, under {th} ft above the aerodrome"),
        Descent => format!("descending through {th} ft above the aerodrome"),
        Flare => format!("flare, below {th} ft above the runway"),
        Touchdown => "touchdown".to_owned(),
        GoAround => format!("lowest point of the approach, then a climb of {th} ft"),
        Airborne => return "airborne when the recording starts".to_owned(),
    };
    let unit = match c.basis {
        Basis::GroundSpeed => "kt",
        Basis::Ra => "ft RA",
        Basis::Aal => "ft AAL",
    };
    let v = Some(format!("{:.0}", c.value))
        .filter(|v| *v != th)
        .unwrap_or_else(|| format!("{:.1}", c.value));
    match c.value.is_finite() {
        true => format!("{what} ({v} {unit})"),
        false => what,
    }
}

/// A rule text as shown: trimmed, `None` when empty.
fn text(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Unix seconds of the recording start, from a `flight-<unix seconds>` id,
/// as `foqa-rec` names them.
fn start_unix(id: &str) -> Option<i64> {
    id.strip_prefix("flight-")?.parse().ok()
}

/// ISO 8601 UTC from a `flight-<unix seconds>` id.
fn started_utc(id: &str) -> Option<String> {
    Some(iso_utc(start_unix(id)? * 1_000_000, false))
}

/// `2026-10-28T09:41:05Z` for µs since the Unix epoch, with milliseconds
/// (`09:41:05.250Z`) when `millis`.
fn iso_utc(unix_us: i64, millis: bool) -> String {
    let (days, us) = (
        unix_us.div_euclid(86_400_000_000),
        unix_us.rem_euclid(86_400_000_000),
    );
    let s = us / 1_000_000;
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
    let fraction = match millis {
        true => format!(".{:03}", us / 1000 % 1000),
        false => String::new(),
    };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}{fraction}Z",
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::*;

    pub(crate) const C172: &str = concat!(
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
        assert_eq!(d.flight.summary.worst_level, Some(Level::Medium));
        assert_eq!(d.events.len(), 5);
        assert!(d.events.iter().all(|e| !e.description.is_empty()));
        assert!(d.events.iter().all(|e| e.advice.is_some()));
        // Every measured value is said in words.
        assert!(
            d.events
                .iter()
                .all(|e| e.value.is_none() || e.detail.is_some())
        );
        assert!(
            d.events
                .iter()
                .all(|e| (e.start_s..=e.end_s).contains(&e.peak_s))
        );
        let [td] = &d.touchdowns[..] else {
            panic!("{:?}", d.touchdowns)
        };
        assert!((td.sink_fpm - 103.0).abs() < 5.0, "{}", td.sink_fpm);
        // The wind, when recorded, is the sampled wind resolved on the runway.
        let source = &c172().touchdowns[0];
        let speed = c172().flight.at("wind.speed", source.t);
        let from = c172().flight.at("wind.dir", source.t);
        match (&td.wind, &source.runway, speed, from) {
            (Some(w), Some(r), Some(speed), Some(from)) => {
                let (head, cross) = analyzer::wind_components(speed, from, r.heading_deg);
                assert_eq!(
                    (w.speed_kt, w.from_deg, w.headwind_kt, w.crosswind_kt),
                    (speed, from, head, cross)
                );
            }
            (None, runway, speed, from) => {
                assert!(runway.is_none() || speed.is_none() || from.is_none());
            }
            other => panic!("{other:?}"),
        }

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
            &s.hdg_deg,
            &s.ra_ft,
            &s.flap_deg,
            &s.vfe_kt,
            &s.gear_ext,
            &s.ap_engaged,
            &s.at_engaged,
            &s.lat,
            &s.lon,
        ]
        .map(Vec::len);
        assert_eq!(lengths, [n; 17]);
        // What the replay's instruments read: heading in range, radio height
        // only low down, the discretes as 0 or 1.
        let known = |v: &[f64]| {
            v.iter()
                .copied()
                .filter(|x| x.is_finite())
                .collect::<Vec<_>>()
        };
        assert!(known(&s.hdg_deg).iter().all(|h| (0.0..=360.0).contains(h)));
        assert!(!known(&s.ra_ft).is_empty());
        assert!(known(&s.ra_ft).iter().all(|&h| h <= 2500.0));
        for discrete in [&s.ap_engaged, &s.at_engaged] {
            assert!(known(discrete).iter().all(|&x| x == 0.0 || x == 1.0));
        }
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
        // The capture predates the identity: nothing is known, every type
        // limit is off, and it is still taken for an airplane.
        let id = &flight["identification"];
        assert_eq!(
            (id["matched"].as_str(), id["runway_landing"].as_bool()),
            (Some("unknown"), Some(true))
        );
        assert_eq!(id["unchecked"].as_array().unwrap().len(), 9);
        assert_eq!(flight["profile"], "no aircraft identity, unknown class");
        assert_eq!(flight["worst_level"], "medium");

        assert_eq!(
            flight["event_counts"],
            serde_json::json!({"advisory": 2, "low": 0, "medium": 3, "high": 0})
        );
        // Unknown optional fields are left out, not null.
        assert!(
            flight.get("aircraft").is_none()
                && flight.get("class").is_none()
                && flight.get("damaged_at").is_none()
        );
        assert_eq!(v["phases"][0]["phase"], "Standing");
        let takeoff = &v["phases"][2];
        assert_eq!(takeoff["phase"], "Takeoff");
        let roll = takeoff["trigger"].as_str().unwrap();
        assert!(roll.starts_with("on the ground above 30 kt ("), "{roll}");
        // No class: the transport boundaries.
        let flare = &flight["phase_values"]["flare_ft"];
        assert_eq!(flare["value"], 50.0);
        assert_eq!(flare["source"], "transport class default (unknown class)");
        assert_eq!(v["events"][0]["level"], "medium");
    }

    /// The summary carries what the trends read: each landing and each rule
    /// that fired at low or above.
    #[test]
    fn the_summary_carries_landing_facts_and_rule_counts() {
        let s = c172().summary();
        let [l] = &s.landings_detail[..] else {
            panic!("{:?}", s.landings_detail)
        };
        let d = c172().debrief();
        assert_eq!(
            (l.t_s, l.sink_fpm),
            (d.touchdowns[0].t_s, d.touchdowns[0].sink_fpm)
        );
        assert_eq!(l.score, s.assessment.score);
        assert_eq!(l.bounces, 0);
        let medium = s.event_counts[&Level::Medium];
        let counted: usize = s.rule_counts.values().map(|c| c.count).sum();
        assert_eq!(counted, medium);
        assert!(s.rule_counts.values().all(|c| c.level >= Level::Low));
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
        assert_eq!((report.level, report.start_s), (Level::Advisory, 100.0));
        // With the two 500 ft gate notes of the pattern.
        let counts = &a.summary().event_counts;
        assert_eq!(counts[&Level::Advisory], 3);
    }

    #[test]
    fn deleting_a_flight_takes_its_files_only() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let names = [
            "flight-1.fqj",
            "flight-1.raw.jsonl.gz",
            "flight-10.fqz",
            "flight-1.txt",
        ];
        for name in names {
            fs::write(dir.join(name), b"x").unwrap();
        }
        assert_eq!(delete_flight(dir, "flight-1").unwrap(), 2);
        let left: Vec<_> = names.iter().filter(|n| dir.join(n).exists()).collect();
        assert_eq!(left, [&"flight-10.fqz", &"flight-1.txt"]);
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
