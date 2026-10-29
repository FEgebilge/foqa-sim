//! A C172 flight at LTBJ flown in MSFS 2024: a dive and climb flying away
//! from the field, a go-around from 800 ft on final, a touch-and-go on 16L
//! and a full-stop landing on 34R.

use std::fs::File;
use std::path::Path;

use analyzer::{Flight, LiveInput, LiveTrack, Phase, PhaseConfig, Track, phases};
use recorder::SimEvent;

fn flight() -> (Flight, Track) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/c172-ltbj-go-around-touch-and-go.fqz");
    let flight = Flight::read(File::open(path).unwrap()).unwrap();
    let track = Track::new(&flight).unwrap();
    (flight, track)
}

/// The phases in order with their start, s from the first point.
fn spans(track: &Track) -> Vec<(Phase, f64)> {
    let t0 = track.t[0];
    (phases(track, &PhaseConfig::LIGHT).into_iter())
        .map(|s| (s.phase, s.start.us_since(t0) as f64 / 1e6))
        .collect()
}

#[test]
fn only_the_go_around_on_final_is_one() {
    let (_, track) = flight();
    let spans = spans(&track);
    let go_arounds: Vec<f64> = (spans.iter())
        .filter(|s| s.0 == Phase::GoAround)
        .map(|s| s.1)
        .collect();
    // Not the dive at 172 s, which flew away from both runways.
    assert_eq!(go_arounds.len(), 1, "{spans:?}");
    assert!((go_arounds[0] - 683.0).abs() < 2.0, "{spans:?}");
    use Phase::*;
    let sequence: Vec<Phase> = spans.iter().map(|s| s.0).collect();
    assert_eq!(
        sequence,
        [
            Standing,
            Taxi,
            Takeoff,
            InitialClimb,
            EnRoute,
            Approach,
            GoAround,
            EnRoute,
            Approach,
            Landing,
            // The touch-and-go.
            Takeoff,
            InitialClimb,
            EnRoute,
            Approach,
            Landing,
            Taxi,
            Standing
        ]
    );
}

#[test]
fn the_live_phases_see_the_same_go_around() {
    let (flight, track) = flight();
    let airports: Vec<_> = (flight.events.iter())
        .filter_map(|(t, e)| match e {
            SimEvent::Airport(a) => Some((*t, a)),
            _ => None,
        })
        .collect();
    let mut live = LiveTrack::default();
    let mut seen = Vec::new();
    for &t in &track.t {
        for (_, airport) in airports.iter().filter(|(at, _)| *at <= t) {
            live.airport(airport);
        }
        let at = |name| flight.at(name, t).unwrap_or(f64::NAN);
        let point = live.step(
            &LiveInput {
                t,
                on_ground: flight.at("air_ground", t).map(|v| v >= 0.5),
                alt_ft: at("alt.pressure"),
                gs_kt: at("groundspeed"),
                ra_ft: at("alt.radio"),
                nz_g: at("accel.normal"),
                pitch_deg: at("pitch"),
                roll_deg: at("roll"),
                pos: flight.at("pos.lat", t).zip(flight.at("pos.lon", t)),
            },
            &PhaseConfig::LIGHT,
        );
        if seen
            .last()
            .is_none_or(|l: &(Phase, f64)| l.0 != point.phase)
        {
            seen.push((point.phase, t.us_since(track.t[0]) as f64 / 1e6));
        }
    }
    eprintln!("{seen:?}");
    let go_arounds: Vec<f64> = (seen.iter())
        .filter(|s| s.0 == Phase::GoAround)
        .map(|s| s.1)
        .collect();
    // Live, it is known once the climb has risen 200 ft: past 1000 ft.
    assert_eq!(go_arounds.len(), 1, "{seen:?}");
    assert!((683.0..720.0).contains(&go_arounds[0]), "{seen:?}");
}
