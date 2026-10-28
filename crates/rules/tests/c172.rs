//! The core rule set on the M1 C172 traffic pattern. That approach was
//! genuinely unstable (sim's own data: ~2800 fpm descent in a 42° banked
//! turn through 1000 ft at 105–114 kt), and the events must say so.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use analyzer::{Flight, Track};
use fdm_core::Lfl;
use recorder::{Recorder, ReplaySource};
use rules::{Aircraft, Level, ProfileSet, Selection, evaluate, load};

fn c172() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/c172-pattern.raw.jsonl.gz")
}
const CORE: &str = include_str!("../../../rules/core.yaml");

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

fn events(yaml: &str) -> Vec<(String, Level, Option<f64>)> {
    // Recorded before aircraft identities and weight: graded with no type
    // profile, class unknown, so with the transport phase boundaries.
    let profiles = ProfileSet::shipped().unwrap();
    graded(yaml, &profiles.select(&flight().0))
}

fn graded(yaml: &str, selection: &Selection) -> Vec<(String, Level, Option<f64>)> {
    let (flight, track) = flight();
    evaluate(&load(yaml).unwrap(), selection, flight, track)
        .into_iter()
        .map(|e| (e.rule, e.level, e.value))
        .collect()
}

#[test]
fn core_rules_flag_the_unstable_approach() {
    let found: Vec<(String, Level)> = events(CORE)
        .into_iter()
        .map(|(rule, level, _)| (rule, level))
        .collect();
    // Graded as transport (unknown class): Data4Safety's second thresholds
    // above 500 ft (SL2A) for the bank and the descent, the first below it
    // (SL2B) for the bank; the 500 ft gate notes the ALAR limits.
    let expected = [
        ("bank-above-500", Level::Medium),
        ("high-sink-above-500", Level::Medium),
        ("bank-below-500", Level::Medium),
        ("unstable-sink-500", Level::Advisory),
        ("unstable-bank-500", Level::Advisory),
    ];
    assert_eq!(found, expected.map(|(r, l)| (r.to_owned(), l)));
}

/// Graded as the light aircraft it is: the 42 deg turn through 1000 ft is
/// past the 30 deg of a shallow-to-medium pattern turn, and the ~2800 fpm
/// dive past the 2000 fpm medium level; below 500 ft the sink and bank were
/// not held long enough, so the gate notes remain. The fixed gear reads
/// down: no gear event.
#[test]
fn light_rules_flag_the_pattern_dive_and_turn() {
    let profiles = ProfileSet::shipped().unwrap();
    let light = profiles.select_type(None, || Some(Aircraft::Light));
    let rules: Vec<_> = graded(CORE, &light)
        .into_iter()
        .map(|(rule, level, _)| (rule, level))
        .collect();
    let found = |rule: &str, level| (rule.to_owned(), level);
    assert_eq!(
        rules,
        [
            found("bank-light-above-300", Level::Low),
            found("high-sink-light-above-500", Level::Medium),
            found("unstable-sink-500", Level::Advisory),
            found("unstable-bank-500", Level::Advisory),
        ]
    );
}

#[test]
fn peak_descent_matches_the_sims_own_vertical_speed() {
    let (_, _, peak) = events(CORE)
        .into_iter()
        .find(|(rule, ..)| rule == "high-sink-above-500")
        .unwrap();
    let peak = peak.unwrap();
    // The sim's own VERTICAL SPEED peaked at 2787 fpm in this descent.
    assert!((peak - 2787.0).abs() < 300.0, "{peak:.0} fpm");
}

#[test]
fn touchdown_and_condition_rules_fire_on_their_trigger() {
    let yaml = "
rule: firm-touchdown
version: 1
domain: other
title: Touchdown firmer than 50 fpm
at: touchdown
measure: td.sink
above: { low: 50 fpm }
---
rule: airborne-past-200s
version: 1
domain: other
title: Airborne after 200 s
phase: [EnRoute, Approach]
when: not air_ground
measure: true
sustain: 5 s
level: advisory
";
    let found = events(yaml);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].0, "airborne-past-200s");
    let (rule, level, sink) = &found[1];
    assert_eq!((rule.as_str(), *level), ("firm-touchdown", Level::Low));
    assert!((sink.unwrap() - 103.0).abs() < 5.0);
}
