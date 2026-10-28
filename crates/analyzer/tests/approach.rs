//! The approach and rollout values of synthetic landings (`testkit`), whose
//! answers are known by construction: the stable 140 kt airliner approach
//! (1500 ft to 100 ft over 112 s, then 10 ft at 2 s before contact) onto
//! runway 20, its threshold 400 m behind the touchdown, 3000 m long.

use analyzer::{Approach, Config, Flight, Track, touchdowns};
use recorder::SimEvent;
use testkit::*;

/// The landing of `script` on the runway, with its values for a 1000 ft
/// to 50 ft window.
fn approach(script: impl Fn(f64) -> Pt) -> Approach {
    let events = [SimEvent::Airport(runway_behind_touchdown(400.0))];
    let (flight, track): (Flight, Track) = fly_to(&events, AIRLINER_LB, TD_S + ROLLOUT_S, script);
    let tds = touchdowns(&flight, &track);
    Approach::of(&flight, &track, &tds[0], 1000.0, 50.0)
}

fn near(x: f64, want: f64, tol: f64) -> bool {
    (x - want).abs() <= tol
}

#[test]
fn a_stable_landing_has_the_values_of_its_script() {
    let a = approach(|t| landing(t, TD_S, 1.2));
    let w = &a.window;
    // 1000 ft at 84 s before contact, under 50 ft at 6.4 s.
    let span = w.t[w.t.len() - 1].us_since(w.t[0]) as f64 / 1e6;
    assert!(near(span, 84.0 - 6.4, 0.3), "{span}");
    assert!(w.height_ft[0] <= 1000.0 && *w.height_ft.last().unwrap() >= 50.0);
    // 1400 ft over 112 s: 750 fpm.
    let mid = w.sink_fpm[w.t.len() / 2];
    assert!(near(mid, 750.0, 30.0), "{mid}");
    assert_eq!(w.ias_kt[0], 140.0);
    assert!(a.config_changes.is_empty());
    // 50 ft at 6.44 s before contact.
    assert!(near(a.float_s.unwrap(), 6.44, 0.15), "{:?}", a.float_s);
    // The threshold is 5.55 s back at 140 kt: 41.9 ft.
    let c = a.threshold.as_ref().unwrap();
    assert!(near(c.height_ft, 41.9, 1.0), "{c:?}");
    assert_eq!((c.ias_kt, c.gs_kt), (140.0, 140.0));
    assert!(near(a.crab_deg.unwrap(), 0.0, 0.01) && near(a.drift_kt.unwrap(), 0.0, 0.1));
    // 140 kt with 2600 m left: 72.0² / 5200 m/s², 0.102 g.
    assert!(near(a.required_decel_g.unwrap(), 0.1017, 0.002));
    assert!(a.rollout_cross_m.unwrap() < 1.0, "{:?}", a.rollout_cross_m);
    // Slowing from 140 to 20 kt over 40 s covers 1646 m.
    let s = a.slowed.as_ref().unwrap();
    assert!(near(s.along_m, 400.0 + 1646.0, 20.0), "{s:?}");
    assert_eq!(a.decel_delay_s, None);
}

#[test]
fn crab_and_drift_come_from_heading_and_track() {
    let a = approach(|t| Pt {
        heading: if t > TD_S - 10.0 { 205.0 } else { 200.0 },
        track: if t > TD_S - 3.0 { 202.0 } else { 200.0 },
        ..landing(t, TD_S, 1.2)
    });
    assert!(near(a.crab_deg.unwrap(), 5.0, 0.01), "{:?}", a.crab_deg);
    // 140 kt at 2 deg to the runway: 4.9 kt to the right.
    assert!(near(a.drift_kt.unwrap(), 4.89, 0.2), "{:?}", a.drift_kt);
}

#[test]
fn the_first_deceleration_means_sets_the_delay() {
    let a = approach(|t| Pt {
        set: vec![
            (
                "BRAKE LEFT POSITION",
                if t > TD_S + 2.5 { 50.0 } else { 0.0 },
            ),
            (
                "SPOILERS HANDLE POSITION",
                if t > TD_S + 4.0 { 100.0 } else { 0.0 },
            ),
        ],
        ..landing(t, TD_S, 1.2)
    });
    // Brakes at 1 Hz: seen within a second of 2.5 s.
    let d = a.decel_delay_s.unwrap();
    assert!((2.5..=3.6).contains(&d), "{d}");
}

#[test]
fn a_flap_change_in_the_window_is_found_once() {
    let a = approach(|t| Pt {
        flaps: lerp(&[(TD_S - 60.0, 15.0), (TD_S - 55.0, 30.0)], t),
        ..landing(t, TD_S, 1.2)
    });
    let [c] = &a.config_changes[..] else {
        panic!("{:?}", a.config_changes)
    };
    assert_eq!((c.what, c.from, c.to), (Config::Flaps, 15.0, 30.0));
    // 60 s before contact: 700 ft.
    assert!(near(c.height_ft, 700.0, 40.0), "{c:?}");
}

#[test]
fn a_fast_turn_off_past_the_tdz_ends_the_centreline_tracking() {
    // 140 kt slowing 3 kt/s: 40 kt at 33.3 s, ~1940 m past the threshold
    // (TDZ 914 m); then a 35 deg turn that goes 30 m right by 30 kt.
    let exit = |turn_kt: f64| {
        let at = TD_S + (140.0 - turn_kt) / 3.0;
        approach(move |t| {
            let track = if t > at { 235.0 } else { TRACK_DEG };
            Pt {
                heading: track,
                track,
                ..landing(t, TD_S, 1.2)
            }
        })
    };
    // The 1 s chord of the 1 Hz positions passes 15 deg within a second
    // of the turn: 20.6 m/s x sin 35 deg x < 1 s, under 12 m.
    let a = exit(40.0);
    assert!(a.rollout_cross_m.unwrap() < 12.0, "{:?}", a.rollout_cross_m);
    // The same turn at 80 kt is a veer-off: tracked to 30 kt, 22.5 m off.
    let a = exit(80.0);
    assert!(a.rollout_cross_m.unwrap() > 22.5, "{:?}", a.rollout_cross_m);
}

#[test]
fn a_slewed_approach_is_graded_from_the_end_of_the_slew() {
    // Slewed until 40 s before contact (~470 ft): the window starts 5 s
    // after it, not at 1000 ft.
    let a = approach(|t| Pt {
        set: vec![("IS SLEW ACTIVE", f64::from(u8::from(t < TD_S - 40.0)))],
        ..landing(t, TD_S, 1.2)
    });
    let start = a.window.t[0].as_secs_f64();
    let td = a.window.t.last().unwrap().as_secs_f64() + 6.4;
    assert!(near(td - start, 35.0, 0.5), "{}", td - start);
}
