//! Ground-segment analysis of the M1 C172 traffic pattern, recorded through
//! the on-board chain from its raw capture.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use analyzer::{Flight, Phase, PhaseConfig, Track, phases, touchdowns};
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
