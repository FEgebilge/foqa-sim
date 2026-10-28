use std::io::{self, Read};
use std::ops::Range;

use fdm_core::geo::distance_m;
use fdm_core::{AircraftId, Airport, Lfl, Sample, SignalDef, SignalId, SimTime, Unit};
use recorder::{FlightRecords, FlowEvent, Record, SimEvent, read_records};

/// Parameters whose value wraps around, as (name, lowest value, period).
/// Interpolating across the wrap-around must take the short way round
/// (359 deg to 1 deg passes through 0, not 180).
const WRAPPING: [(&str, f64, f64); 4] = [
    ("heading.mag", 0.0, 360.0),
    ("sel.heading", 0.0, 360.0),
    ("wind.dir", 0.0, 360.0),
    ("pos.lon", -180.0, 360.0),
];

/// Positions that stand for an FDR discrete, read as their last sample like
/// one: `gear.extended` is the gear position of 14 CFR 121.344 Appendix M
/// item 36 (up, in transit, down and locked), sampled every 4 s. A line
/// between two samples would invent a transit the recorder never saw (0 to
/// 100 % across 4 s) and give short-sustain rules (gear operated above VLO,
/// 1 s) a 4 s ramp to fire on.
const STEPPED: [&str; 1] = ["gear.extended"];
/// An aircraft the sim placed (spawn, slew, teleport) settles on its gear
/// within this long: no touchdown in it is a landing.
pub(crate) const SETTLE_US: i64 = 5_000_000;
/// Faster over the ground than any aircraft flies (Concorde cruised at
/// ~1,200 kt): a position that moved faster jumped (teleport, flight
/// reload), the only trace of one in recordings without flow events. m/s.
const JUMP_MS: f64 = 2000.0 * 1852.0 / 3600.0;

/// Recordings made before the wiring scaled `GEAR TOTAL PCT EXTENDED` (a
/// 0-1 fraction in MSFS 2024, docs/format.md) hold `gear.extended` as 0-1
/// and lack the `GearScaledV1` marker. Of those, a series that never passes
/// 1 is a fraction: a true % series reaches 100 with the gear down, and an
/// always-retracted one is 0 on both scales. Marked recordings are left as is.
fn gear_fraction_to_pct(samples: &mut [Sample]) {
    const MAX_FRACTION: f64 = 1.0 + 1e-6;
    if samples.iter().all(|s| s.value <= MAX_FRACTION) {
        samples.iter_mut().for_each(|s| s.value *= 100.0);
    }
}

/// A decoded recording: each parameter's samples in time order.
pub struct Flight {
    lfl: &'static Lfl,
    series: Vec<Vec<Sample>>,
    pub events: Vec<(SimTime, SimEvent)>,
    /// Offset of a cut-off tail, if the recorder was killed.
    pub damaged_at: Option<u64>,
}

impl Flight {
    pub fn read(input: impl Read) -> io::Result<Self> {
        Self::from_records(read_records(input)?)
    }

    /// The flight of records already opened (e.g. with a lower cap).
    pub fn from_records<R: Read>(mut reader: FlightRecords<R>) -> io::Result<Self> {
        let lfl = &Lfl::V1;
        if reader.lfl_version() != lfl.version {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "journal uses an unknown LFL version",
            ));
        }
        let mut series = vec![Vec::new(); lfl.signals().len()];
        let mut events = Vec::new();
        for record in reader.by_ref() {
            match record? {
                Record::Sample(s) => {
                    if let Some(v) = series.get_mut(usize::from(s.signal.0)) {
                        v.push(s);
                    }
                }
                Record::Event(t, e) => events.push((t, e)),
            }
        }
        // The journal is not time-ordered (touchdown pre-trigger buffer).
        series.iter_mut().for_each(|s| s.sort_by_key(|s| s.t));
        events.sort_by_key(|(t, _)| *t);
        let marked = events.iter().any(|(_, e)| *e == SimEvent::GearScaledV1);
        if let Some(gear) = lfl.by_name("gear.extended").filter(|_| !marked) {
            gear_fraction_to_pct(&mut series[usize::from(gear.id.0)]);
        }
        Ok(Self {
            lfl,
            series,
            events,
            damaged_at: reader.damaged_at(),
        })
    }

    /// Samples of a parameter, in time order. Panics on a name the LFL lacks.
    pub fn series(&self, name: &str) -> &[Sample] {
        &self.series[usize::from(self.def(name).id.0)]
    }

    /// Every parameter with its samples, in LFL order.
    pub fn parameters(&self) -> impl Iterator<Item = (&'static SignalDef, &[Sample])> {
        self.lfl
            .signals()
            .iter()
            .zip(self.series.iter().map(Vec::as_slice))
    }

    /// The airport snapshots recorded in flight, one per ICAO, the latest
    /// kept (the sim's runway data can change mid-flight, e.g. on a scenery
    /// update).
    pub fn airports(&self) -> Vec<Airport> {
        let mut out: Vec<Airport> = Vec::new();
        for airport in self.events.iter().filter_map(|(_, e)| match e {
            SimEvent::Airport(a) => Some(a),
            _ => None,
        }) {
            match out.iter_mut().find(|a| a.icao == airport.icao) {
                Some(old) => *old = airport.clone(),
                None => out.push(airport.clone()),
            }
        }
        out
    }

    /// The aircraft flown: the latest identity recorded, since the sim
    /// reports it again whenever it changes. `None` for a recording made
    /// before identities were captured. Its `icao_type()` selects the type
    /// profile.
    pub fn aircraft(&self) -> Option<&AircraftId> {
        self.events.iter().rev().find_map(|(_, e)| match e {
            SimEvent::Aircraft(id) => Some(id),
            _ => None,
        })
    }

    /// Where the sim, not the pilot, moved the aircraft: slew, a teleport,
    /// a flight (re)load, skip or back-on-track (flow events), or a position
    /// jump; each with `SETTLE_US` after it. No touchdown in one is a
    /// landing, and no approach or rollout is graded across one. A pause is
    /// none: the recorder drops frames whose sim time stands still, so it
    /// leaves no samples and no jump.
    pub fn breaks(&self) -> Vec<Range<SimTime>> {
        let Some((_, end)) = self.span() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut open: Option<SimTime> = None;
        for s in self.series("sim.slew").iter().filter(|s| s.is_valid()) {
            match (open, s.value >= 0.5) {
                (None, true) => open = Some(s.t),
                (Some(from), false) => {
                    out.push(from..s.t);
                    open = None;
                }
                _ => {}
            }
        }
        out.extend(open.take().map(|from| from..end));
        for &(t, ref e) in &self.events {
            use FlowEvent::*;
            match e {
                SimEvent::Flow(FltLoad | TeleportStart | BackOnTrackStart | SkipStart) => {
                    open.get_or_insert(t);
                }
                SimEvent::Flow(FltLoaded | TeleportDone | BackOnTrackDone | SkipDone)
                | SimEvent::FlightLoaded(_) => out.push(open.take().unwrap_or(t)..t),
                _ => {}
            }
        }
        out.extend(open.map(|from| from..end));
        let fixes: Vec<_> = (self.series("pos.lat").iter())
            .filter(|s| s.is_valid())
            .filter_map(|s| Some((s.t, (s.value, self.at("pos.lon", s.t)?))))
            .collect();
        out.extend(
            fixes
                .windows(2)
                .filter(|w| {
                    distance_m(w[0].1, w[1].1) > JUMP_MS * w[1].0.us_since(w[0].0) as f64 / 1e6
                })
                .map(|w| w[0].0..w[1].0),
        );
        for b in &mut out {
            b.end = b.end.add_us(SETTLE_US);
        }
        out
    }

    /// When the sim reported a crash: its `Crashed` system event or the
    /// `PlaneCrash` flow event, sent only with crash detection on.
    pub fn crashes(&self) -> impl Iterator<Item = SimTime> + '_ {
        (self.events.iter())
            .filter(|(_, e)| matches!(e, SimEvent::Crashed | SimEvent::Flow(FlowEvent::PlaneCrash)))
            .map(|&(t, _)| t)
    }

    pub fn lfl_version(&self) -> u16 {
        self.lfl.version
    }

    /// First and last recorded time.
    pub fn span(&self) -> Option<(SimTime, SimTime)> {
        let first = self
            .series
            .iter()
            .filter_map(|s| s.first())
            .map(|s| s.t)
            .min()?;
        let last = self
            .series
            .iter()
            .filter_map(|s| s.last())
            .map(|s| s.t)
            .max()?;
        Some((first, last))
    }

    /// Value at `t`: interpolated between valid neighbours for continuous
    /// parameters, the last value for discrete and `STEPPED` ones. `None` outside the
    /// recording, across invalid (non-`Normal`) samples, or across a gap of
    /// more than two missed samples.
    pub fn at(&self, name: &str, t: SimTime) -> Option<f64> {
        self.value(self.def(name).id, t)
    }

    /// `at` over `times`, `NaN` where a parameter has no valid value.
    pub fn resample(&self, name: &str, times: &[SimTime]) -> Vec<f64> {
        let id = self.def(name).id;
        times
            .iter()
            .map(|&t| self.value(id, t).unwrap_or(f64::NAN))
            .collect()
    }

    /// `at` by parameter id; `None` for an id the LFL lacks.
    pub fn value(&self, id: SignalId, t: SimTime) -> Option<f64> {
        let def = self.lfl.get(id)?;
        let s = &self.series[usize::from(id.0)];
        let max_gap = (2.5e6 / def.rate_hz) as i64;
        let next = s.partition_point(|x| x.t <= t);
        let prev = s.get(next.checked_sub(1)?).filter(|p| p.is_valid())?;
        if prev.t == t {
            return Some(prev.value);
        }
        if matches!(def.unit, Unit::Discrete | Unit::Index) || STEPPED.contains(&def.name) {
            return (t.us_since(prev.t) <= max_gap).then_some(prev.value);
        }
        let next = s
            .get(next)
            .filter(|n| n.is_valid() && n.t.us_since(prev.t) <= max_gap)?;
        let f = t.us_since(prev.t) as f64 / next.t.us_since(prev.t) as f64;
        let mut delta = next.value - prev.value;
        let Some(&(_, low, period)) = WRAPPING.iter().find(|w| w.0 == def.name) else {
            return Some(prev.value + f * delta);
        };
        delta -= period * (delta / period).round();
        Some(low + (prev.value + f * delta - low).rem_euclid(period))
    }

    fn def(&self, name: &str) -> &'static SignalDef {
        self.lfl
            .by_name(name)
            .unwrap_or_else(|| panic!("LFL lacks {name}"))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use fdm_core::Ssm;

    use super::*;

    pub(crate) type Row = (&'static str, f64, f64, Ssm);

    /// A valid sample of `name` at `secs`.
    pub(crate) fn ok(name: &'static str, secs: f64, value: f64) -> Row {
        (name, secs, value, Ssm::Normal)
    }

    /// A flight built straight from samples, bypassing the journal.
    pub(crate) fn flight(rows: &[Row]) -> Flight {
        let lfl = &Lfl::V1;
        let mut series = vec![Vec::new(); lfl.signals().len()];
        for &(name, secs, value, ssm) in rows {
            let id = lfl.by_name(name).unwrap().id;
            let t = SimTime::from_secs_f64(secs);
            series[usize::from(id.0)].push(Sample {
                ssm,
                ..Sample::new(t, id, value)
            });
        }
        series.iter_mut().for_each(|s| s.sort_by_key(|s| s.t));
        Flight {
            lfl,
            series,
            events: Vec::new(),
            damaged_at: None,
        }
    }

    /// Value a fraction `f` of the way between two samples half a second apart.
    fn at(name: &'static str, a: f64, b: f64, f: f64) -> f64 {
        let flight = flight(&[ok(name, 0.0, a), ok(name, 0.5, b)]);
        flight.at(name, SimTime::from_secs_f64(0.5 * f)).unwrap()
    }

    /// `gear.extended` as read back from a journal holding `values`,
    /// with or without the `GearScaledV1` marker.
    fn gear_read(values: &[f64], marked: bool) -> Vec<f64> {
        let lfl = &Lfl::V1;
        let id = lfl.by_name("gear.extended").unwrap().id;
        let mut journal = recorder::JournalWriter::new(Vec::new(), lfl).unwrap();
        if marked {
            journal
                .event(SimTime::ZERO, &SimEvent::GearScaledV1)
                .unwrap();
        }
        for (i, &v) in values.iter().enumerate() {
            let t = SimTime::from_secs_f64(i as f64);
            journal.sample(&Sample::new(t, id, v)).unwrap();
        }
        let flight = Flight::read(&journal.into_inner()[..]).unwrap();
        flight
            .series("gear.extended")
            .iter()
            .map(|s| s.value)
            .collect()
    }

    #[test]
    fn old_fraction_gear_reads_as_percent() {
        assert_eq!(gear_read(&[0.0, 0.5, 1.0], false), [0.0, 50.0, 100.0]);
        assert_eq!(gear_read(&[0.0, 50.0, 100.0], false), [0.0, 50.0, 100.0]);
        assert_eq!(gear_read(&[0.0, 0.0], false), [0.0, 0.0]);
    }

    #[test]
    fn marked_gear_is_never_rescaled() {
        // A new recording cut mid-retraction: under 1 %, but already percent.
        assert_eq!(gear_read(&[0.0, 0.5], true), [0.0, 0.5]);
    }

    #[test]
    fn airports_keep_the_latest_snapshot_per_icao() {
        let airport = |icao: &str, elevation_m| Airport {
            icao: icao.into(),
            region: String::new(),
            lat_deg: 0.0,
            lon_deg: 0.0,
            elevation_m,
            magvar_deg: 0.0,
            runways: Vec::new(),
        };
        let mut f = flight(&[]);
        f.events = [("LTBJ", 1.0), ("LTFM", 2.0), ("LTBJ", 3.0)]
            .map(|(icao, e)| {
                (
                    SimTime::from_secs_f64(e),
                    SimEvent::Airport(airport(icao, e)),
                )
            })
            .into();
        let seen: Vec<_> = f
            .airports()
            .iter()
            .map(|a| (a.icao.clone(), a.elevation_m))
            .collect();
        assert_eq!(seen, [("LTBJ".to_owned(), 3.0), ("LTFM".to_owned(), 2.0)]);
    }

    #[test]
    fn aircraft_is_the_latest_identity() {
        let id = |title: &str| AircraftId {
            title: title.into(),
            atc_model: "ATCCOM.AC_MODEL C172.0.text".into(),
            atc_type: "ATCCOM.ATC_NAME CESSNA.0.text".into(),
            atc_id: "BILGE".into(),
        };
        let mut f = flight(&[]);
        assert_eq!(f.aircraft(), None);
        f.events = ["C172SP G1000 Passengers", "C172SP Classic"]
            .map(|t| (SimTime::ZERO, SimEvent::Aircraft(id(t))))
            .into();
        assert_eq!(f.aircraft(), Some(&id("C172SP Classic")));
        assert_eq!(f.aircraft().unwrap().icao_type().as_deref(), Some("C172"));
    }

    #[test]
    fn gear_position_holds_its_last_sample() {
        let f = flight(&[
            ok("gear.extended", 0.0, 0.0),
            ok("gear.extended", 4.0, 100.0),
        ]);
        let at = |s| f.at("gear.extended", SimTime::from_secs_f64(s));
        assert_eq!(
            (at(2.0), at(3.9), at(4.0)),
            (Some(0.0), Some(0.0), Some(100.0))
        );
    }

    #[test]
    fn angles_interpolate_the_short_way_round() {
        assert!((at("heading.mag", 359.0, 1.0, 0.5) - 0.0).abs() < 1e-9);
        assert!((at("heading.mag", 350.0, 10.0, 0.25) - 355.0).abs() < 1e-9);
        assert!((at("wind.dir", 10.0, 350.0, 0.75) - 355.0).abs() < 1e-9);
        assert!((at("pos.lon", 179.0, -179.0, 0.25) - 179.5).abs() < 1e-9);
        assert!((at("pos.lon", 179.0, -179.0, 0.75) + 179.5).abs() < 1e-9);
        // Ordinary parameters are untouched.
        assert!((at("pitch", 359.0, 1.0, 0.5) - 180.0).abs() < 1e-9);
    }
}
