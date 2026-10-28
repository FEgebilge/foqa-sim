//! Synthetic golden flights (`testkit`) through the core rule set, to check
//! the phase, touchdown and rule logic against flights whose answer is known
//! by construction.

use analyzer::{Flight, Phase, Track, phases, touchdowns};
use fdm_core::Airport;
use fdm_core::geo::LocalFrame;
use recorder::SimEvent;
use rules::{Event, Level, ProfileSet, evaluate, load};
use testkit::*;

const CORE: &str = include_str!("../../../rules/core.yaml");
/// Every event the rules in `yaml` raise, in full, with the shipped
/// profiles.
fn all_events(yaml: &str, flight: &Flight, track: &Track) -> Vec<Event> {
    graded(&ProfileSet::shipped().unwrap(), yaml, flight, track)
}

/// `all_events` with the given profiles.
fn graded(profiles: &ProfileSet, yaml: &str, flight: &Flight, track: &Track) -> Vec<Event> {
    evaluate(
        &load(yaml).unwrap(),
        &profiles.select(flight),
        flight,
        track,
    )
}

/// Just the (rule, level) pairs.
fn pairs(events: Vec<Event>) -> Vec<(String, Level)> {
    events.into_iter().map(|e| (e.rule, e.level)).collect()
}

fn events(yaml: &str, flight: &Flight, track: &Track) -> Vec<(String, Level)> {
    pairs(all_events(yaml, flight, track))
}

fn found(rules: &[(&str, Level)]) -> Vec<(String, Level)> {
    rules.iter().map(|&(r, l)| (r.to_owned(), l)).collect()
}

/// The phases, with the boundaries the flight is graded with.
fn sequence(flight: &Flight, track: &Track) -> Vec<Phase> {
    let profiles = ProfileSet::shipped().unwrap();
    let cfg = profiles.select(flight).phases;
    phases(track, &cfg).iter().map(|s| s.phase).collect()
}

/// A stabilised airliner approach and a smooth landing (1.2 g, 300 fpm):
/// the stable flight a FOQA programme must stay silent on.
#[test]
fn stable_airliner_approach_raises_nothing() {
    let (flight, track) = fly(AIRLINER_LB, TD_S + ROLLOUT_S, |t| landing(t, TD_S, 1.2));
    assert_eq!(
        sequence(&flight, &track),
        [Phase::EnRoute, Phase::Approach, Phase::Landing, Phase::Taxi]
    );
    assert_eq!(events(CORE, &flight, &track), []);
    let [td] = &touchdowns(&flight, &track)[..] else {
        panic!("expected one touchdown")
    };
    assert!(!td.bounce);
    assert!((td.sink_fpm - 300.0).abs() < 30.0, "{:.0} fpm", td.sink_fpm);
}

/// The stable approach with the gear left up: not down at 500 ft (High).
#[test]
fn gear_up_at_500_ft_is_raised() {
    let (flight, track) = fly(AIRLINER_LB, TD_S + ROLLOUT_S, |t| Pt {
        gear: 0.0,
        ..landing(t, TD_S, 1.2)
    });
    assert!(events(CORE, &flight, &track).contains(&("gear-not-down-500".into(), Level::High)));
}

/// A light retractable (BE36, PC12 …) with the gear up at 500 ft is
/// raised as for a transport; fixed gear reads down and stays silent (the
/// light landings below).
#[test]
fn light_gear_up_at_500_ft_is_raised() {
    let (flight, track) = fly(LIGHT_LB, TD_S + ROLLOUT_S, |t| Pt {
        gear: 0.0,
        ..landing(t, TD_S, 1.2)
    });
    assert!(events(CORE, &flight, &track).contains(&("gear-not-down-500".into(), Level::High)));
}

/// The stall warning for 3 s in the flare, under 50 ft: a stick shaker in
/// a transport (High), a light aircraft's horn by design (nothing).
#[test]
fn a_stall_warning_in_the_flare_counts_for_a_transport_only() {
    let script = |t: f64| Pt {
        set: if (TD_S - 4.0..TD_S - 1.0).contains(&t) {
            vec![("STALL WARNING", 1.0)]
        } else {
            vec![]
        },
        ..landing(t, TD_S, 1.2)
    };
    let (flight, track) = fly(AIRLINER_LB, TD_S + ROLLOUT_S, script);
    assert_eq!(
        events(CORE, &flight, &track),
        found(&[("stall-warning", Level::High)])
    );
    let (flight, track) = fly(LIGHT_LB, TD_S + ROLLOUT_S, script);
    assert_eq!(events(CORE, &flight, &track), []);
}

/// The same landing with a 2.0 g impact: above the transport hard-landing
/// medium level (1.8 g) but below high (2.1 g).
#[test]
fn airliner_hard_landing_is_medium() {
    let (flight, track) = fly(AIRLINER_LB, TD_S + ROLLOUT_S, |t| landing(t, TD_S, 2.0));
    assert_eq!(
        events(CORE, &flight, &track),
        found(&[("hard-landing", Level::Medium)])
    );
}

/// The jet airliner levels do not grade a transport turboprop: the same
/// 2.0 g landing of a type whose profile scores it as one raises nothing.
#[test]
fn turboprop_is_not_graded_by_jet_hard_landing_levels() {
    let profile = format!("{LIMITS_PROFILE}scoring_class: turboprop\n")
        .replace("vmo: { value: 132 kt", "vmo: { value: 250 kt")
        .replace("vlo: { value: 125 kt", "vlo: { value: 200 kt");
    let (flight, track) = fly_to(&[identity("TEST")], AIRLINER_LB, TD_S + ROLLOUT_S, |t| {
        landing(t, TD_S, 2.0)
    });
    let profiles = ProfileSet::load([("TEST", profile.as_str())]).unwrap();
    assert_eq!(pairs(graded(&profiles, CORE, &flight, &track)), []);
}

/// 2.0 g is a hard landing for a 2300 lb light aircraft only at its low level
/// (1.8 g; medium starts at 2.2 g), and the transport rule is not applied to
/// it; 1.7 g is within limits.
#[test]
fn light_aircraft_uses_its_own_hard_landing_levels() {
    let (flight, track) = fly(LIGHT_LB, TD_S + ROLLOUT_S, |t| landing(t, TD_S, 2.0));
    assert_eq!(
        events(CORE, &flight, &track),
        found(&[("hard-landing-light", Level::Low)])
    );
    let (flight, track) = fly(LIGHT_LB, TD_S + ROLLOUT_S, |t| landing(t, TD_S, 1.7));
    assert_eq!(events(CORE, &flight, &track), []);
}

/// Touchdown, a 1.5 s hop to 2 ft, touchdown again. Only the second contact
/// is the bounce, and the landing is graded Medium once.
#[test]
fn bounce_flags_only_the_second_touchdown() {
    const HOP_S: f64 = 1.5;
    let (flight, track) = fly(AIRLINER_LB, TD_S + ROLLOUT_S, |t| {
        let mut p = landing(t, TD_S, 1.3);
        let hop = (t - TD_S - 0.3) / HOP_S;
        if (0.0..1.0).contains(&hop) {
            p.alt = 8.0 * hop * (1.0 - hop);
            p.ground = false;
        } else if (1.0..1.13).contains(&hop) {
            p.g = 1.4;
        }
        p
    });
    let tds = touchdowns(&flight, &track);
    assert_eq!(
        tds.iter().map(|t| t.bounce).collect::<Vec<_>>(),
        [false, true]
    );
    let events = all_events(CORE, &flight, &track);
    let [bounce] = &events[..] else {
        panic!("expected one event, got {events:?}")
    };
    assert_eq!(
        (bounce.rule.as_str(), bounce.level),
        ("bounced-landing", Level::Medium)
    );
    assert_eq!(bounce.start, tds[1].t);
}

/// Approach down to 300 ft AAL, go-around back above 1000 ft, second
/// approach to a landing. The go-around is recorded as an advisory, and the
/// 500 ft gate fires on each approach that reaches it.
#[test]
fn go_around_is_recorded_and_rearms_the_gates() {
    const GATE: &str = "
rule: gate-500
version: 1
domain: other
title: Passed 500 ft on approach
phase: Approach
gate: height <= 500 ft
measure: true
level: advisory
";
    const SECOND_TD_S: f64 = 175.0 + APPROACH_S; // second approach starts where the climb ends
    let (flight, track) = fly(AIRLINER_LB, SECOND_TD_S + ROLLOUT_S, |t| {
        if t < SECOND_TD_S - APPROACH_S {
            let alt = lerp(&[(0.0, 1500.0), (103.0, 300.0), (175.0, 1600.0)], t);
            Pt {
                alt,
                speed: 140.0,
                ..landing(t, SECOND_TD_S, 1.0)
            }
        } else {
            landing(t, SECOND_TD_S, 1.2)
        }
    });
    assert_eq!(
        sequence(&flight, &track),
        [
            Phase::EnRoute,
            Phase::Approach,
            Phase::GoAround,
            Phase::EnRoute,
            Phase::Approach,
            Phase::Landing,
            Phase::Taxi
        ]
    );
    assert_eq!(
        events(CORE, &flight, &track),
        found(&[("go-around", Level::Advisory)])
    );
    assert_eq!(
        events(GATE, &flight, &track),
        found(&[("gate-500", Level::Advisory); 2])
    );
}

/// A continuous rule with `sustain: 2 s` ignores a 1 s excursion above 1.2 g
/// early in the flight and reports a 3 s one.
#[test]
fn sustain_filters_short_exceedances() {
    const RULE: &str = "
rule: g-sustained
version: 1
domain: other
title: Normal load above 1.2 g
phase: EnRoute
measure: accel.normal
sustain: 2 s
above: { low: 1.2 g }
";
    let with_bump = |secs: f64| {
        let (flight, track) = fly(AIRLINER_LB, TD_S + ROLLOUT_S, |t| Pt {
            g: if (5.0..5.0 + secs).contains(&t) {
                1.5
            } else {
                1.0
            },
            ..landing(t, TD_S, 1.0)
        });
        all_events(RULE, &flight, &track)
    };
    assert_eq!(with_bump(1.0), []);
    let [event] = &with_bump(3.0)[..] else {
        panic!("expected one event")
    };
    assert_eq!(event.level, Level::Low);
    assert_eq!(event.value, Some(1.5));
}

fn land_on(airports: &[Airport]) -> (Flight, Track) {
    let events: Vec<_> = airports.iter().cloned().map(SimEvent::Airport).collect();
    fly_to(&events, AIRLINER_LB, TD_S + ROLLOUT_S, |t| {
        landing(t, TD_S, 1.2)
    })
}

/// 1500 m past the threshold is beyond the 914 m touchdown zone: a medium
/// long landing, nothing else, and the touchdown carries the runway fix.
#[test]
fn touchdown_past_the_zone_is_a_long_landing() {
    let (flight, track) = land_on(&[runway_behind_touchdown(1500.0)]);
    assert_eq!(
        events(CORE, &flight, &track),
        found(&[("long-landing", Level::Medium)])
    );
    let fix = touchdowns(&flight, &track)[0].runway.clone().unwrap();
    assert_eq!((fix.icao.as_str(), fix.runway.as_str()), ("TEST", "20"));
    assert!((fix.along_m - 1500.0).abs() < 2.0 && fix.cross_m.abs() < 1.0);
}

#[test]
fn touchdown_inside_the_zone_raises_nothing() {
    let (flight, track) = land_on(&[runway_behind_touchdown(400.0)]);
    assert_eq!(events(CORE, &flight, &track), []);
    assert!(
        touchdowns(&flight, &track)[0]
            .runway
            .as_ref()
            .unwrap()
            .in_tdz
    );
}

/// Runway moved 15 m to the left of the touchdown: contact ~49 ft right of
/// the centreline, inside the touchdown zone, a medium off-centreline event.
#[test]
fn touchdown_right_of_the_centreline_is_flagged() {
    let mut airport = runway_behind_touchdown(400.0);
    let rw = &mut airport.runways[0];
    let (sin, cos) = TRACK_DEG.to_radians().sin_cos();
    (rw.lat_deg, rw.lon_deg) =
        LocalFrame::new(rw.lat_deg, rw.lon_deg).point(-15.0 * cos, 15.0 * sin);
    let (flight, track) = land_on(&[airport]);
    assert_eq!(
        events(CORE, &flight, &track),
        found(&[("off-centreline-touchdown", Level::Medium)])
    );
    let cross = touchdowns(&flight, &track)[0]
        .runway
        .as_ref()
        .unwrap()
        .cross_m;
    assert!((cross - 15.0).abs() < 1.0, "{cross:.1} m");
}

#[test]
fn touchdown_before_the_threshold_is_a_short_landing() {
    let (flight, track) = land_on(&[runway_behind_touchdown(-80.0)]);
    assert_eq!(
        events(CORE, &flight, &track),
        found(&[("short-landing", Level::High)])
    );
}

/// Without an airport snapshot the runway values are unknown: no runway
/// event fires and the rest of the flight is graded as before.
#[test]
fn no_airport_data_leaves_runway_values_unknown() {
    let (flight, track) = land_on(&[]);
    assert_eq!(touchdowns(&flight, &track)[0].runway, None);
    assert_eq!(events(CORE, &flight, &track), []);
    let probe = "
rule: probe
version: 1
domain: other
title: Distance known
at: touchdown
measure: td.distance > -1000 ft
level: advisory
";
    assert_eq!(events(probe, &flight, &track), []);
    let (flight, track) = land_on(&[runway_behind_touchdown(400.0)]);
    assert_eq!(
        events(probe, &flight, &track),
        found(&[("probe", Level::Advisory)])
    );
}

/// A light aircraft on approach at 80 kt with the flaps at 20 deg (C172: VFE
/// 85 kt above 10 deg), at 95 kt for 3 s. As a C172 that is 10 kt over VFE
/// for 3 s: Medium (over 5 kt; High needs more than 10 kt). Without an
/// identity no type profile applies and nothing fires.
#[test]
fn flap_overspeed_uses_the_vfe_of_the_type_profile() {
    let script = |t: f64| {
        let p = landing(t, TD_S, 1.2);
        Pt {
            speed: if (10.0..13.0).contains(&t) {
                95.0
            } else {
                p.speed.min(80.0)
            },
            flaps: 20.0,
            ..p
        }
    };
    let (flight, track) = fly_to(&[identity("C172")], LIGHT_LB, TD_S + ROLLOUT_S, script);
    let found = all_events(CORE, &flight, &track);
    let [e] = &found[..] else {
        panic!("expected one event, got {found:?}")
    };
    assert_eq!(
        (e.rule.as_str(), e.level, e.value, e.threshold),
        ("flap-overspeed", Level::Medium, Some(10.0), Some(5.0))
    );
    assert_eq!(e.detail.as_deref(), Some("95 kt at flaps 20°, limit 85 kt"));
    assert!(e.end.us_since(e.start) >= 2_000_000);
    let (flight, track) = fly(LIGHT_LB, TD_S + ROLLOUT_S, script);
    assert_eq!(events(CORE, &flight, &track), []);
}

/// A test type whose tail touches at 6 deg on rotation and 3.5 deg at
/// touchdown, inspected from 1.1 g.
const TEST_PROFILE: &str = r#"
type: TEST
name: Synthetic test aircraft
class: light
tail_contact:
  takeoff: { value: 6 deg, source: synthetic }
  landing: { value: 3.5 deg, source: synthetic }
hard_landing_g: { value: 1.1 g, source: synthetic }
"#;

/// Takeoff roll to 60 kt, rotation to 4.5 deg, liftoff at 20 s, climb to
/// 1500 ft, then the 3 deg nose-up 1.2 g landing. Rotation leaves ~1.5 deg
/// of tail clearance (Medium), touchdown 0.5 deg (High), and 1.2 g passes
/// the 1.1 g inspection load. Without the type's profile the same flight is
/// graded by its class and raises nothing.
#[test]
fn tail_contact_and_inspection_values_drive_their_rules() {
    const LIFTOFF_S: f64 = 20.0;
    const CLIMB_END_S: f64 = 100.0;
    const TD: f64 = CLIMB_END_S + APPROACH_S;
    let (flight, track) = fly_to(&[identity("TEST")], LIGHT_LB, TD + ROLLOUT_S, |t| {
        if t < CLIMB_END_S {
            Pt {
                alt: lerp(&[(LIFTOFF_S, 0.0), (CLIMB_END_S, 1500.0)], t),
                speed: lerp(&[(0.0, 0.0), (LIFTOFF_S, 60.0)], t),
                g: 1.0,
                pitch: lerp(&[(LIFTOFF_S - 3.0, 0.0), (LIFTOFF_S, 4.5)], t),
                flaps: 0.0,
                gear: 100.0,
                wind: (0.0, 0.0),
                heading: TRACK_DEG,
                track: TRACK_DEG,
                sim_rate: 1.0,
                ap: false,
                ground: t < LIFTOFF_S,
                lost: &[],
                set: Vec::new(),
            }
        } else {
            landing(t, TD, 1.2)
        }
    });
    let profiles = ProfileSet::load([("TEST", TEST_PROFILE)]).unwrap();
    assert_eq!(profiles.select(&flight).to_string(), "TEST profile");
    let events = graded(&profiles, CORE, &flight, &track);
    assert_eq!(
        pairs(events.clone()),
        found(&[
            ("tailstrike-risk-takeoff", Level::Medium),
            ("tailstrike-risk-landing", Level::High),
            ("hard-landing-inspection", Level::High),
        ])
    );
    let margin = |rule: &str| {
        events
            .iter()
            .find(|e| e.rule == rule)
            .unwrap()
            .value
            .unwrap()
    };
    assert!((margin("tailstrike-risk-takeoff") - 1.5).abs() < 0.2);
    assert!((margin("tailstrike-risk-landing") - 0.5).abs() < 0.05);

    let shipped = ProfileSet::shipped().unwrap();
    assert_eq!(
        shipped.select(&flight).to_string(),
        "light class default (no TEST profile)"
    );
    assert_eq!(graded(&shipped, CORE, &flight, &track), []);
}

/// A touch-and-go: the landing roll slows to 122 kt at 6 s with the nose
/// lowered, power goes on, rotation to 4.5 deg and liftoff at 14 s. The
/// landing ends at the lowest speed and the second takeoff roll starts
/// there, so its rotation is checked (1.5 deg of tail clearance, Medium)
/// as well as the 3 deg touchdown (0.5 deg, High).
#[test]
fn a_touch_and_go_splits_the_roll_and_checks_the_second_rotation() {
    const LIFTOFF_S: f64 = TD_S + 14.0;
    let (flight, track) = fly_to(&[identity("TEST")], LIGHT_LB, LIFTOFF_S + 60.0, |t| {
        let p = landing(t, TD_S, 1.0);
        if t < TD_S {
            return p;
        }
        let pitch = [
            (TD_S, 3.0),
            (TD_S + 2.0, 0.0),
            (LIFTOFF_S - 3.0, 0.0),
            (LIFTOFF_S, 4.5),
        ];
        Pt {
            alt: lerp(&[(LIFTOFF_S, 0.0), (LIFTOFF_S + 60.0, 1500.0)], t),
            speed: lerp(&[(TD_S, 140.0), (TD_S + 6.0, 122.0), (LIFTOFF_S, 140.0)], t),
            pitch: lerp(&pitch, t),
            ground: t < LIFTOFF_S,
            ..p
        }
    });
    let profiles = ProfileSet::load([("TEST", TEST_PROFILE)]).unwrap();
    let spans = phases(&track, &profiles.select(&flight).phases);
    use Phase::*;
    let seq: Vec<_> = spans.iter().map(|s| s.phase).collect();
    assert_eq!(
        seq,
        [EnRoute, Approach, Landing, Takeoff, InitialClimb, EnRoute]
    );
    let go = &spans[3];
    assert_eq!(go.cause.trigger, analyzer::Trigger::TouchAndGo);
    assert!((go.start.as_secs_f64() - (TD_S + 6.0)).abs() < 0.2);
    assert!((go.cause.value - 122.0).abs() < 0.5);
    assert_eq!(
        pairs(graded(&profiles, CORE, &flight, &track)),
        found(&[
            ("tailstrike-risk-landing", Level::High),
            ("tailstrike-risk-takeoff", Level::Medium),
        ])
    );
}

const LIFTOFF_S: f64 = 30.0;

/// A takeoff roll from 0 to 150 kt at `LIFTOFF_S`, the pitch along
/// `pitch` (time, deg), flaps 5 down to `flaps_at_200` by 200 ft, a climb
/// at 1500 fpm to 1500 ft, then the stable landing.
fn takeoff(
    pitch: &[(f64, f64)],
    flaps_at_200: f64,
    set: &[(f64, f64, &'static str, f64)],
) -> Takeoff {
    Takeoff {
        pitch: pitch.to_vec(),
        flaps_at_200,
        set: set.to_vec(),
    }
}

/// A scripted takeoff; `set` holds SimVars over (from, to) s.
struct Takeoff {
    pitch: Vec<(f64, f64)>,
    flaps_at_200: f64,
    set: Vec<(f64, f64, &'static str, f64)>,
}

const CLIMB_TOP_S: f64 = LIFTOFF_S + 60.0;
const TAKEOFF_TD_S: f64 = CLIMB_TOP_S + APPROACH_S;

impl Takeoff {
    fn at(&self, t: f64) -> Pt {
        if t >= CLIMB_TOP_S {
            return landing(t, TAKEOFF_TD_S, 1.2);
        }
        let alt = lerp(&[(LIFTOFF_S, 0.0), (CLIMB_TOP_S, 1500.0)], t);
        Pt {
            alt,
            speed: lerp(&[(0.0, 0.0), (LIFTOFF_S, 150.0)], t),
            pitch: lerp(&self.pitch, t),
            flaps: lerp(
                &[
                    (0.0, 5.0),
                    (LIFTOFF_S + 4.0, 5.0),
                    (LIFTOFF_S + 8.0, self.flaps_at_200),
                ],
                t,
            ),
            ground: t < LIFTOFF_S,
            set: (self.set.iter())
                .filter(|s| (s.0..s.1).contains(&t))
                .map(|s| (s.2, s.3))
                .collect(),
            ..landing(TAKEOFF_TD_S - APPROACH_S, TAKEOFF_TD_S, 1.0)
        }
    }

    fn events(&self) -> Vec<(String, Level)> {
        let (flight, track) = fly(AIRLINER_LB, TAKEOFF_TD_S + ROLLOUT_S, |t| self.at(t));
        events(CORE, &flight, &track)
    }
}

/// Rotation at 6 deg/s is past the medium level (5 deg/s), at 1 deg/s
/// under the low one (1.5 deg/s); 3 deg/s, the Airbus rate, is quiet.
#[test]
fn the_rotation_rate_is_graded_at_liftoff() {
    let rotate = |rate: f64| {
        let start = LIFTOFF_S - 8.0 / rate;
        takeoff(
            &[(start, 0.0), (LIFTOFF_S, 8.0), (LIFTOFF_S + 10.0, 8.0)],
            5.0,
            &[],
        )
        .events()
    };
    assert_eq!(rotate(6.0), found(&[("rotation-rate", Level::Medium)]));
    assert_eq!(rotate(1.0), found(&[("rotation-rate", Level::Low)]));
    assert_eq!(rotate(3.0), []);
}

const NORMAL_ROTATION: [(f64, f64); 2] = [(LIFTOFF_S - 3.0, 0.0), (LIFTOFF_S, 9.0)];

/// Flaps 5 at liftoff, up by 200 ft: retracted below 400 ft (Medium).
#[test]
fn flaps_retracted_below_400_ft_after_takeoff_are_raised() {
    let early = takeoff(&NORMAL_ROTATION, 0.0, &[]).events();
    assert_eq!(early, found(&[("early-flap-retraction", Level::Medium)]));
    assert_eq!(takeoff(&NORMAL_ROTATION, 5.0, &[]).events(), []);
}

/// The stick shaker for 2 s from liftoff, under 35 ft: a takeoff-phase
/// stall warning (High); 18 deg of bank for 4 s from liftoff: over 15 deg
/// below 500 ft (Medium).
#[test]
fn stall_warning_and_bank_at_liftoff_are_raised() {
    let shaker = [(LIFTOFF_S, LIFTOFF_S + 2.0, "STALL WARNING", 1.0)];
    assert_eq!(
        takeoff(&NORMAL_ROTATION, 5.0, &shaker).events(),
        found(&[("stall-warning", Level::High)])
    );
    let bank = [(LIFTOFF_S, LIFTOFF_S + 4.0, "PLANE BANK DEGREES", -18.0)];
    assert_eq!(
        takeoff(&NORMAL_ROTATION, 5.0, &bank).events(),
        found(&[("bank-below-500", Level::Medium)])
    );
}

/// Up to 90 kt, braked to a stop without lifting off: a rejected takeoff
/// past the 80 kt high-speed split (Medium), at its highest speed.
#[test]
fn a_rejected_takeoff_is_graded_by_its_highest_speed() {
    let (flight, track) = fly(AIRLINER_LB, 60.0, |t| Pt {
        alt: 0.0,
        speed: lerp(
            &[
                (0.0, 0.0),
                (5.0, 10.0),
                (25.0, 90.0),
                (40.0, 10.0),
                (45.0, 0.0),
            ],
            t,
        ),
        ground: true,
        ..landing(TD_S, TD_S, 1.0)
    });
    assert_eq!(
        sequence(&flight, &track),
        [
            Phase::Standing,
            Phase::Taxi,
            Phase::RejectedTakeoff,
            Phase::Taxi,
            Phase::Standing
        ]
    );
    let [rto] = &all_events(CORE, &flight, &track)[..] else {
        panic!("expected one event")
    };
    assert_eq!(
        (rto.rule.as_str(), rto.level),
        ("rejected-takeoff", Level::Medium)
    );
    assert!((rto.value.unwrap() - 90.0).abs() < 1.0);
    assert_eq!(rto.detail.as_deref(), Some("rejected at 90 kt"));
}

/// A test type with speed and wind limits under what the scripted approach
/// flies: VMO 132 kt, gear 150 kt extended and 125 kt moving, 10 kt
/// demonstrated crosswind, 5 kt tailwind.
const LIMITS_PROFILE: &str = r#"
type: TEST
name: Synthetic test aircraft
class: transport
vmo: { value: 132 kt, source: synthetic }
vle: { value: 150 kt, source: synthetic }
vlo: { value: 125 kt, source: synthetic, approximate: true, note: "synthetic sister type" }
max_crosswind: { value: 10 kt, source: synthetic }
max_tailwind: { value: 5 kt, source: synthetic }
"#;

/// The 140 kt approach onto runway 20 with the gear moving for 3 s at
/// 30 s and the wind `wind`, graded with `LIMITS_PROFILE`.
fn limits_flight(wind: (f64, f64)) -> Vec<(String, Level)> {
    limits_flight_with(LIMITS_PROFILE, wind)
}

/// `limits_flight` graded with `profile`.
fn limits_flight_with(profile: &str, wind: (f64, f64)) -> Vec<(String, Level)> {
    let events = [
        identity("TEST"),
        SimEvent::Airport(runway_behind_touchdown(400.0)),
    ];
    let (flight, track) = fly_to(&events, AIRLINER_LB, TD_S + ROLLOUT_S, |t| Pt {
        gear: if (30.0..33.0).contains(&t) {
            50.0
        } else {
            100.0
        },
        wind,
        ..landing(t, TD_S, 1.2)
    });
    let profiles = ProfileSet::load([("TEST", profile)]).unwrap();
    pairs(graded(&profiles, CORE, &flight, &track))
}

/// With VLE lowered to 128 kt the 140 kt approach is 12 kt over it with
/// the gear down (High).
#[test]
fn gear_down_above_vle_is_an_overspeed() {
    let profile = LIMITS_PROFILE.replace("vle: { value: 150 kt", "vle: { value: 128 kt");
    let events = limits_flight_with(&profile, (0.0, 0.0));
    assert!(
        events.contains(&("gear-extended-overspeed".into(), Level::High)),
        "{events:?}"
    );
}

/// 140 kt is 8 kt over VMO (Medium) and 15 kt over VLO while the gear
/// moves (High); under VLE with it down. In calm air nothing else.
#[test]
fn speed_limits_of_the_type_profile_drive_their_rules() {
    assert_eq!(
        limits_flight((0.0, 0.0)),
        found(&[
            ("vmo-exceedance", Level::Medium),
            ("gear-operating-overspeed", Level::High),
        ])
    );
}

/// The 140,000 lb test aircraft over an MLW of 138,000 lb (1.4 %, Medium)
/// and of 130,000 lb (7.7 %, High); under it nothing.
#[test]
fn a_landing_above_the_mlw_is_graded_by_its_margin() {
    let with = |mlw: &str| {
        let profile = format!("{LIMITS_PROFILE}mlw: {{ value: {mlw}, source: synthetic }}\n");
        let events = limits_flight_with(&profile, (0.0, 0.0));
        events
            .into_iter()
            .filter(|(r, _)| r == "overweight-landing")
            .map(|(_, l)| l)
            .collect::<Vec<_>>()
    };
    assert_eq!(with("138000 lb"), [Level::Medium]);
    assert_eq!(with("130000 lb"), [Level::High]);
    assert_eq!(with("150000 lb"), []);
}

/// The approach at Mach 0.215 against an MMO of 0.2 (0.015 over, Medium);
/// a recording without Mach stays silent.
#[test]
fn mach_above_mmo_is_an_overspeed() {
    let profile = format!("{LIMITS_PROFILE}mmo: {{ value: 0.2 M, source: synthetic }}\n");
    let fly = |mach: Option<f64>| {
        let (flight, track) = fly_to(&[identity("TEST")], AIRLINER_LB, TD_S + ROLLOUT_S, |t| Pt {
            set: mach.map(|m| ("AIRSPEED MACH", m)).into_iter().collect(),
            ..landing(t, TD_S, 1.2)
        });
        let profiles = ProfileSet::load([("TEST", profile.as_str())]).unwrap();
        let events = graded(&profiles, CORE, &flight, &track);
        events
            .into_iter()
            .filter(|e| e.rule == "mmo-exceedance")
            .collect::<Vec<_>>()
    };
    let [e] = &fly(Some(0.215))[..] else {
        panic!("expected one Mmo exceedance")
    };
    assert_eq!(e.level, Level::Medium);
    assert_eq!(e.detail.as_deref(), Some("0.215 M, limit 0.200 M"));
    assert!(fly(None).is_empty());
}

/// A helicopter (the sim's `CATEGORY`) touching down past the touchdown
/// zone: no runway landing rule grades it.
#[test]
fn runway_rules_skip_a_helicopter() {
    let SimEvent::Aircraft(mut heli) = identity("H125") else {
        unreachable!()
    };
    heli.facts.category = Some("Helicopter".into());
    let recorded = [
        SimEvent::Aircraft(heli),
        SimEvent::Airport(runway_behind_touchdown(1500.0)),
    ];
    let (flight, track) = fly_to(&recorded, LIGHT_LB, TD_S + ROLLOUT_S, |t| {
        landing(t, TD_S, 1.2)
    });
    assert_eq!(events(CORE, &flight, &track), []);
    let (flight, track) = land_on(&[runway_behind_touchdown(1500.0)]);
    assert_eq!(
        events(CORE, &flight, &track),
        found(&[("long-landing", Level::Medium)])
    );
}

/// The gear retracted and put back down between two of its 4 s samples:
/// no sample saw it in transit, so nothing says it moved above VLO.
#[test]
fn gear_unseen_in_transit_is_not_an_operating_overspeed() {
    let events = [identity("TEST")];
    let (flight, track) = fly_to(&events, AIRLINER_LB, TD_S + ROLLOUT_S, |t| Pt {
        gear: if (30.0..33.0).contains(&t) {
            0.0
        } else {
            100.0
        },
        ..landing(t, TD_S, 1.2)
    });
    let at = |s: f64| {
        flight.at(
            "gear.extended",
            flight.span().unwrap().0.add_us((s * 1e6) as i64),
        )
    };
    assert!((29..34).all(|s| at(f64::from(s)).is_some_and(|g| g == 0.0 || g == 100.0)));
    let profiles = ProfileSet::load([("TEST", LIMITS_PROFILE)]).unwrap();
    let events = pairs(graded(&profiles, CORE, &flight, &track));
    assert!(
        !events.iter().any(|(r, _)| r == "gear-operating-overspeed"),
        "{events:?}"
    );
}

/// 17 kt square across runway 20 is 7 kt over the demonstrated crosswind
/// (Medium); 12 kt straight behind it is 7 kt over the certified tailwind
/// limit (High, more than 5 kt).
#[test]
fn touchdown_wind_is_resolved_on_the_runway() {
    let wind = |from: f64, kt: f64| {
        limits_flight((from, kt))
            .into_iter()
            .filter(|(rule, _)| !rule.contains("speed") && rule != "vmo-exceedance")
            .collect::<Vec<_>>()
    };
    assert_eq!(
        wind(290.0, 17.0),
        found(&[("crosswind-above-demonstrated", Level::Medium)])
    );
    assert_eq!(
        wind(20.0, 12.0),
        found(&[("tailwind-landing-limit", Level::High)])
    );
    assert_eq!(wind(200.0, 25.0), []);
}

/// A jet whose Vref is 115 kt at any weight with the flaps up, so the
/// 140 kt approach is Vref+25 throughout.
const VREF_PROFILE: &str = r#"
type: TEST
name: Synthetic test jet
class: transport
vref:
  source: synthetic
  flaps:
    - { flap: 0 deg, weights_kg: [50000, 70000], ias: [115 kt, 115 kt] }
"#;

/// Rule set v2, Data4Safety's split at 500 ft: Vref+25 is past the first
/// threshold (+20) above 500 ft (SL1, Low) and below it (SL2B, Medium).
/// Without a Vref the rules stay silent (the stable landing above).
#[test]
fn approach_speed_is_graded_against_vref_split_at_500_ft() {
    let events = [identity("TEST")];
    let (flight, track) = fly_to(&events, AIRLINER_LB, TD_S + ROLLOUT_S, |t| {
        landing(t, TD_S, 1.2)
    });
    let profiles = ProfileSet::load([("TEST", VREF_PROFILE)]).unwrap();
    let speed = graded(&profiles, CORE, &flight, &track);
    assert_eq!(
        pairs(speed.clone()),
        found(&[
            ("unstable-speed-above-500", Level::Low),
            ("unstable-speed-below-500", Level::Medium),
        ])
    );
    assert_eq!(
        speed[0].detail.as_deref(),
        Some("140 kt, 25 kt from Vref 115 kt, limit 20 kt")
    );
}

/// A C172 at 84 kt with full flaps: its POH approach speed is 65 kt (60-70),
/// so +19 kt is past the light first level (+15) but not the second (+20).
#[test]
fn a_light_approach_speed_is_graded_against_the_published_range() {
    let (flight, track) = fly_to(&[identity("C172")], LIGHT_LB, TD_S + ROLLOUT_S, |t| {
        let p = landing(t, TD_S, 1.2);
        Pt {
            speed: if t < TD_S { 84.0 } else { p.speed.min(84.0) },
            flaps: 30.0,
            ..p
        }
    });
    assert_eq!(
        events(CORE, &flight, &track),
        found(&[("unstable-speed-light", Level::Low)])
    );
}

/// 1290 fpm held for 20 s below 500 ft: past Data4Safety's first
/// threshold, which below 500 ft is SL2B (Medium); the same descent in a
/// light aircraft is its first level (Low).
#[test]
fn a_high_sink_below_500_ft_is_split_by_class() {
    let script = |t: f64| Pt {
        alt: lerp(
            &[
                (0.0, 1500.0),
                (TD_S - 50.0, 480.0),
                (TD_S - 30.0, 50.0),
                (TD_S - 2.0, 10.0),
                (TD_S, 0.0),
            ],
            t,
        ),
        ..landing(t, TD_S, 1.2)
    };
    let (flight, track) = fly(AIRLINER_LB, TD_S + ROLLOUT_S, script);
    assert_eq!(
        events(CORE, &flight, &track),
        found(&[("high-sink-below-500", Level::Medium)])
    );
    let (flight, track) = fly(LIGHT_LB, TD_S + ROLLOUT_S, script);
    assert_eq!(
        events(CORE, &flight, &track),
        found(&[("high-sink-light", Level::Low)])
    );
}

/// 22 deg of bank held for 5 s from 225 ft to 160 ft: Data4Safety's first
/// threshold below 500 ft (15 deg, SL2B: Medium) for a jet; for a light
/// aircraft, below 300 ft, past its second level (20 deg).
#[test]
fn bank_on_short_final_is_split_by_class() {
    let script = |t: f64| Pt {
        set: if (TD_S - 22.0..TD_S - 17.0).contains(&t) {
            vec![("PLANE BANK DEGREES", -22.0)]
        } else {
            vec![]
        },
        ..landing(t, TD_S, 1.2)
    };
    for (lb, rule) in [(AIRLINER_LB, "bank-below-500"), (LIGHT_LB, "bank-light")] {
        let (flight, track) = fly(lb, TD_S + ROLLOUT_S, script);
        assert_eq!(
            events(CORE, &flight, &track),
            found(&[(rule, Level::Medium)])
        );
    }
}

/// 450 fpm at contact: past the transport first level (360 fpm, 6 ft/s of
/// 14 CFR 25.473) and the light one (400 fpm).
#[test]
fn touchdown_sink_is_split_by_class() {
    let script = |t: f64| Pt {
        alt: lerp(
            &[
                (0.0, 1500.0),
                (TD_S - 12.0, 100.0),
                (TD_S - 1.0, 7.5),
                (TD_S, 0.0),
            ],
            t,
        ),
        ..landing(t, TD_S, 1.2)
    };
    for (lb, rule) in [
        (AIRLINER_LB, "high-sink-touchdown"),
        (LIGHT_LB, "high-sink-touchdown-light"),
    ] {
        let (flight, track) = fly(lb, TD_S + ROLLOUT_S, script);
        assert_eq!(events(CORE, &flight, &track), found(&[(rule, Level::Low)]));
    }
}

/// 1.5 dots below the glideslope held from 1000 ft to 520 ft: past
/// Data4Safety's first threshold (1 dot), not its second (2 dots). The
/// signal is lost before the 500 ft gate, which then never fires.
#[test]
fn glideslope_deviation_above_500_ft() {
    let (flight, track) = fly(AIRLINER_LB, TD_S + ROLLOUT_S, |t| {
        let p = landing(t, TD_S, 1.2);
        let on = p.alt > 520.0;
        Pt {
            set: vec![
                ("NAV HAS GLIDE SLOPE:1", f64::from(u8::from(on))),
                ("NAV GSI:1", if on { 1.5 * 119.0 / 2.0 } else { 0.0 }),
            ],
            ..p
        }
    });
    assert_eq!(
        events(CORE, &flight, &track),
        found(&[("glideslope-deviation-above-500", Level::Low)])
    );
}

/// Every core rule belongs to the domain docs/scoring.md §3 gives it; the
/// loader already rejects a rule without one.
#[test]
fn core_rules_map_to_the_scoring_domains() {
    use rules::Domain::*;
    let mut by_domain: Vec<(rules::Domain, Vec<String>)> = Vec::new();
    for r in load(CORE).unwrap() {
        match by_domain.iter_mut().find(|(d, _)| *d == r.domain) {
            Some((_, ids)) => ids.push(r.id),
            None => by_domain.push((r.domain, vec![r.id])),
        }
    }
    let ids = |d| {
        let mut ids = by_domain.iter().find(|(x, _)| *x == d).unwrap().1.clone();
        ids.sort();
        ids.join(" ")
    };
    assert_eq!(
        ids(Stability),
        "bank-above-500 bank-below-500 bank-light bank-light-above-300 gear-not-down-500 \
         glideslope-deviation-500 glideslope-deviation-above-500 high-sink-above-500 \
         high-sink-below-500 high-sink-light high-sink-light-above-500 localizer-deviation-500 \
         localizer-deviation-above-500 unstable-bank-500 unstable-sink-500 \
         unstable-speed-above-500 unstable-speed-below-500 unstable-speed-light"
    );
    assert_eq!(
        ids(Energy),
        "flap-overspeed gear-extended-overspeed gear-operating-overspeed mmo-exceedance \
         stall-warning stall-warning-light vmo-exceedance"
    );
    assert_eq!(ids(Point), "long-landing short-landing");
    assert_eq!(
        ids(Lateral),
        "crosswind-above-demonstrated off-centreline-touchdown"
    );
    assert_eq!(
        ids(Contact),
        "bounced-landing hard-landing hard-landing-inspection hard-landing-light \
         high-sink-touchdown high-sink-touchdown-light tailstrike-risk-landing"
    );
    assert_eq!(ids(Rollout), "tailwind-landing-limit");
    assert_eq!(
        ids(Other),
        "early-flap-retraction go-around overweight-landing rejected-takeoff \
         rejected-takeoff-light rotation-rate tailstrike-risk-takeoff"
    );
}
