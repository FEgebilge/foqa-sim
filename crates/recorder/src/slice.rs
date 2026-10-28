//! The recording slice of one attempt (docs/challenge-protocol.md A5): the
//! part of a flight a site may receive, cut by the attempt's markers and
//! stripped of what identifies the pilot or the PC.

use std::collections::BTreeMap;
use std::io::{self, Read};

use fdm_core::geo::distance_m;
use fdm_core::{AircraftId, Lfl, Sample, SimTime};

use crate::{AttemptMarker, AttemptPhase, Recording, SimEvent, read_records};

/// Kept before the begin marker and after the end marker, µs.
pub const BEFORE_US: i64 = 10_000_000;
pub const AFTER_US: i64 = 5_000_000;
/// An airport snapshot recorded before the window is kept when the
/// aircraft passes this close to it inside the window, m: the runways an
/// attempt lands on.
const AIRPORT_M: f64 = 10_000.0;

/// What to cut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cut<'a> {
    /// The attempt with this marker reference: from `BEFORE_US` before its
    /// latest begin marker to `AFTER_US` after the end marker that follows,
    /// or to the end of the data without one. Only its own markers are kept.
    Attempt(&'a str),
    /// Sim times, inclusive; every marker inside is kept.
    Range(SimTime, SimTime),
}

/// A slice and the window it covers.
#[derive(Debug)]
pub struct Slice {
    pub recording: Recording,
    pub from: SimTime,
    pub to: SimTime,
    /// The end marker was found and the data reaches `AFTER_US` past it:
    /// nothing more will be added by recording on.
    pub complete: bool,
}

impl Slice {
    /// The compact flight bytes: the same input gives the same bytes.
    pub fn to_bytes(&self) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        self.recording.write(&mut out)?;
        Ok(out)
    }
}

/// Reads a journal or compact flight and cuts it; `None` when the
/// attempt's begin marker is not in it.
pub fn slice(input: impl Read, cut: Cut) -> io::Result<Option<Slice>> {
    let records = read_records(input)?;
    let recording = Recording::from_records(records.lfl_version(), records)?;
    Ok(recording.slice(cut))
}

impl Recording {
    /// The slice `cut` of this recording (A5): samples inside the window
    /// plus each parameter's last one before it; the aircraft identity in
    /// force at its start and any change inside, with the tail number
    /// (`atc_id`) and sim title blanked; the airport snapshots inside it
    /// and those the aircraft passes near; sim events and markers inside
    /// it. Never kept: the flight-loaded event (a local file path), pilot
    /// reports, anything else outside the window. Sim times are kept.
    pub fn slice(&self, cut: Cut) -> Option<Slice> {
        let end = self.series.values().flatten().map(|s| s.t).max()?;
        let (from, to, only, complete) = match cut {
            Cut::Range(from, to) => (from, to, None, false),
            Cut::Attempt(reference) => {
                let marks = |phase| move |(_, e): &&(SimTime, SimEvent)| matches!(e, SimEvent::Attempt(m) if m.phase == phase && ours(m, reference));
                let begin = self
                    .events
                    .iter()
                    .filter(marks(AttemptPhase::Begin))
                    .map(|(t, _)| *t)
                    .max()?;
                let ended = self
                    .events
                    .iter()
                    .filter(marks(AttemptPhase::End))
                    .map(|(t, _)| *t)
                    .filter(|t| *t >= begin)
                    .min();
                let to = ended.map_or(end, |t| t.add_us(AFTER_US));
                let complete = ended.is_some() && end >= to;
                (begin.add_us(-BEFORE_US), to, Some(reference), complete)
            }
        };
        let inside = |t: SimTime| (from..=to).contains(&t);

        let mut series = BTreeMap::new();
        for (id, samples) in &self.series {
            // The last before the window, the latest written at equal times.
            let before = samples
                .iter()
                .filter(|s| s.t < from)
                .reduce(|a, b| if b.t >= a.t { b } else { a });
            let kept: Vec<Sample> = before
                .into_iter()
                .chain(samples.iter().filter(|s| inside(s.t)))
                .copied()
                .collect();
            if !kept.is_empty() {
                series.insert(*id, kept);
            }
        }

        let positions = self.positions(from, to);
        let near = |lat: f64, lon: f64| {
            positions
                .iter()
                .any(|&p| distance_m(p, (lat, lon)) <= AIRPORT_M)
        };
        // Before the window: the identity in force and the latest snapshot
        // of each airport passed near, by journal position.
        let earlier = || {
            self.events
                .iter()
                .enumerate()
                .filter(|(_, (t, _))| *t < from)
        };
        let identity = earlier()
            .rfind(|(_, (_, e))| matches!(e, SimEvent::Aircraft(_)))
            .map(|(i, _)| i);
        let mut airports: BTreeMap<&str, usize> = BTreeMap::new();
        for (i, (_, e)) in earlier() {
            if let SimEvent::Airport(a) = e
                && near(a.lat_deg, a.lon_deg)
            {
                airports.insert(&a.icao, i);
            }
        }
        let carried = |i: usize| {
            Some(i) == identity
                || airports.values().any(|&k| k == i)
                || self.events[i].1 == SimEvent::GearScaledV1
        };
        let events = self
            .events
            .iter()
            .enumerate()
            .filter(|&(i, (t, e))| {
                let kept = match e {
                    SimEvent::FlightLoaded(_) | SimEvent::PilotReport(_) => false,
                    SimEvent::Attempt(m) => only.is_none_or(|r| ours(m, r)),
                    _ => true,
                };
                kept && (inside(*t) || *t < from && carried(i))
            })
            .map(|(_, (t, e))| (*t, anonymous(e)))
            .collect();
        Some(Slice {
            recording: Recording {
                lfl_version: self.lfl_version,
                series,
                events,
            },
            from,
            to,
            complete,
        })
    }

    /// Valid positions recorded inside the window, as (lat, lon).
    fn positions(&self, from: SimTime, to: SimTime) -> Vec<(f64, f64)> {
        let lfl = &Lfl::V1;
        let (Some(lat), Some(lon)) = (lfl.by_name("pos.lat"), lfl.by_name("pos.lon")) else {
            return Vec::new();
        };
        if self.lfl_version != lfl.version {
            return Vec::new();
        }
        let valid = |id: u16| -> BTreeMap<SimTime, f64> {
            let samples = self.series.get(&id).into_iter().flatten();
            samples
                .filter(|s| s.is_valid() && (from..=to).contains(&s.t))
                .map(|s| (s.t, s.value))
                .collect()
        };
        let lons = valid(lon.id.0);
        valid(lat.id.0)
            .into_iter()
            .filter_map(|(t, lat)| Some((lat, *lons.get(&t)?)))
            .collect()
    }
}

fn ours(m: &AttemptMarker, reference: &str) -> bool {
    m.reference.as_deref() == Some(reference)
}

/// The event with the aircraft's tail number and sim title blanked; the
/// ATC model and type stay, for the type profile.
fn anonymous(e: &SimEvent) -> SimEvent {
    match e {
        SimEvent::Aircraft(id) => SimEvent::Aircraft(AircraftId {
            title: String::new(),
            atc_id: String::new(),
            ..id.clone()
        }),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use fdm_core::{Airport, SignalId};

    use super::*;
    use crate::airport_json::tests::ltbj;
    use crate::{JournalWriter, Record};

    const REF: &str = "Qe3v1hJ0dEa9fP2kLm7xRw";

    fn s(secs: f64) -> SimTime {
        SimTime::from_secs_f64(secs)
    }

    fn marker(phase: AttemptPhase, reference: &str) -> SimEvent {
        SimEvent::Attempt(AttemptMarker {
            phase,
            challenge: "ltbj-16l".into(),
            version: 1,
            reference: Some(reference.into()),
            reason: (phase == AttemptPhase::End).then(|| "stopped".into()),
        })
    }

    fn far(icao: &str) -> Airport {
        Airport {
            icao: icao.into(),
            lat_deg: 41.0,
            ..ltbj()
        }
    }

    /// 0–400 s at 1 Hz; the aircraft sits on LTBJ's reference point. An
    /// earlier attempt is replaced at 95 s, ours flies 100–300 s.
    fn journal() -> Vec<u8> {
        let mut w = JournalWriter::new(Vec::new(), &Lfl::V1).unwrap();
        let ap = ltbj();
        let (lat, lon) = (
            Lfl::V1.by_name("pos.lat").unwrap().id,
            Lfl::V1.by_name("pos.lon").unwrap().id,
        );
        let at = |t: f64, e: SimEvent| (s(t), e);
        let mut events = vec![
            at(
                0.0,
                SimEvent::FlightLoaded(r"C:\Users\Ege\Flights\x.FLT".into()),
            ),
            at(0.0, SimEvent::Aircraft(crate::aircraft_json::tests::c172())),
            at(1.0, SimEvent::Airport(far("LTFJ"))),
            at(2.0, SimEvent::Airport(ap.clone())),
            at(50.0, marker(AttemptPhase::Begin, "OlderAttempt1")),
            at(95.0, marker(AttemptPhase::End, "OlderAttempt1")),
            at(100.0, marker(AttemptPhase::Begin, REF)),
            at(150.0, SimEvent::PilotReport("unstable".into())),
            at(300.0, marker(AttemptPhase::End, REF)),
        ];
        events.reverse();
        for i in 0..=400 {
            let t = s(f64::from(i));
            while events.last().is_some_and(|(et, _)| *et <= t) {
                let (et, e) = events.pop().unwrap();
                w.event(et, &e).unwrap();
            }
            for (id, v) in [
                (lat, ap.lat_deg),
                (lon, ap.lon_deg),
                (SignalId(3), f64::from(i)),
            ] {
                w.sample(&Sample::new(t, id, v)).unwrap();
            }
        }
        w.into_inner()
    }

    fn cut(cut: Cut) -> Slice {
        slice(&journal()[..], cut).unwrap().unwrap()
    }

    fn events(slice: &Slice) -> Vec<(f64, SimEvent)> {
        let r = &slice.recording;
        r.events
            .iter()
            .map(|(t, e)| (t.as_secs_f64(), e.clone()))
            .collect()
    }

    #[test]
    fn an_attempt_is_cut_from_its_markers_with_the_margins() {
        let slice = cut(Cut::Attempt(REF));
        assert_eq!(
            (slice.from, slice.to, slice.complete),
            (s(90.0), s(305.0), true)
        );
        let times: Vec<f64> = slice.recording.series[&3].iter().map(|x| x.value).collect();
        assert_eq!(times[0], 89.0, "the last sample before the window");
        assert_eq!(
            times[1..],
            (90..=305).map(f64::from).collect::<Vec<_>>()[..]
        );
        let kept = events(&slice);
        let anon = AircraftId {
            title: String::new(),
            atc_id: String::new(),
            ..crate::aircraft_json::tests::c172()
        };
        assert_eq!(
            kept,
            [
                (0.0, SimEvent::Aircraft(anon)),
                (2.0, SimEvent::Airport(ltbj())),
                (100.0, marker(AttemptPhase::Begin, REF)),
                (300.0, marker(AttemptPhase::End, REF)),
            ],
            "no far airport, flight file, pilot report or other attempt"
        );
    }

    #[test]
    fn without_an_end_marker_the_slice_runs_to_the_end_of_the_data() {
        let bytes = journal();
        let records = crate::JournalReader::new(&bytes[..]).unwrap();
        let mut r = Recording::from_records(1, records).unwrap();
        r.events.retain(|(t, _)| *t != s(300.0));
        let slice = r.slice(Cut::Attempt(REF)).unwrap();
        assert_eq!((slice.to, slice.complete), (s(400.0), false));
        assert!(r.slice(Cut::Attempt("NoSuchAttempt")).is_none());
        let range = cut(Cut::Range(s(40.0), s(96.0)));
        let markers = events(&range)
            .iter()
            .filter(|(_, e)| matches!(e, SimEvent::Attempt(_)))
            .count();
        assert_eq!(markers, 2, "a range keeps every marker inside");
    }

    #[test]
    fn the_slice_carries_no_tail_number_title_or_path() {
        let bytes = cut(Cut::Attempt(REF)).to_bytes().unwrap();
        assert_eq!(&bytes[..4], b"FQJC");
        let body = zstd::decode_all(&bytes[8..]).unwrap();
        let c172 = crate::aircraft_json::tests::c172();
        for secret in [
            c172.atc_id.as_str(),
            &c172.title,
            "Users",
            "unstable",
            "OlderAttempt1",
        ] {
            for hay in [&body[..], &bytes[..]] {
                assert!(
                    !hay.windows(secret.len()).any(|w| w == secret.as_bytes()),
                    "{secret}"
                );
            }
        }
        assert!(body.windows(4).any(|w| w == b"C172"), "the type stays");
        // And it reads back as a compact flight.
        let back: Vec<Record> = read_records(&bytes[..])
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(
            back.iter()
                .any(|r| matches!(r, Record::Event(_, SimEvent::Attempt(_))))
        );
    }

    #[test]
    fn the_same_flight_gives_the_same_bytes() {
        let a = cut(Cut::Attempt(REF)).to_bytes().unwrap();
        let b = cut(Cut::Attempt(REF)).to_bytes().unwrap();
        assert_eq!(a, b);
        // Compacted first, the flight slices the same.
        let mut compact = Vec::new();
        let bytes = journal();
        let records = crate::JournalReader::new(&bytes[..]).unwrap();
        Recording::from_records(1, records)
            .unwrap()
            .write(&mut compact)
            .unwrap();
        let c = slice(&compact[..], Cut::Attempt(REF)).unwrap().unwrap();
        assert_eq!(c.to_bytes().unwrap(), a);
    }
}
