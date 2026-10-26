//! The track and its phase while the flight is still being flown, one
//! recorded frame at a time, for the in-sim panel. Same methods and
//! thresholds as the ground segment (aerodrome height from the pressure
//! altitude on the ground, the inertial VS filter, the parked RA bias, the
//! CICTT phase boundaries) but without look-ahead: a phase that the ground
//! segment decides from what follows (the top of a climb, a go-around, a
//! bounce) is decided here once it has happened, so the debrief can differ
//! by a few seconds.

use fdm_core::SimTime;

use crate::Phase;
use crate::phase::{
    APPROACH_START_FT, FLARE_FT, GO_AROUND_DROP_FT, GO_AROUND_RISE_FT, INITIAL_CLIMB_END_FT,
    ROLL_MIN_GS_KT, STANDING_MAX_GS_KT, TAKEOFF_END_FT,
};
use crate::track::{BOUNCE_MAX_US, RA_BIAS_MAX_GS_KT, RA_MAX_FT};
use crate::vs::Inertial;

/// One recorded frame's parameters; `NaN` where missing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveInput {
    pub t: SimTime,
    pub on_ground: Option<bool>,
    pub alt_ft: f64,
    pub gs_kt: f64,
    pub ra_ft: f64,
    pub nz_g: f64,
    pub pitch_deg: f64,
    pub roll_deg: f64,
}

/// The derived values at the latest frame; `NaN` where unknown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LivePoint {
    pub phase: Phase,
    pub on_ground: bool,
    /// Above the aerodrome of the last ground contact (departure, or the
    /// last landing), as `Track::aal_ft` has it for a flight not yet landed.
    pub aal_ft: f64,
    /// Radio height less the parked bias, `NaN` above 2500 ft.
    pub ra_ft: f64,
    pub vs_fpm: f64,
}

#[derive(Debug, Clone)]
pub struct LiveTrack {
    last: Option<LivePoint>,
    last_t: Option<SimTime>,
    /// Pressure altitude of the aerodrome: the last one on the ground.
    aerodrome_ft: Option<f64>,
    ra_bias: f64,
    vs: Option<Inertial>,
    touchdown: Option<SimTime>,
    /// Lowest height of an approach, highest of a climb or go-around.
    extreme_ft: f64,
}

impl Default for LiveTrack {
    fn default() -> Self {
        Self {
            last: None,
            last_t: None,
            aerodrome_ft: None,
            ra_bias: 0.0,
            vs: None,
            touchdown: None,
            extreme_ft: f64::NAN,
        }
    }
}

impl LiveTrack {
    /// The latest point, if a frame was stepped.
    pub fn last(&self) -> Option<LivePoint> {
        self.last
    }

    pub fn step(&mut self, x: &LiveInput) -> LivePoint {
        let dt = self
            .last_t
            .map_or(0.0, |t| x.t.us_since(t).max(0) as f64 / 1e6);
        self.last_t = Some(x.t);
        let was = self.last.map(|p| p.on_ground);
        // A gap keeps the last known state; on the ground before any.
        let on_ground = x.on_ground.or(was).unwrap_or(true);
        if on_ground {
            if x.alt_ft.is_finite() {
                self.aerodrome_ft = Some(x.alt_ft);
            }
            if x.gs_kt < RA_BIAS_MAX_GS_KT && x.ra_ft.is_finite() {
                self.ra_bias = x.ra_ft;
            }
        }
        let ra_ft = Some(x.ra_ft - self.ra_bias)
            .filter(|r| *r <= RA_MAX_FT)
            .unwrap_or(f64::NAN);
        let aal_ft = match self.aerodrome_ft {
            _ if on_ground => 0.0,
            Some(e) => x.alt_ft - e,
            None => f64::NAN,
        };
        if self.vs.is_none() && x.alt_ft.is_finite() {
            self.vs = Some(Inertial::new(x.alt_ft));
        }
        let vs_fpm = self.vs.as_mut().map_or(f64::NAN, |f| {
            f.step(x.alt_ft, x.nz_g, x.pitch_deg, x.roll_deg, on_ground, dt)
        });
        let phase = self.phase(x, was, on_ground, aal_ft, ra_ft);
        let point = LivePoint {
            phase,
            on_ground,
            aal_ft,
            ra_ft,
            vs_fpm,
        };
        self.last = Some(point);
        point
    }

    fn phase(
        &mut self,
        x: &LiveInput,
        was: Option<bool>,
        on_ground: bool,
        aal: f64,
        ra: f64,
    ) -> Phase {
        use Phase::*;
        let mut p = self.last.map_or(Standing, |l| l.phase);
        let gs = x.gs_kt;
        if on_ground {
            if was == Some(false) {
                self.touchdown = Some(x.t);
                p = Landing;
            }
            return match p {
                // The rollout, or a dropout in ground speed.
                Landing if gs >= ROLL_MIN_GS_KT || gs.is_nan() => Landing,
                _ if gs.is_nan() => p,
                _ if gs >= ROLL_MIN_GS_KT => Takeoff,
                _ if gs >= STANDING_MAX_GS_KT => Taxi,
                _ => Standing,
            };
        }
        if was == Some(true) {
            let bounce = p == Landing
                && self
                    .touchdown
                    .is_some_and(|td| x.t.us_since(td) <= BOUNCE_MAX_US);
            p = if bounce { Landing } else { Takeoff };
            self.extreme_ft = aal;
        }
        // Calibrated RA where valid, else AAL, as `phase::runway_height`.
        let height = if ra.is_finite() { ra } else { aal };
        let (low, high) = (self.extreme_ft.min(aal), self.extreme_ft.max(aal));
        let next = match p {
            Takeoff if height >= TAKEOFF_END_FT => InitialClimb,
            InitialClimb | GoAround if aal >= INITIAL_CLIMB_END_FT => EnRoute,
            // Past the top of a climb below 1000 ft: an approach.
            InitialClimb | GoAround if aal <= high - GO_AROUND_DROP_FT => Approach,
            EnRoute if aal < APPROACH_START_FT => Approach,
            Approach | Landing if aal >= low + GO_AROUND_RISE_FT => GoAround,
            Approach if aal >= APPROACH_START_FT => EnRoute,
            Approach if height < FLARE_FT => Landing,
            // Airborne with no liftoff seen: the flight started in the air.
            Standing | Taxi => EnRoute,
            _ => p,
        };
        self.extreme_ft = match next {
            _ if next != p => aal,
            Approach | Landing => low,
            InitialClimb | GoAround => high,
            _ => self.extreme_ft,
        };
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 8 Hz frames of (on ground, pressure altitude, ground speed), level.
    fn run(frames: &[(bool, f64, f64)]) -> Vec<LivePoint> {
        let mut track = LiveTrack::default();
        frames
            .iter()
            .enumerate()
            .map(|(i, &(g, alt, gs))| {
                track.step(&LiveInput {
                    t: SimTime::from_micros(i as i64 * 125_000),
                    on_ground: Some(g),
                    alt_ft: alt,
                    gs_kt: gs,
                    ra_ft: f64::NAN,
                    nz_g: 1.0,
                    pitch_deg: 0.0,
                    roll_deg: 0.0,
                })
            })
            .collect()
    }

    fn phases(points: &[LivePoint]) -> Vec<Phase> {
        let mut out: Vec<Phase> = Vec::new();
        for p in points {
            if out.last() != Some(&p.phase) {
                out.push(p.phase);
            }
        }
        out
    }

    /// A circuit from an aerodrome at 500 ft: taxi, takeoff, climb to
    /// `top` above it, descend, land, roll out and stop.
    fn circuit(top: f64) -> Vec<(bool, f64, f64)> {
        let mut f = vec![(true, 500.0, 0.0); 4];
        f.extend([(true, 500.0, 10.0); 4]);
        f.extend([(true, 500.0, 60.0); 4]);
        f.extend((1..=40).map(|i| (false, 500.0 + top * f64::from(i) / 40.0, 70.0)));
        f.extend((1..=40).map(|i| (false, 500.0 + top - top * f64::from(i) / 40.0, 70.0)));
        f.extend([(true, 500.0, 50.0); 4]);
        f.extend([(true, 500.0, 10.0); 4]);
        f.extend([(true, 500.0, 0.0); 4]);
        f
    }

    #[test]
    fn a_low_circuit_climbs_then_approaches_from_its_top() {
        use Phase::*;
        let points = run(&circuit(800.0));
        assert_eq!(
            phases(&points),
            [
                Standing,
                Taxi,
                Takeoff,
                InitialClimb,
                Approach,
                Landing,
                Taxi,
                Standing
            ]
        );
        // Height above the departure aerodrome, 0 on the ground.
        assert_eq!(points[12 + 39].aal_ft, 800.0);
        assert_eq!(points[0].aal_ft, 0.0);
    }

    #[test]
    fn a_high_circuit_goes_en_route_above_1000_ft() {
        use Phase::*;
        let points = run(&circuit(1500.0));
        assert_eq!(
            phases(&points),
            [
                Standing,
                Taxi,
                Takeoff,
                InitialClimb,
                EnRoute,
                Approach,
                Landing,
                Taxi,
                Standing
            ]
        );
    }

    #[test]
    fn a_climb_of_200_ft_on_approach_is_a_go_around() {
        use Phase::*;
        let mut f = circuit(800.0)[..12 + 60].to_vec(); // down to 400 ft
        f.extend((1..=20).map(|i| (false, 900.0 + 15.0 * f64::from(i), 70.0)));
        let points = run(&f);
        assert_eq!(
            phases(&points),
            [Standing, Taxi, Takeoff, InitialClimb, Approach, GoAround]
        );
    }

    #[test]
    fn a_short_bounce_stays_in_the_landing() {
        use Phase::*;
        let mut f = circuit(800.0)[..12 + 80].to_vec();
        f.extend([(true, 500.0, 50.0); 8]); // 1 s on the ground
        f.extend([(false, 505.0, 50.0); 8]);
        f.extend([(true, 500.0, 40.0); 8]);
        let points = run(&f);
        assert_eq!(phases(&points)[4..], [Approach, Landing]);
    }

    #[test]
    fn vs_follows_a_steady_descent() {
        let f: Vec<_> = (0..8 * 60)
            .map(|i| (false, 3000.0 - 10.0 * f64::from(i) / 8.0, 100.0))
            .collect();
        let last = *run(&f).last().unwrap();
        assert!((last.vs_fpm + 600.0).abs() < 5.0, "{}", last.vs_fpm);
        // Airborne from the start: no aerodrome, so no height above it.
        assert!(last.aal_ft.is_nan());
        assert_eq!(last.phase, Phase::EnRoute);
    }
}
