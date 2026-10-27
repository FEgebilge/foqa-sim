//! Attempts flown as synthetic flights (`testkit`): the stable airliner
//! approach onto runway 20 of `TEST`, touching down 400 m past the
//! threshold, starting about 4.6 NM out.

use challenge::{Attempt, Challenge, evaluate};
use debrief::Station;
use recorder::SimEvent;
use testkit::*;

const CHALLENGE: &str = r#"{
  "id": "test-20", "version": 3, "title": "Runway 20",
  "airport": "TEST", "runway": "20",
  "aircraft": { "classes": ["transport"] },
  "start": { "distance_nm": 4, "height_ft": 1300, "ias_kt": 140, "gear_down": true, "flaps": 1, "mode": "self" },
  "gate": { "distance_nm": 3 },
  "scoring": { "version": "landing-1" }
}"#;

/// The stable landing changed by `change`, graded against `CHALLENGE` with
/// each `(from, to)` replaced.
fn attempt(edits: &[(&str, &str)], change: impl Fn(f64, Pt) -> Pt) -> Attempt {
    let json = edits
        .iter()
        .fold(CHALLENGE.to_owned(), |j, (a, b)| j.replace(a, b));
    let challenge = Challenge::parse(&json).unwrap();
    let events = [SimEvent::Airport(runway_behind_touchdown(400.0))];
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

/// Every check passes. The only imperfect part is the 300 fpm sink, a third
/// of the way from the transport ideal (250) to half marks (400).
#[test]
fn a_clean_attempt_counts_and_scores() {
    let a = attempt(&[], |_, p| p);
    assert!(a.valid, "{:?}", a.checks);
    let names: Vec<_> = a.checks.iter().map(|c| c.name).collect();
    assert_eq!(
        names,
        [
            "aircraft",
            "runway",
            "gate",
            "sim_rate",
            "slew",
            "pause",
            "continuity"
        ]
    );
    let score = a.score.unwrap();
    let sink = score.parts.iter().find(|p| p.name == "sink_rate").unwrap();
    assert!((sink.score - 0.83).abs() < 0.05, "{sink:?}");
    for p in score.parts.iter().filter(|p| p.name != "sink_rate") {
        assert_eq!(p.score, 1.0, "{p:?}");
    }
    assert!((score.total - (75.0 + 25.0 * sink.score)).abs() < 0.1);
    assert!(a.gate_s.unwrap() < a.touchdown_s.unwrap());
}

/// 17 kt from 290 is square across runway 20: outside a 10 kt crosswind
/// limit and a wind band of 200 ± 20°. The attempt does not count but is
/// still scored, so the pilot sees how it went.
#[test]
fn conditions_outside_their_bands_void_the_attempt() {
    let conditions = r#""conditions": {
    "wind": { "from_deg": 200, "speed_kt": 15, "tolerance_deg": 20, "tolerance_kt": 5 },
    "crosswind_kt": { "max": 10 }
  },
  "scoring""#;
    let a = attempt(&[(r#""scoring""#, conditions)], |_, p| Pt {
        wind: (290.0, 17.0),
        ..p
    });
    assert!(!a.valid);
    assert_eq!(failed(&a), ["wind", "crosswind"]);
    assert!(a.score.is_some());
    let calm = attempt(&[(r#""scoring""#, conditions)], |_, p| Pt {
        wind: (200.0, 15.0),
        ..p
    });
    assert!(calm.valid, "{:?}", calm.checks);
}

/// The autopilot flown down to 600 ft breaks "off by 1000 ft"; a 2x sim
/// rate for 5 s inside the attempt breaks the integrity check.
#[test]
fn rules_and_integrity_are_checked_after_the_gate() {
    let ap = r#""rules": { "ap_off_by_height_ft": 1000, "stop_on_runway": true }, "scoring""#;
    let a = attempt(&[(r#""scoring""#, ap)], |_, p| Pt {
        ap: p.alt > 600.0,
        ..p
    });
    assert_eq!(failed(&a), ["autopilot"]);
    let off = attempt(&[(r#""scoring""#, ap)], |_, p| Pt {
        ap: p.alt > 1100.0,
        ..p
    });
    assert!(off.valid, "{:?}", off.checks);
    let fast = attempt(&[], |t, p| Pt {
        sim_rate: if (100.0..105.0).contains(&t) {
            2.0
        } else {
            1.0
        },
        ..p
    });
    assert_eq!(failed(&fast), ["sim_rate"]);
}

/// A gate beyond where the flight started, or a landing on the other
/// runway: nothing to grade.
#[test]
fn no_attempt_without_the_gate_or_the_runway() {
    let gate = [
        (r#""distance_nm": 3"#, r#""distance_nm": 5.5"#),
        (r#""distance_nm": 4"#, r#""distance_nm": 7"#),
    ];
    let far = attempt(&gate, |_, p| p);
    assert_eq!(failed(&far), ["gate"]);
    assert!(far.score.is_none() && !far.valid);
    let other = attempt(&[(r#""runway": "20""#, r#""runway": "02""#)], |_, p| p);
    assert_eq!(failed(&other), ["runway"]);
    assert!(other.score.is_none());
}

/// A ground-speed dropout does not hide a jump: 1.7 km in two frames while
/// GS is missing (longer than `Flight::at` bridges) is beyond any speed.
#[test]
fn a_jump_during_a_ground_speed_dropout_breaks_continuity() {
    let a = attempt(&[], |t, p| Pt {
        speed: if (60.0..60.017).contains(&t) {
            100_000.0
        } else {
            p.speed
        },
        lost: if (57.0..62.0).contains(&t) {
            &["GROUND VELOCITY"]
        } else {
            &[]
        },
        ..p
    });
    assert_eq!(failed(&a), ["continuity"]);
}

/// 4 s without a position on the rollout is not leaving the runway:
/// only a located point off it fails the stop and the rollout score.
#[test]
fn a_position_dropout_on_the_rollout_stays_on_the_runway() {
    let stop = r#""rules": { "stop_on_runway": true }, "scoring""#;
    let a = attempt(&[(r#""scoring""#, stop)], |t, p| Pt {
        lost: match (TD_S + 8.0..TD_S + 12.0).contains(&t) {
            true => &["PLANE LATITUDE", "PLANE LONGITUDE"],
            false => &[],
        },
        ..p
    });
    assert!(a.valid, "{:?}", a.checks);
    let parts = a.score.unwrap().parts;
    let rollout = parts.iter().find(|p| p.name == "rollout").unwrap();
    assert_eq!(rollout.score, 1.0, "{rollout:?}");
}
