//! Recorded synthetic landings (`testkit`) assessed end to end: measures
//! from the analyzer, findings from the core rules, Vref from the profile
//! or the selected speed.

use analyzer::{Flight, Track, touchdowns};
use assess::*;
use recorder::SimEvent;
use rules::{ProfileSet, evaluate, load};
use testkit::*;

const CORE: &str = include_str!("../../../rules/core.yaml");

/// A jet whose Vref is 135 kt with the flaps up: the 140 kt approach is
/// flown at Vapp (Vref + the 5 kt minimum additive in calm air).
const PROFILE: &str = r#"
type: TEST
name: Synthetic test jet
class: transport
vref:
  source: synthetic
  flaps:
    - { flap: 0 deg, weights_kg: [50000, 70000], ias: [135 kt, 135 kt] }
"#;

/// The stable airliner landing, its threshold 400 m back on a 3000 m
/// runway (`airport`), assessed as `TEST` with `profiles`.
fn assess(profiles: &ProfileSet, airport: bool, ranked: bool) -> Assessment {
    flown(profiles, airport, ranked, TD_S + ROLLOUT_S, |t| {
        landing(t, TD_S, 1.2)
    })
}

/// `script` flown for `duration` s onto that runway and assessed.
fn flown(
    profiles: &ProfileSet,
    airport: bool,
    ranked: bool,
    duration: f64,
    script: impl Fn(f64) -> Pt,
) -> Assessment {
    let mut events = vec![identity("TEST")];
    if airport {
        events.push(SimEvent::Airport(runway_behind_touchdown(400.0)));
    }
    let (flight, track): (Flight, Track) = fly_to(&events, AIRLINER_LB, duration, script);
    let rules = load(CORE).unwrap();
    let selection = profiles.select(&flight);
    let found = evaluate(&rules, &selection, &flight, &track);
    let tds = touchdowns(&flight, &track);
    Method::landing_3().unwrap().assess(&Landing {
        flight: &flight,
        track: &track,
        selection: &selection,
        touchdowns: &tds,
        index: 0,
        events: &found,
        rules: &rules,
        ranked,
        gate_ft: None,
        target_m: None,
    })
}

/// The TEST jet's ranked score of `script` flown for `duration` s.
fn scored(duration: f64, script: impl Fn(f64) -> Pt) -> (LandingScore, bool) {
    let profiles = ProfileSet::load([("TEST", PROFILE)]).unwrap();
    match flown(&profiles, true, true, duration, script) {
        Assessment::Scored {
            score, full_stop, ..
        } => (score, full_stop),
        a => panic!("not scored: {a:?}"),
    }
}

/// The graded criteria `id` (one per half of the window for a series).
fn criteria<'a>(s: &'a LandingScore, id: &str) -> Vec<&'a CriterionScore> {
    s.domains
        .iter()
        .flat_map(|d| &d.criteria)
        .filter(|c| c.id == id)
        .collect()
}

#[test]
fn a_stable_landing_on_its_aiming_point_is_good() {
    let profiles = ProfileSet::load([("TEST", PROFILE)]).unwrap();
    let Assessment::Scored {
        score,
        speeds,
        full_stop,
    } = assess(&profiles, true, true)
    else {
        panic!("not scored")
    };
    assert!(full_stop);
    let speeds = speeds.unwrap();
    assert_eq!(
        (speeds.vref_kt, speeds.vapp_kt, speeds.source),
        (135.0, 140.0, SpeedSource::Profile)
    );
    assert_eq!(score.confidence, Confidence::Full, "{:?}", score.partial);
    assert!(score.caps.is_empty(), "{:?}", score.caps);
    assert_eq!(score.grade, Grade::Good, "{score:#?}");
    // On speed, on the aiming point, on the centreline.
    let ell = |id: &str| {
        let c = score
            .domains
            .iter()
            .flat_map(|d| &d.criteria)
            .find(|c| c.id == id);
        c.unwrap_or_else(|| panic!("no {id}")).ell
    };
    assert_eq!([ell("A1"), ell("C1"), ell("D1")], [0.0, 0.0, 0.0]);
    // Threshold crossed at 42 ft: 7 ft over the 35 ft wheel height of a
    // 50 ft glidepath (jet eye to wheel 15 ft), within 15 ft.
    assert!(ell("B1") > 0.0 && ell("B1") <= 0.5);
    assert_eq!(score.stable_from_ft.map(f64::round), Some(1000.0));
}

/// Decision 1: without a profile Vref the selected speed (140 kt) serves
/// the debrief, labelled; a ranked attempt gets none and is partial.
#[test]
fn the_selected_speed_is_only_a_debrief_fallback() {
    let none = ProfileSet::load([]).unwrap();
    let Assessment::Scored { speeds, .. } = assess(&none, true, false) else {
        panic!("not scored")
    };
    let s = speeds.unwrap();
    assert_eq!((s.vapp_kt, s.source), (140.0, SpeedSource::Selected));
    let Assessment::Scored { score, speeds, .. } = assess(&none, true, true) else {
        panic!("not scored")
    };
    assert_eq!(speeds, None);
    assert_eq!(score.confidence, Confidence::Partial);
    assert_eq!(
        score.partial,
        ["No reference speed (Vref) for TEST, so speed items were not graded"]
    );
}

#[test]
fn a_landing_without_a_runway_is_not_scored() {
    let none = ProfileSet::load([]).unwrap();
    assert_eq!(
        assess(&none, false, false),
        Assessment::Insufficient {
            missing: vec!["runway"]
        }
    );
}

/// A touch-and-go whose recording ends in the climb-out, turned 60 deg off
/// the runway: the rollout ends at the liftoff, so it is no full stop, no
/// excursion and no D4.
#[test]
fn a_touch_and_go_ending_airborne_is_no_full_stop() {
    const LIFTOFF_S: f64 = TD_S + 14.0;
    let (s, full_stop) = scored(LIFTOFF_S + 40.0, |t| {
        let p = landing(t, TD_S, 1.2);
        if t < TD_S {
            return p;
        }
        Pt {
            alt: lerp(&[(LIFTOFF_S, 0.0), (LIFTOFF_S + 40.0, 800.0)], t),
            speed: lerp(&[(TD_S, 140.0), (TD_S + 6.0, 122.0), (LIFTOFF_S, 140.0)], t),
            track: if t > LIFTOFF_S + 5.0 { 260.0 } else { 200.0 },
            heading: if t > LIFTOFF_S + 5.0 { 260.0 } else { 200.0 },
            ground: t < LIFTOFF_S,
            ..p
        }
    });
    assert!(!full_stop);
    assert!(s.caps.is_empty(), "{:?}", s.caps);
    assert!(criteria(&s, "D4").is_empty() && criteria(&s, "F2").is_empty());
}

/// Data4Safety late gear: lowered between 1000 ft and the 500 ft split is
/// L2, below it L3, still up at the bottom of the window L3.
#[test]
fn late_gear_is_graded_by_where_it_moved() {
    // 700 ft at 60 s before contact, 325 ft at 30 s.
    let a6 = |gear: &'static [(f64, f64)]| {
        let (s, _) = scored(TD_S + ROLLOUT_S, |t| Pt {
            gear: lerp(gear, t),
            ..landing(t, TD_S, 1.2)
        });
        criteria(&s, "A6")[0].ell
    };
    assert_eq!(a6(&[(0.0, 100.0)]), 0.0);
    assert_eq!(a6(&[(TD_S - 60.0, 0.0), (TD_S - 55.0, 100.0)]), 2.0);
    assert_eq!(a6(&[(TD_S - 30.0, 0.0), (TD_S - 25.0, 100.0)]), 3.0);
    assert_eq!(a6(&[(0.0, 0.0)]), 3.0);
}

/// A2's persistence (S1): a 4 s dive above 500 ft holds over 1500 fpm
/// for 3 s at 2200 fpm, L2 by the second threshold (held 5 s it is only
/// 1390 fpm, L1). At 1800 fpm it stays under 1500 fpm and is graded by its
/// 5 s value, about 1214 fpm (the inertial rate smooths the dive).
#[test]
fn the_descent_rate_is_held_3_s_only_past_the_second_threshold() {
    // 875 ft at 50 s, then `fpm` for 4 s, back on the path at 100 ft.
    let a2 = |fpm: f64| {
        let dive = [
            (0.0, 1500.0),
            (50.0, 875.0),
            (54.0, 875.0 - fpm / 15.0),
            (TD_S - 12.0, 100.0),
            (TD_S - 2.0, 10.0),
            (TD_S, 0.0),
        ];
        let (s, _) = scored(TD_S + ROLLOUT_S, |t| Pt {
            alt: lerp(&dive, t),
            ..landing(t, TD_S, 1.2)
        });
        let above = criteria(&s, "A2")
            .into_iter()
            .find(|c| c.height == Height::Above)
            .cloned();
        above.unwrap()
    };
    let steep = a2(2200.0);
    assert!(steep.level == 2 && steep.value >= 1500.0, "{steep:?}");
    let firm = a2(1800.0);
    assert!(firm.level == 1 && firm.value < 1250.0, "{firm:?}");
}

/// A 3 s IAS dropout in the window and no radio height at the threshold:
/// A1 and B1 are n/a, not graded, and the score is partial.
#[test]
fn data_gaps_make_their_criteria_n_a() {
    let (s, _) = scored(TD_S + ROLLOUT_S, |t| {
        let lost: &'static [&'static str] = if (60.0..63.0).contains(&t) {
            &["AIRSPEED INDICATED"]
        } else if (TD_S - 8.0..TD_S - 3.0).contains(&t) {
            &["RADIO HEIGHT"]
        } else {
            &[]
        };
        Pt {
            lost,
            ..landing(t, TD_S, 1.2)
        }
    });
    assert!(criteria(&s, "A1").is_empty() && criteria(&s, "B1").is_empty());
    assert!(!criteria(&s, "A2").is_empty());
    assert_eq!(s.confidence, Confidence::Partial);
    assert_eq!(
        s.partial,
        [
            "IAS off the approach speed was not graded: the recording has a gap of over 1.5 s in the approach",
            "Threshold crossing height was not graded: no radio altitude at the threshold"
        ]
    );
}

/// A light aircraft aims at 150 m, not the 400 m aiming point of this
/// 3000 m runway, and crosses the threshold against its own wheel path.
#[test]
fn a_light_aircraft_aims_short_of_the_airline_aiming_point() {
    let light = "type: TEST\nname: Synthetic light aircraft\nclass: light\n";
    let profiles = ProfileSet::load([("TEST", light)]).unwrap();
    let Assessment::Scored { score, .. } = assess(&profiles, true, false) else {
        panic!("not scored")
    };
    let value = |id: &str| criteria(&score, id)[0].value;
    // Down 400 m past the threshold: 250 m = 820 ft past the target.
    assert!(
        (value("C1") - 250.0 / 0.3048).abs() < 2.0,
        "{}",
        value("C1")
    );
    // 41.9 ft against 150 m x tan 3 deg = 25.8 ft less 6 ft eye to wheel.
    assert!((value("B1") - (41.9 - 19.8)).abs() < 1.0, "{}", value("B1"));
}

/// A crash the sim reports during the landing caps it as an excursion.
#[test]
fn a_crash_on_the_landing_caps_the_score() {
    let events = [
        (0.0, identity("TEST")),
        (0.0, SimEvent::Airport(runway_behind_touchdown(400.0))),
        (TD_S + 1.0, SimEvent::Crashed),
    ];
    let journal = journal(&events, AIRLINER_LB, TD_S + ROLLOUT_S, |t| {
        landing(t, TD_S, 1.2)
    });
    let flight = Flight::read(&journal[..]).unwrap();
    let track = Track::new(&flight).unwrap();
    let profiles = ProfileSet::load([("TEST", PROFILE)]).unwrap();
    let (rules, selection) = (load(CORE).unwrap(), profiles.select(&flight));
    let tds = touchdowns(&flight, &track);
    let Assessment::Scored { score, .. } = Method::landing_3().unwrap().assess(&Landing {
        flight: &flight,
        track: &track,
        selection: &selection,
        touchdowns: &tds,
        index: 0,
        events: &evaluate(&rules, &selection, &flight, &track),
        rules: &rules,
        ranked: false,
        gate_ft: None,
        target_m: None,
    }) else {
        panic!("not scored")
    };
    assert_eq!(score.score, 0.0, "{:?}", score.caps);
}
