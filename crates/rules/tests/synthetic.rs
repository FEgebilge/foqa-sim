//! Synthetic golden flights: scripted profiles pushed through the whole
//! on-board chain (wiring, FDAU, journal) and the ground segment, to check
//! the phase, touchdown and rule logic against flights whose answer is known
//! by construction. Every flight lands on a 600 ft aerodrome, 0 ft AGL being
//! the wheels on the runway.

use std::collections::HashMap;

use analyzer::{Flight, Phase, Track, phases, touchdowns};
use fdm_core::geo::LocalFrame;
use fdm_core::{AircraftId, Airport, Designator, Lfl, Runway, RunwayEnd};
use recorder::{Recorder, SimEvent, Telemetry};
use rules::{Event, Level, ProfileSet, evaluate, load};

const CORE: &str = include_str!("../../../rules/core.yaml");
const FRAME_HZ: f64 = 60.0;
/// Sim time of the first frame: the recording is relative to it.
const SIM_START_S: f64 = 1000.0;
const FIELD_FT: f64 = 600.0;
const AIRLINER_LB: f64 = 140_000.0;
const LIGHT_LB: f64 = 2_300.0;
/// Time from 1500 ft on the approach to touchdown.
const APPROACH_S: f64 = 124.0;
/// Touchdown time of the single-approach flights (the flight starts on the approach).
const TD_S: f64 = APPROACH_S;
/// Time from touchdown to the end of the recording, after the rollout.
const ROLLOUT_S: f64 = 45.0;

/// Heading of every scripted flight over the ground (runway 20 on a 3000 m
/// runway), and where the approach starts.
const TRACK_DEG: f64 = 200.0;
const START: (f64, f64) = (38.5, 27.0);
const KT_MS: f64 = 1852.0 / 3600.0;

/// The aircraft state at one instant, in aviation units.
struct Pt {
    /// Height above the runway (also the radio height), ft.
    alt: f64,
    /// Indicated and ground speed, kt.
    speed: f64,
    /// Normal load, g.
    g: f64,
    /// Pitch attitude, nose-up positive, deg.
    pitch: f64,
    /// Trailing-edge flap angle, deg, 0 = retracted.
    flaps: f64,
    ground: bool,
}

/// Piecewise-linear interpolation through `(t, value)` keys, held at both ends.
fn lerp(keys: &[(f64, f64)], t: f64) -> f64 {
    let next = keys.partition_point(|k| k.0 <= t);
    match (next.checked_sub(1).map(|i| keys[i]), keys.get(next)) {
        (Some((t0, v0)), Some(&(t1, v1))) => v0 + (t - t0) / (t1 - t0) * (v1 - v0),
        (Some((_, v)), None) | (None, Some(&(_, v))) => v,
        (None, None) => unreachable!("no keys"),
    }
}

/// A 1500 ft approach at 140 kt, about 750 fpm down, flared to a 300 fpm
/// touchdown at `td`, then a rollout slowing to 20 kt. `peak_g` is held for
/// 0.2 s from contact, the gear compressing.
fn landing(t: f64, td: f64, peak_g: f64) -> Pt {
    let g = if (td..td + 0.2).contains(&t) {
        peak_g
    } else {
        1.0
    };
    let descent = [
        (td - APPROACH_S, 1500.0),
        (td - 12.0, 100.0),
        (td - 2.0, 10.0),
        (td, 0.0),
    ];
    Pt {
        alt: lerp(&descent, t),
        speed: lerp(&[(td, 140.0), (td + 40.0, 20.0)], t),
        g,
        pitch: 3.0,
        flaps: 0.0,
        ground: t >= td,
    }
}

/// Records the script at 60 Hz as raw MSFS frames and analyses it. Bank is
/// fixed at zero; attitude is written the way MSFS reports it: -3.0 for
/// 3 deg nose-up (nose-up is negative), left-wing-down positive.
fn fly(weight_lb: f64, duration_s: f64, script: impl Fn(f64) -> Pt) -> (Flight, Track) {
    fly_to(&[], weight_lb, duration_s, script)
}

/// `fly`, with `events` (airport snapshots, aircraft identity) recorded at
/// the start. The aircraft flies straight along `TRACK_DEG` from `START` at
/// its ground speed.
fn fly_to(
    events: &[SimEvent],
    weight_lb: f64,
    duration_s: f64,
    script: impl Fn(f64) -> Pt,
) -> (Flight, Track) {
    let mut recorder = Recorder::new(&Lfl::V1, Vec::new()).unwrap();
    let columns: HashMap<_, _> = recorder
        .acquisition()
        .columns()
        .iter()
        .enumerate()
        .map(|(i, c)| (c.simvar, i))
        .collect();
    for e in events {
        recorder.record(&Telemetry::Event(e.clone())).unwrap();
    }
    let frame = LocalFrame::new(START.0, START.1);
    let (sin, cos) = TRACK_DEG.to_radians().sin_cos();
    let mut flown_m = 0.0;
    for k in 0..=(duration_s * FRAME_HZ) as usize {
        let t = k as f64 / FRAME_HZ;
        let p = script(t);
        flown_m += p.speed * KT_MS / FRAME_HZ;
        let (lat, lon) = frame.point(flown_m * sin, flown_m * cos);
        let mut raw = vec![f64::NAN; columns.len()];
        for (simvar, value) in [
            ("SIMULATION TIME", SIM_START_S + t),
            ("PLANE LATITUDE", lat),
            ("PLANE LONGITUDE", lon),
            ("PRESSURE ALTITUDE", FIELD_FT + p.alt),
            ("RADIO HEIGHT", p.alt),
            ("AIRSPEED INDICATED", p.speed),
            ("GROUND VELOCITY", p.speed),
            ("G FORCE", p.g),
            ("PLANE PITCH DEGREES", -p.pitch),
            ("PLANE BANK DEGREES", 0.0),
            ("SIM ON GROUND", f64::from(u8::from(p.ground))),
            ("GEAR TOTAL PCT EXTENDED", 100.0),
            ("TRAILING EDGE FLAPS LEFT ANGLE", p.flaps),
            ("AUTOPILOT AIRSPEED HOLD VAR", 140.0),
            ("STALL WARNING", 0.0),
            ("TOTAL WEIGHT", weight_lb),
        ] {
            raw[columns[simvar]] = value;
        }
        recorder.record(&Telemetry::Frame(raw)).unwrap();
    }
    let flight = Flight::read(&recorder.into_inner()[..]).unwrap();
    let track = Track::new(&flight).unwrap();
    (flight, track)
}

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

fn sequence(track: &Track) -> Vec<Phase> {
    phases(track).iter().map(|s| s.phase).collect()
}

/// A stabilised airliner approach and a smooth landing (1.2 g, 300 fpm):
/// the stable flight a FOQA programme must stay silent on.
#[test]
fn stable_airliner_approach_raises_nothing() {
    let (flight, track) = fly(AIRLINER_LB, TD_S + ROLLOUT_S, |t| landing(t, TD_S, 1.2));
    assert_eq!(
        sequence(&track),
        [Phase::EnRoute, Phase::Approach, Phase::Landing, Phase::Taxi]
    );
    assert_eq!(events(CORE, &flight, &track), []);
    let [td] = &touchdowns(&flight, &track)[..] else {
        panic!("expected one touchdown")
    };
    assert!(!td.bounce);
    assert!((td.sink_fpm - 300.0).abs() < 30.0, "{:.0} fpm", td.sink_fpm);
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
        sequence(&track),
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

/// Runway 20 whose threshold lies `along_m` behind where the stable
/// airliner landing (`landing`) touches down. The flight is flown once to
/// find that point.
fn runway_behind_touchdown(along_m: f64) -> Airport {
    let (flight, _) = fly(AIRLINER_LB, TD_S + 1.0, |t| landing(t, TD_S, 1.2));
    let t = touchdowns(&flight, &Track::new(&flight).unwrap())[0].t;
    let (lat, lon) = (
        flight.at("pos.lat", t).unwrap(),
        flight.at("pos.lon", t).unwrap(),
    );
    // The centre is half the runway past the threshold.
    let (sin, cos) = TRACK_DEG.to_radians().sin_cos();
    let past = 1500.0 - along_m;
    let (lat_deg, lon_deg) = LocalFrame::new(lat, lon).point(past * sin, past * cos);
    let end = |number| RunwayEnd {
        number,
        designator: Designator::None,
        displaced_m: 0.0,
    };
    Airport {
        icao: "TEST".into(),
        region: String::new(),
        lat_deg,
        lon_deg,
        elevation_m: 0.0,
        magvar_deg: 0.0,
        runways: vec![Runway {
            lat_deg,
            lon_deg,
            elevation_m: 0.0,
            heading_deg: TRACK_DEG,
            length_m: 3000.0,
            width_m: 45.0,
            primary: end(20),
            secondary: end(2),
        }],
    }
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

/// The identity MSFS 2024 reports for ICAO type `icao`.
fn identity(icao: &str) -> SimEvent {
    SimEvent::Aircraft(AircraftId {
        title: format!("{icao} test"),
        atc_model: format!("ATCCOM.AC_MODEL {icao}.0.text"),
        atc_type: String::new(),
        atc_id: "N123FQ".into(),
    })
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
                ground: t < LIFTOFF_S,
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
