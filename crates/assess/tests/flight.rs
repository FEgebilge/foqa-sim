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
    let mut events = vec![identity("TEST")];
    if airport {
        events.push(SimEvent::Airport(runway_behind_touchdown(400.0)));
    }
    let (flight, track): (Flight, Track) = fly_to(&events, AIRLINER_LB, TD_S + ROLLOUT_S, |t| {
        landing(t, TD_S, 1.2)
    });
    let rules = load(CORE).unwrap();
    let selection = profiles.select(&flight);
    let found = evaluate(&rules, &selection, &flight, &track);
    let tds = touchdowns(&flight, &track);
    Method::landing_2().unwrap().assess(&Landing {
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
    // Threshold crossed at 42 ft: 8 ft under 50, within 10 ft.
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
    assert_eq!(score.partial, ["No Vref for TEST"]);
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
