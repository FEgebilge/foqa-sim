//! The debrief's landing-2 assessment of a synthetic landing (`testkit`),
//! against a snapshot of its wire form. `UPDATE_SNAPSHOTS=1` rewrites it.

use std::fs;
use std::path::Path;

use debrief::Station;

use recorder::SimEvent;
use serde_json::Value;
use testkit::*;

const SNAPSHOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/snapshots/assessment.json"
);

/// Numbers to 4 decimals, so the last bits of a platform's maths do not
/// matter.
fn rounded(v: Value) -> Value {
    match v {
        Value::Number(n) => n
            .as_f64()
            .filter(|_| !n.is_i64() && !n.is_u64())
            .map_or(Value::Number(n), |x| {
                serde_json::json!((x * 1e4).round() / 1e4)
            }),
        Value::Array(a) => Value::Array(a.into_iter().map(rounded).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, rounded(v))).collect()),
        v => v,
    }
}

/// The stable airliner landing on runway 20, 400 m past the threshold,
/// with a 1.6 g touchdown.
fn analysed() -> debrief::Debrief {
    let events = [
        identity("B738"),
        SimEvent::Airport(runway_behind_touchdown(400.0)),
    ];
    let (flight, _) = fly_to(&events, AIRLINER_LB, TD_S + ROLLOUT_S, |t| {
        landing(t, TD_S, 1.6)
    });
    let station = Station::core().unwrap();
    station
        .analyse("flight-1".into(), flight)
        .unwrap()
        .debrief()
}

#[test]
fn the_assessment_matches_its_snapshot() {
    let d = analysed();
    let a = &d.flight.assessment_detail;
    assert_eq!(a.method, "landing-2");
    assert_eq!(a.graded_by, Some(0));
    let [landing] = &a.landings[..] else {
        panic!("{:?}", a.landings)
    };
    assert_eq!(landing.status, "scored");
    assert_eq!(landing.domains.len(), 6);
    // The summary carries the flight's landing-2 grade and score.
    let s = &d.flight.summary.assessment;
    assert_eq!(
        (s.method.as_str(), s.score, s.grade),
        ("landing-2", a.score, a.grade)
    );
    // Every finding names its event; every domain that lost points says why.
    for dom in &landing.domains {
        for f in &dom.findings {
            let e = &d.events[f.event.expect("an event")];
            assert_eq!(e.rule, f.rule);
        }
        if dom.points > 0.0 {
            let set = dom.criteria.iter().filter(|c| c.sets).count()
                + dom.findings.iter().filter(|f| f.sets).count();
            assert_eq!(set, 1, "{dom:?}");
        }
        // The words the app and the EFB show come with the numbers.
        let why = dom.set_by_text.as_deref();
        if let Some(c) = dom.criteria.iter().find(|c| c.sets) {
            assert!(
                why.unwrap()
                    .starts_with(&format!("{} {}", c.title, c.value_text))
            );
        }
        for c in &dom.criteria {
            let half = c.height_text.as_deref();
            match c.height {
                assess::Height::All => assert_eq!(half, None),
                assess::Height::Above => assert_eq!(half, Some("above 500 ft")),
                assess::Height::Below => assert_eq!(half, Some("below 500 ft")),
            }
        }
    }
    assert_eq!(landing.split_ft, Some(500.0), "a jet's split");
    let lost: f64 = landing.domains.iter().map(|d| d.points).sum();
    assert!((100.0 - lost - landing.raw.unwrap()).abs() < 0.3, "{lost}");

    let json = rounded(serde_json::to_value(a).unwrap());
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() || !Path::new(SNAPSHOT).exists() {
        let text = serde_json::to_string_pretty(&json).unwrap() + "\n";
        fs::write(SNAPSHOT, text).unwrap();
    }
    let saved: Value = serde_json::from_str(&fs::read_to_string(SNAPSHOT).unwrap()).unwrap();
    assert_eq!(json, rounded(saved), "UPDATE_SNAPSHOTS=1 to accept");
}
