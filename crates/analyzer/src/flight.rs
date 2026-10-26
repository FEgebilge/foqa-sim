use std::io::{self, Read};

use fdm_core::{AircraftId, Airport, Lfl, Sample, SignalDef, SignalId, SimTime, Unit};
use recorder::{Record, SimEvent, read_records};

/// Parameters whose value wraps around, as (name, lowest value, period).
/// Interpolating across the wrap-around must take the short way round
/// (359 deg to 1 deg passes through 0, not 180).
const WRAPPING: [(&str, f64, f64); 4] = [
    ("heading.mag", 0.0, 360.0),
    ("sel.heading", 0.0, 360.0),
    ("wind.dir", 0.0, 360.0),
    ("pos.lon", -180.0, 360.0),
];

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
        let mut reader = read_records(input)?;
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
    /// parameters, the last value for discrete ones. `None` outside the
    /// recording, across invalid (non-`Normal`) samples, or across a gap of
    /// more than two missed samples.
    pub fn at(&self, name: &str, t: SimTime) -> Option<f64> {
        self.value(self.def(name).id, t)
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
        if matches!(def.unit, Unit::Discrete | Unit::Index) {
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
