//! `landing-2` challenges (docs/scoring.md §7.6): ranked only for types
//! with a Vref source, scored by the shared assessment, a go-around never
//! ranked. The synthetic landing (`testkit`) is flown as a C172, whose POH
//! approach speeds are in its profile.

use challenge::{Attempt, Challenge, LANDING_2, evaluate};
use debrief::Station;
use recorder::SimEvent;
use testkit::*;

const CHALLENGE: &str = r#"{
  "id": "test-20", "version": 1, "title": "Runway 20",
  "airport": "TEST", "runway": "20",
  "aircraft": { "types": ["C172"] },
  "start": { "distance_nm": 4, "height_ft": 1300, "ias_kt": 140, "gear_down": true, "flaps": 1, "mode": "self" },
  "gate": { "distance_nm": 3 },
  "scoring": { "version": "landing-2" }
}"#;

fn attempt(change: impl Fn(f64, Pt) -> Pt) -> Attempt {
    let challenge = Challenge::parse(CHALLENGE).unwrap();
    let events = [
        identity("C172"),
        SimEvent::Airport(runway_behind_touchdown(400.0)),
    ];
    let (flight, _) = fly_to(&events, AIRLINER_LB, TD_S + ROLLOUT_S, |t| {
        change(t, landing(t, TD_S, 1.2))
    });
    let station = Station::core().unwrap();
    evaluate(
        &challenge,
        &station.analyse("flight-1".into(), flight).unwrap(),
    )
}

fn failed(a: &Attempt) -> Vec<&str> {
    a.checks.iter().filter(|c| !c.ok).map(|c| c.name).collect()
}

#[test]
fn only_types_with_a_vref_source_are_ranked() {
    let parse = |from: &str, to: &str| Challenge::parse(&CHALLENGE.replace(from, to));
    assert!(parse("", "").is_ok());
    assert_eq!(
        parse(r#""C172""#, r#""C172", "B738""#).unwrap_err(),
        "no Vref source for B738: landing-2 cannot rank it"
    );
    assert_eq!(
        parse(r#""types": ["C172"]"#, r#""classes": ["light"]"#).unwrap_err(),
        "landing-2 needs `aircraft.types`, each with a Vref source"
    );
}

/// The attempt is scored by the shared module: its total is the debrief's
/// landing score as a ranked attempt, the parts its six domains.
#[test]
fn an_attempt_is_scored_by_the_shared_assessment() {
    let a = attempt(|_, p| p);
    assert!(a.valid, "{:?}", a.checks);
    assert!(a.checks.iter().any(|c| c.name == "data" && c.ok));
    let score = a.score.unwrap();
    assert_eq!(score.version, LANDING_2);
    let names: Vec<_> = score.parts.iter().map(|p| p.name).collect();
    assert_eq!(
        names,
        [
            "stability",
            "energy",
            "point",
            "lateral",
            "contact",
            "rollout"
        ]
    );
    let weights: f64 = score.parts.iter().map(|p| p.weight).sum();
    assert!((weights - 100.0).abs() < 1e-9);
    // An airliner's 140 kt is far above the C172's 70 kt approach speed.
    let stability = &score.parts[0];
    assert!(stability.score < 0.5, "{stability:?}");
    assert!(score.total < 85.0, "{}", score.total);
}

/// Down to 250 ft inside the gate, up to 500 ft, then the landing: a
/// go-around, so the attempt is not ranked.
#[test]
fn a_go_around_is_not_ranked() {
    let a = attempt(|t, p| Pt {
        alt: p
            .alt
            .min(lerp(&[(55.0, 2000.0), (65.0, 250.0), (85.0, 2000.0)], t)),
        ..p
    });
    assert_eq!(failed(&a), ["go_around"], "{:?}", a.checks);
    assert!(!a.valid);
}
