//! Ground-segment analysis of the M1 C172 traffic pattern, recorded through
//! the on-board chain from its raw capture.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use analyzer::{Approach, Flight, Phase, PhaseConfig, Track, phases, sustained_worst, touchdowns};
use fdm_core::Lfl;
use recorder::{Recorder, ReplaySource};

fn c172() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/c172-pattern.raw.jsonl.gz")
}

fn flight() -> &'static (Flight, Track) {
    static FLIGHT: OnceLock<(Flight, Track)> = OnceLock::new();
    FLIGHT.get_or_init(|| {
        let mut recorder = Recorder::new(&Lfl::V1, Vec::new()).unwrap();
        recorder
            .run(&mut ReplaySource::open(c172(), recorder.acquisition()).unwrap())
            .unwrap();
        let flight = Flight::read(&recorder.into_inner()[..]).unwrap();
        let track = Track::new(&flight).unwrap();
        (flight, track)
    })
}

#[test]
fn phases_follow_a_traffic_pattern() {
    let (_, track) = flight();
    let spans = phases(track, &PhaseConfig::LIGHT);
    for s in &spans {
        eprintln!(
            "{:>7.1} {:>7.1}  {:?}",
            s.start.as_secs_f64(),
            s.end.as_secs_f64(),
            s.phase
        );
    }
    let sequence: Vec<Phase> = spans.iter().map(|s| s.phase).collect();
    use Phase::*;
    assert_eq!(
        sequence,
        [
            Standing,
            Taxi,
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
fn inertial_vs_tracks_the_sims_own_vertical_speed() {
    let (flight, track) = flight();
    let mut errors: Vec<f64> = (0..track.len())
        .filter(|&i| !track.on_ground[i])
        .filter_map(|i| Some((track.vs_fpm[i] - flight.at("sim.vs", track.t[i])?).abs()))
        .collect();
    errors.sort_by(f64::total_cmp);
    let p50 = errors[errors.len() / 2];
    let p90 = errors[errors.len() * 9 / 10];
    eprintln!("inertial VS error vs sim: p50 {p50:.0} fpm, p90 {p90:.0} fpm");
    // MSFS pressure altitude is itself this noisy against the sim's
    // (geometric) VS: even a full-rate ±1 s baro difference is p50 85 /
    // p90 242 fpm. The bound guards against regressions, not perfection.
    assert!(p50 < 90.0 && p90 < 240.0, "p50 {p50:.0} / p90 {p90:.0} fpm");
}

#[test]
fn single_soft_touchdown_matches_the_sim() {
    let (flight, track) = flight();
    let tds = touchdowns(flight, track);
    eprintln!("{tds:#?}");
    assert_eq!(tds.len(), 1);
    let td = &tds[0];
    assert!(!td.bounce);
    let sim = td.sim_sink_fpm.unwrap();
    assert!(
        (td.sink_fpm - sim).abs() < 15.0,
        "sink {:.0} vs sim {sim:.0} fpm",
        td.sink_fpm
    );
    assert!((td.peak_g - 1.13).abs() < 0.02, "peak {:.2} g", td.peak_g);
    assert!(td.pitch_deg.unwrap() > 3.0, "touchdown should be nose-up");
}

/// Regression snapshot of the approach values of the pattern's landing,
/// over the light window (500 ft to the 20 ft flare). The recording has no
/// airport data, so the runway values are unknown.
#[test]
fn approach_values_of_the_pattern_landing() {
    let (flight, track) = flight();
    let td = &touchdowns(flight, track)[0];
    let a = Approach::of(flight, track, td, 500.0, 20.0);
    let w = &a.window;
    let sink = sustained_worst(&w.t, &w.sink_fpm, 3_000_000);
    let roll = w.roll_deg.iter().map(|r| r.abs()).collect::<Vec<_>>();
    let roll = sustained_worst(&w.t, &roll, 3_000_000);
    let span = w.t[w.t.len() - 1].us_since(w.t[0]) as f64 / 1e6;
    eprintln!(
        "window {span:.1} s, float {:?}, sink {sink:?}, roll {roll:?}",
        a.float_s
    );
    assert!((span - 29.5).abs() < 0.2, "{span}");
    assert!((a.float_s.unwrap() - 13.3).abs() < 0.1);
    // A steep, turning final: 1338 fpm and 24.5 deg held for 3 s.
    assert!((sink.unwrap().1 - 1338.0).abs() < 5.0);
    assert!((roll.unwrap().1 - 24.5).abs() < 0.2);
    // Only the flap handle is recorded: no flap angle, no changes.
    assert!(a.config_changes.is_empty());
    assert_eq!(a.threshold, None);
    assert_eq!((a.crab_deg, a.drift_kt, a.slowed), (None, None, None));
}
