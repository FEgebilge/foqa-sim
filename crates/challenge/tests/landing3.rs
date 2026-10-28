//! `landing-3` challenges (docs/scoring.md §7.6): ranked only for types
//! with a Vref source, scored by the shared assessment, a go-around never
//! ranked. The synthetic landing (`testkit`) is flown as a C172, whose POH
//! approach speeds are in its profile.

use challenge::{Attempt, Challenge, LANDING_2, LANDING_3, evaluate};
use debrief::{AssessmentDetail, Station};
use recorder::SimEvent;
use testkit::*;

const CHALLENGE: &str = r#"{
  "id": "test-20", "version": 1, "title": "Runway 20",
  "airport": "TEST", "runway": "20",
  "aircraft": { "types": ["C172"] },
  "start": { "distance_nm": 4, "height_ft": 1300, "ias_kt": 140, "gear_down": true, "flaps": 1, "mode": "self" },
  "gate": { "distance_nm": 3 },
  "scoring": { "version": "landing-3" }
}"#;

fn attempt(change: impl Fn(f64, Pt) -> Pt) -> Attempt {
    graded(change).0
}

/// The attempt and the debrief's assessment of the same flight.
fn graded(change: impl Fn(f64, Pt) -> Pt) -> (Attempt, AssessmentDetail) {
    graded_on(CHALLENGE, change)
}

fn graded_on(json: &str, change: impl Fn(f64, Pt) -> Pt) -> (Attempt, AssessmentDetail) {
    let challenge = Challenge::parse(json).unwrap();
    let events = [
        identity("C172"),
        SimEvent::Airport(runway_behind_touchdown(400.0)),
    ];
    let (flight, _) = fly_to(&events, AIRLINER_LB, TD_S + ROLLOUT_S, |t| {
        change(t, landing(t, TD_S, 1.2))
    });
    let station = Station::core().unwrap();
    let analysis = station.analyse("flight-1".into(), flight).unwrap();
    let detail = analysis.assessment(&analysis.events());
    (evaluate(&challenge, &analysis), detail)
}

fn failed(a: &Attempt) -> Vec<&str> {
    a.checks.iter().filter(|c| !c.ok).map(|c| c.name).collect()
}

#[test]
fn only_types_with_a_vref_source_are_ranked() {
    let parse = |from: &str, to: &str| Challenge::parse(&CHALLENGE.replace(from, to));
    assert!(parse("", "").is_ok());
    assert_eq!(
        parse(r#""C172""#, r#""C172", "SR22""#).unwrap_err(),
        "no Vref source for SR22: landing-3 cannot rank it"
    );
    assert_eq!(
        parse(r#""types": ["C172"]"#, r#""classes": ["light"]"#).unwrap_err(),
        "landing-3 needs `aircraft.types`, each with a Vref source"
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
    assert_eq!(score.version, LANDING_3);
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

/// A challenge naming the retired landing-2 parses and is graded with
/// landing-3, the attempt saying so.
#[test]
fn a_landing_2_challenge_is_graded_with_landing_3() {
    let a = graded_on(&CHALLENGE.replace(LANDING_3, LANDING_2), |_, p| p).0;
    assert_eq!(a.score, attempt(|_, p| p).score);
    assert_eq!(a.score.unwrap().version, LANDING_3);
    assert_eq!(a.assessment.unwrap().method, LANDING_3);
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

/// The attempt carries the debrief's grade, confidence and caps, and its
/// score is `score.total`.
#[test]
fn the_attempt_carries_the_debriefs_assessment() {
    let (a, detail) = graded(|_, p| p);
    let x = a.assessment.unwrap();
    assert_eq!(x.method, detail.method);
    assert_eq!(x.method, LANDING_3);
    assert_eq!(Some(x.score), detail.score);
    assert_eq!(x.score, a.score.unwrap().total);
    assert_eq!(x.grade, detail.grade);
    assert_eq!(Some(x.confidence), detail.confidence);
    assert!(!x.caps.is_empty());
    let strip = |caps: &[debrief::CapNote]| -> Vec<_> {
        caps.iter()
            .map(|c| (c.kind.clone(), c.score, c.reason.clone()))
            .collect()
    };
    assert_eq!(strip(&x.caps), strip(&detail.caps));
}
