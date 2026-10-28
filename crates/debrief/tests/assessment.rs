//! The debrief's landing-3 assessment of a synthetic landing (`testkit`),
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
    assert_eq!(a.method, "landing-3");
    assert_eq!(a.graded_by, Some(0));
    let [landing] = &a.landings[..] else {
        panic!("{:?}", a.landings)
    };
    assert_eq!(landing.status, "scored");
    assert_eq!(landing.domains.len(), 6);
    // The summary carries the flight's landing-3 grade and score.
    let s = &d.flight.summary.assessment;
    assert_eq!(
        (s.method.as_str(), s.score, s.grade),
        ("landing-3", a.score, a.grade)
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

    // The profile's arrays are checked below, not snapshotted.
    let mut json = serde_json::to_value(a).unwrap();
    for l in json["landings"].as_array_mut().unwrap() {
        l.as_object_mut().unwrap().remove("profile");
    }
    let json = rounded(json);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() || !Path::new(SNAPSHOT).exists() {
        let text = serde_json::to_string_pretty(&json).unwrap() + "\n";
        fs::write(SNAPSHOT, text).unwrap();
    }
    let saved: Value = serde_json::from_str(&fs::read_to_string(SNAPSHOT).unwrap()).unwrap();
    assert_eq!(json, rounded(saved), "UPDATE_SNAPSHOTS=1 to accept");
}

/// The profile of the only landing of `script` flown into `events`.
fn profile_of(events: &[SimEvent], script: impl Fn(f64) -> Pt) -> Option<debrief::ApproachProfile> {
    let (flight, _) = fly_to(events, AIRLINER_LB, TD_S + ROLLOUT_S, script);
    let d = Station::core()
        .unwrap()
        .analyse("flight-1".into(), flight)
        .unwrap()
        .debrief();
    d.flight.assessment_detail.landings[0].profile.clone()
}

/// A 140 kt approach on the 3 degree path to a 50 ft threshold crossing,
/// the threshold 400 m before the touchdown (`runway_behind_touchdown`),
/// flared over the last metres.
fn on_path(t: f64) -> Pt {
    let threshold_s = TD_S - 400.0 / (140.0 * KT_MS);
    let ft_s = 140.0 * KT_MS / 0.3048 * 3f64.to_radians().tan();
    let alt = lerp(
        &[
            (0.0, 50.0 + ft_s * threshold_s),
            (threshold_s, 50.0),
            (TD_S, 0.0),
        ],
        t,
    );
    Pt {
        alt,
        ..landing(t, TD_S, 1.2)
    }
}

fn airport() -> [SimEvent; 2] {
    [
        identity("B738"),
        SimEvent::Airport(runway_behind_touchdown(400.0)),
    ]
}

#[test]
fn the_profile_follows_the_threshold_and_the_path() {
    let p = profile_of(&airport(), on_path).expect("a profile");
    let n = p.t_s.len();
    assert!(n > 50, "{n}");
    assert!(
        [&p.dist_nm, &p.height_ft, &p.gs_dot, &p.loc_dot]
            .iter()
            .all(|v| v.len() == n)
    );
    assert_eq!((p.path_deg, p.tch_ft), (3.0, 50.0));
    assert_eq!((p.top_ft, p.bottom_ft), (1000.0, 50.0));
    // Closing on the threshold at 140 kt, 72 m/s.
    assert!(p.dist_nm.windows(2).all(|w| w[1] < w[0]));
    let ahead = p.dist_nm.last().unwrap();
    assert!((0.0..0.25).contains(ahead), "{ahead}");
    // Above the flare the height is the 3 degree path's.
    for (&d, &h) in p
        .dist_nm
        .iter()
        .zip(&p.height_ft)
        .filter(|(d, _)| **d > 0.2)
    {
        let path = 50.0 + d * 1852.0 / 0.3048 * 3f64.to_radians().tan();
        assert!((h - path).abs() < 10.0, "{d} nm: {h} ft, path {path} ft");
    }
    // No ILS recorded: unknown, not zero.
    assert!(p.gs_dot.iter().chain(&p.loc_dot).all(|x| x.is_nan()));
}

#[test]
fn the_profile_carries_the_ils_deviation_and_needs_a_runway() {
    let p = profile_of(&airport(), |t| Pt {
        set: vec![
            ("NAV HAS GLIDE SLOPE:1", 1.0),
            ("NAV GSI:1", 1.5 * 119.0 / 2.0),
        ],
        ..on_path(t)
    })
    .unwrap();
    assert!(
        p.gs_dot.iter().all(|x| (x.abs() - 1.5).abs() < 0.1),
        "{:?}",
        p.gs_dot
    );
    assert!(p.loc_dot.iter().all(|x| x.is_nan()));
    assert_eq!(profile_of(&[identity("B738")], on_path), None);
}
