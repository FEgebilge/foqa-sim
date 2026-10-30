//! Which runway an ILS deviation belongs to. A localizer and its glideslope
//! guide to one runway end, and an FDM program reads their deviations only
//! on an approach to that end. Tuned to another one (a parallel runway, the
//! reciprocal, another airport) the receiver still shows a deviation, often
//! full scale, that says nothing about the approach flown.
//!
//! Which localizer was tuned is read from physics, not from its identity:
//! MSFS 2024 does not report the identity strings (`NAV LOC AIRPORT IDENT`,
//! `NAV LOC RUNWAY NUMBER`, `NAV LOC RUNWAY DESIGNATOR` came back empty or
//! holding another request's text, 2026-10-30). A localizer serves the
//! runway landed on when its course points along that runway and its needle
//! is near the middle at touchdown: landing on the centreline puts this
//! runway's localizer there, while a parallel runway's, aligned the same
//! way, sits near full scale.

use std::ops::RangeInclusive;

use fdm_core::SimTime;

use crate::runway::{magnetic_deg, wrap180};
use crate::{Flight, Phase, RunwayFix, Span, Touchdown};

/// The ILS deviation parameters.
const ILS: [&str; 2] = ["nav.loc_dev", "nav.gs_dev"];
/// The tuned localizer's course.
const COURSE: &str = "nav.loc_course";
/// How far the course may be from the runway's heading. Rejects the
/// reciprocal and crossing runways; parallel runways share a heading and are
/// told apart by the needle (`CENTRED_DOTS`).
const COURSE_TOLERANCE_DEG: f64 = 10.0;
/// The localizer needle at touchdown on this runway's localizer: within a
/// dot of the middle (a centreline landing reads near 0). A parallel
/// runway's localizer, a runway width or more away, reads near full scale.
const CENTRED_DOTS: f64 = 1.0;

/// Reads `nav.loc_dev` and `nav.gs_dev` as no computed data except on the
/// way to the runway end NAV1's localizer serves: en route and on approach
/// before a landing, from the climb (takeoff or go-around) before them to
/// the touchdown. The localizer serves that runway when the course recorded
/// at or just before the touchdown points along it (`aligned`) and the
/// localizer needle reads within a dot of the middle at the touchdown;
/// before the touchdown the deviations count only while the course in force
/// is aligned too, so a retune on the way leaves out what came before it.
/// Unknown everywhere else, so no deviation is graded on a guess: a
/// go-around's approach (no touchdown names its runway), a touchdown matched
/// to no runway, and every recording without `nav.loc_course` (all made
/// before 0.11.3).
pub fn mask_ils(flight: &mut Flight, phases: &[Span], touchdowns: &[Touchdown]) {
    let course: Vec<(SimTime, f64)> = (flight.series(COURSE).iter())
        .filter(|s| s.is_valid())
        .map(|s| (s.t, s.value))
        .collect();
    let course_at = |t: SimTime| {
        let next = course.partition_point(|(at, _)| *at <= t);
        course.get(next.checked_sub(1)?).copied()
    };
    let loc = flight.series(ILS[0]);
    let approaches: Vec<(RangeInclusive<SimTime>, &RunwayFix)> = (touchdowns.iter())
        .filter(|td| !td.bounce)
        .filter_map(|td| {
            let fix = td.runway.as_ref()?;
            let landing = (phases.iter())
                .position(|s| s.phase == Phase::Landing && (s.start..=s.end).contains(&td.t))?;
            let inbound = (phases[..landing].iter().rev())
                .take_while(|s| matches!(s.phase, Phase::Approach | Phase::EnRoute))
                .last()?;
            let on = inbound.start..=td.t;
            let (at, deg) = course_at(td.t)?;
            let needle = loc.get(loc.partition_point(|s| s.t <= td.t).checked_sub(1)?)?;
            let centred = needle.is_valid() && needle.value.abs() < CENTRED_DOTS;
            (on.contains(&at) && on.contains(&needle.t) && aligned(deg, fix) && centred)
                .then_some((on, fix))
        })
        .collect();
    let serves = |t: SimTime| {
        (approaches.iter()).any(|(on, fix)| {
            on.contains(&t) && course_at(t).is_some_and(|(_, deg)| aligned(deg, fix))
        })
    };
    for name in ILS {
        flight.withhold(name, serves);
    }
}

/// Whether a localizer course points along the runway landed on. The SDK
/// does not say whether `NAV LOCALIZER` is magnetic, as a published course
/// is, or true: either reading is accepted. A runway that far off the
/// magnetic and true headings alike is another runway.
fn aligned(course_deg: f64, fix: &RunwayFix) -> bool {
    let magnetic = magnetic_deg(fix.heading_deg, fix.magvar_deg);
    [magnetic, fix.heading_deg]
        .iter()
        .any(|h| wrap180(course_deg - h).abs() <= COURSE_TOLERANCE_DEG)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flight::tests::{Row, flight, ok};
    use crate::{Basis, Cause, Trigger};

    const LOC: &str = "nav.loc_dev";
    /// 34L's true heading and its magnetic one (MAGVAR -6, east).
    const TRUE: f64 = 340.0;
    const MAGNETIC: f64 = 334.0;

    fn s(secs: f64) -> SimTime {
        SimTime::from_secs_f64(secs)
    }

    fn span(phase: Phase, start: f64, end: f64) -> Span {
        let cause = Cause {
            trigger: Trigger::Airborne,
            value: f64::NAN,
            threshold: f64::NAN,
            basis: Basis::Aal,
        };
        Span {
            phase,
            start: s(start),
            end: s(end),
            cause,
        }
    }

    /// A touchdown at `t` on LTBJ 34L.
    fn on_34l(t: f64) -> Touchdown {
        let runway = RunwayFix {
            icao: "LTBJ".into(),
            runway: "34L".into(),
            heading_deg: TRUE,
            along_m: 300.0,
            cross_m: 0.0,
            lda_m: 3000.0,
            tdz_m: 900.0,
            aiming_point_m: 400.0,
            width_m: 45.0,
            threshold: (0.0, 0.0),
            magvar_deg: MAGNETIC - TRUE,
            in_tdz: true,
            long_landing: false,
            short_landing: false,
        };
        Touchdown {
            t: s(t),
            bounce: false,
            sink_fpm: 200.0,
            peak_g: 1.2,
            pitch_deg: None,
            roll_deg: None,
            ias_kt: None,
            sim_sink_fpm: None,
            runway: Some(runway),
        }
    }

    /// The seconds at which `nav.loc_dev` stays valid, after a climb to
    /// 20 s, an approach from 50 s and a landing on 34L at 92 s. The needle
    /// reads `dots` throughout; `course` gives the tuned course each second,
    /// `None` for a recording without it.
    fn valid(course: impl Fn(u32) -> Option<f64>, dots: f64) -> Vec<u32> {
        let mut rows: Vec<Row> = (0..100).map(|i| ok(LOC, f64::from(i), dots)).collect();
        rows.extend((0..100).filter_map(|i| Some(ok(COURSE, f64::from(i), course(i)?))));
        let mut f = flight(&rows);
        let phases = [
            span(Phase::InitialClimb, 0.0, 20.0),
            span(Phase::EnRoute, 20.0, 50.0),
            span(Phase::Approach, 50.0, 90.0),
            span(Phase::Landing, 90.0, 100.0),
        ];
        mask_ils(&mut f, &phases, &[on_34l(92.0)]);
        (f.series(LOC).iter())
            .filter(|x| x.is_valid())
            .map(|x| x.t.as_secs_f64() as u32)
            .collect()
    }

    fn tuned(deg: f64) -> impl Fn(u32) -> Option<f64> {
        move |_| Some(deg)
    }

    #[test]
    fn deviations_count_on_the_centreline_of_the_runway_the_course_points_along() {
        let approach: Vec<u32> = (20..=92).collect();
        assert_eq!(valid(tuned(MAGNETIC + 2.0), 0.2), approach, "magnetic");
        assert_eq!(valid(tuned(TRUE), -0.4), approach, "true");
        assert_eq!(valid(tuned(MAGNETIC - 9.0), 0.0), approach, "tolerance");
    }

    #[test]
    fn another_runways_localizer_is_left_out() {
        assert!(
            valid(tuned(MAGNETIC - 180.0), 0.0).is_empty(),
            "the other end"
        );
        assert!(
            valid(tuned(MAGNETIC + 40.0), 0.0).is_empty(),
            "a crossing runway"
        );
        // 34R: the same course, the needle near full scale on 34L.
        assert!(
            valid(tuned(MAGNETIC), 2.0).is_empty(),
            "the parallel runway"
        );
        assert!(
            valid(tuned(MAGNETIC), -1.2).is_empty(),
            "the parallel runway"
        );
        assert!(
            valid(|_| None, 0.0).is_empty(),
            "recorded before the course"
        );
    }

    #[test]
    fn a_retune_on_the_way_counts_from_the_retune() {
        let retuned = |i| Some(if i < 70 { MAGNETIC - 180.0 } else { MAGNETIC });
        assert_eq!(valid(retuned, 0.1), (70..=92).collect::<Vec<_>>());
        // Retuned away before the touchdown: nothing names this runway.
        let away = |i| Some(if i < 85 { MAGNETIC } else { MAGNETIC - 180.0 });
        assert!(valid(away, 0.1).is_empty());
    }

    #[test]
    fn a_go_around_names_no_runway() {
        let mut rows: Vec<Row> = (0..100).map(|i| ok(LOC, f64::from(i), 0.1)).collect();
        rows.extend((0..100).map(|i| ok(COURSE, f64::from(i), MAGNETIC)));
        let mut f = flight(&rows);
        let phases = [
            span(Phase::Approach, 0.0, 40.0),
            span(Phase::GoAround, 40.0, 60.0),
            span(Phase::EnRoute, 60.0, 100.0),
        ];
        mask_ils(&mut f, &phases, &[]);
        assert!(f.series(LOC).iter().all(|x| !x.is_valid()));
    }
}
