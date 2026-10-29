//! The track and its phase while the flight is still being flown, one
//! recorded frame at a time, for the in-sim panel. Same methods and
//! thresholds as the ground segment (aerodrome height from the pressure
//! altitude on the ground, the inertial VS filter, the parked RA bias, the
//! CICTT phase boundaries) but without look-ahead: a phase that the ground
//! segment decides from what follows (the top of a climb, a go-around, a
//! bounce, the lowest speed of a touch-and-go) is decided here once it has
//! happened, so the debrief can differ by a few seconds.

use fdm_core::geo::distance_m;
use fdm_core::{Airport, SimTime};

use crate::phase::{STANDING_MAX_GS_KT, TAKEOFF_END_FT};
use crate::track::{BOUNCE_MAX_US, RA_BIAS_MAX_GS_KT, RA_MAX_FT, closer};
use crate::vs::Inertial;
use crate::{Phase, PhaseConfig};

/// A rise in ground speed this far above the landing roll's lowest takes
/// it for takeoff power: a touch-and-go, past the noise of a slowing roll.
const TOUCH_AND_GO_RISE_KT: f64 = 5.0;

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
    /// Latitude and longitude, deg.
    pub pos: Option<(f64, f64)>,
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
    /// Lowest height of an approach, highest of a climb, go-around or
    /// cruise.
    extreme_ft: f64,
    /// Lowest ground speed since the last touchdown.
    roll_min_kt: f64,
    /// Centres of the runways of the airports recorded so far, by airport.
    runways: Vec<(String, Vec<(f64, f64)>)>,
    /// Distances to those runways (m) where the approach began and at its
    /// lowest point: an approach closes on a runway (`Track::closing`).
    entry_m: Vec<f64>,
    bottom_m: Vec<f64>,
    /// Highest point before the approach, and the highest since its lowest.
    entry_top_ft: f64,
    climb_high_ft: f64,
    /// Where the go-around's climb ends it: the approach height, or for one
    /// that rose enough only above it, another rise further.
    go_around_end_ft: f64,
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
            roll_min_kt: f64::NAN,
            runways: Vec::new(),
            entry_m: Vec::new(),
            bottom_m: Vec::new(),
            entry_top_ft: f64::NAN,
            climb_high_ft: f64::NAN,
            go_around_end_ft: f64::NAN,
        }
    }
}

impl LiveTrack {
    /// The latest point, if a frame was stepped.
    pub fn last(&self) -> Option<LivePoint> {
        self.last
    }

    /// A recorded airport: its runways count for the approaches that follow.
    pub fn airport(&mut self, airport: &Airport) {
        let centres = (airport.runways.iter())
            .map(|r| (r.lat_deg, r.lon_deg))
            .collect();
        self.runways.retain(|(icao, _)| *icao != airport.icao);
        self.runways.push((airport.icao.clone(), centres));
    }

    /// Follows frame `x` with the boundaries of `cfg`, which may change
    /// between frames as the aircraft's identity or weight arrives.
    pub fn step(&mut self, x: &LiveInput, cfg: &PhaseConfig) -> LivePoint {
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
        let phase = self.phase(x, cfg, was, on_ground, aal_ft, ra_ft);
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
        cfg: &PhaseConfig,
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
                self.roll_min_kt = gs;
                p = Landing;
            }
            self.roll_min_kt = self.roll_min_kt.min(gs);
            return match p {
                // Power applied on the rollout: a touch-and-go.
                Landing if gs >= cfg.roll_kt.max(self.roll_min_kt + TOUCH_AND_GO_RISE_KT) => {
                    Takeoff
                }
                // The rollout, or a dropout in ground speed.
                Landing if gs >= cfg.roll_kt || gs.is_nan() => Landing,
                _ if gs.is_nan() => p,
                _ if gs >= cfg.roll_kt => Takeoff,
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
        let (rise, drop) = (cfg.go_around_rise_ft, cfg.go_around_drop_ft);
        let away: Vec<f64> = match x.pos {
            Some(pos) => (self.runways.iter().flat_map(|(_, c)| c))
                .map(|&c| distance_m(pos, c))
                .collect(),
            None => Vec::new(),
        };
        if matches!(p, Approach | Landing) {
            if aal < self.extreme_ft {
                (self.bottom_m, self.climb_high_ft) = (away.clone(), aal);
            }
            self.climb_high_ft = self.climb_high_ft.max(aal);
        }
        let closing = closer(&self.entry_m, &self.bottom_m);
        // A go-around climb may end above the approach height (docs/phases.md).
        let deep = low <= self.entry_top_ft - rise && low < cfg.approach_ft - drop;
        let next = match p {
            Takeoff if height >= TAKEOFF_END_FT => InitialClimb,
            InitialClimb if aal >= cfg.climb_end_ft => EnRoute,
            GoAround if aal >= self.go_around_end_ft => EnRoute,
            GoAround if aal >= cfg.approach_ft && aal <= high - drop => EnRoute,
            // Entered from above only: a climb through it stays en route.
            EnRoute if aal < cfg.approach_ft && high >= cfg.approach_ft => Approach,
            // Past the top of a go-around under the approach height, or of a
            // climb or cruise under it once below the climb end: a dip in a
            // low cruise between the two is not an approach (docs/phases.md).
            InitialClimb | GoAround | EnRoute
                if aal <= high - cfg.go_around_drop_ft
                    && aal
                        < if p == GoAround {
                            cfg.approach_ft
                        } else {
                            cfg.climb_end_ft
                        } =>
            {
                Approach
            }
            // A dip and climb away from every runway is no approach.
            Approach | Landing if aal >= low + rise => match closing {
                true => GoAround,
                false => EnRoute,
            },
            Approach
                if aal >= cfg.approach_ft
                    && !(deep && closing && aal > self.climb_high_ft - drop) =>
            {
                EnRoute
            }
            Approach if height < cfg.flare_ft => Landing,
            // Airborne with no liftoff seen: the flight started in the air.
            Standing | Taxi => EnRoute,
            _ => p,
        };
        if next == Approach && p != Approach {
            (self.entry_m, self.bottom_m) = (away.clone(), away);
            (self.entry_top_ft, self.climb_high_ft) = (high, aal);
        }
        if next == GoAround && p != GoAround {
            let above = aal >= cfg.approach_ft;
            self.go_around_end_ft = if above { aal + rise } else { cfg.approach_ft };
        }
        self.extreme_ft = match next {
            _ if next != p => aal,
            Approach | Landing => low,
            InitialClimb | GoAround | EnRoute => high,
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
        run_with(frames, &PhaseConfig::TRANSPORT)
    }

    fn run_with(frames: &[(bool, f64, f64)], cfg: &PhaseConfig) -> Vec<LivePoint> {
        let mut track = LiveTrack::default();
        frames
            .iter()
            .enumerate()
            .map(|(i, &(g, alt, gs))| {
                track.step(
                    &LiveInput {
                        t: SimTime::from_micros(i as i64 * 125_000),
                        on_ground: Some(g),
                        alt_ft: alt,
                        gs_kt: gs,
                        ra_ft: f64::NAN,
                        nz_g: 1.0,
                        pitch_deg: 0.0,
                        roll_deg: 0.0,
                        pos: None,
                    },
                    cfg,
                )
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
    fn a_climb_through_a_higher_approach_height_stays_en_route() {
        use Phase::*;
        let cfg = PhaseConfig {
            approach_ft: 1500.0,
            ..PhaseConfig::LIGHT
        };
        let points = run_with(&circuit(3000.0), &cfg);
        let seq = [
            Standing,
            Taxi,
            Takeoff,
            InitialClimb,
            EnRoute,
            Approach,
            Landing,
            Taxi,
            Standing,
        ];
        assert_eq!(phases(&points), seq);
        // Through 1275 ft on the climb.
        assert_eq!(points[12 + 16].phase, EnRoute);
        // A pattern topping between 1000 and 1500 ft approaches through 1000 ft.
        let points = run_with(&circuit(1200.0), &cfg);
        assert_eq!(phases(&points), seq);
        let first = points.iter().position(|p| p.phase == Approach).unwrap();
        assert!(points[first].aal_ft < 1000.0, "{}", points[first].aal_ft);
    }

    #[test]
    fn a_dip_in_a_low_cruise_under_a_higher_approach_height_is_not_an_approach() {
        use Phase::*;
        let cfg = PhaseConfig {
            approach_ft: 1500.0,
            ..PhaseConfig::LIGHT
        };
        // Up to 1200 ft, level, a 60 ft dip, on up to 1400 ft (260 ft above
        // the dip), level at 1300 ft, then down and land.
        let mut f = circuit(1200.0)[..12 + 40].to_vec();
        f.extend([(false, 1700.0, 90.0); 16]);
        f.extend([(false, 1640.0, 90.0); 8]);
        f.extend((1..=20).map(|i| (false, 1640.0 + 13.0 * f64::from(i), 90.0)));
        f.extend([(false, 1800.0, 90.0); 16]);
        f.extend(circuit(1300.0)[12 + 40..].iter().copied());
        let points = run_with(&f, &cfg);
        let seq = [
            Standing,
            Taxi,
            Takeoff,
            InitialClimb,
            EnRoute,
            Approach,
            Landing,
            Taxi,
            Standing,
        ];
        assert_eq!(phases(&points), seq);
        let first = points.iter().position(|p| p.phase == Approach).unwrap();
        assert!(points[first].aal_ft < 1000.0, "{}", points[first].aal_ft);
    }

    #[test]
    fn a_touch_and_go_takes_off_once_the_speed_rises() {
        use Phase::*;
        let mut f = circuit(800.0)[..12 + 80].to_vec();
        f.extend((0..16).map(|i| (true, 500.0, 50.0 - f64::from(i))));
        f.extend((1..=16).map(|i| (true, 500.0, 35.0 + 2.0 * f64::from(i))));
        f.extend((1..=8).map(|i| (false, 500.0 + 10.0 * f64::from(i), 70.0)));
        let points = run(&f);
        assert_eq!(
            phases(&points)[4..],
            [Approach, Landing, Takeoff, InitialClimb]
        );
        // The first frame 5 kt above the lowest (35 kt).
        let go = (12 + 80..f.len()).find(|&i| points[i].phase == Takeoff);
        assert_eq!(go.map(|i| f[i].2), Some(41.0));
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

    /// A climb from an aerodrome at 500 ft to 1300 ft above it, down to
    /// `bottom` ft and back up to 1300 ft, at `pos(i)` for frame `i`, with
    /// one runway at (0, 0).
    fn high_go_around(bottom: f64, pos: impl Fn(usize) -> (f64, f64)) -> Vec<Phase> {
        use fdm_core::{Designator, Runway, RunwayEnd};
        let mut f = vec![(true, 500.0, 0.0); 4];
        f.extend([(true, 500.0, 60.0); 4]);
        f.extend((1..=26).map(|i| (false, 500.0 + 50.0 * f64::from(i), 70.0)));
        let steps = ((1300.0 - bottom) / 25.0) as i32;
        f.extend((1..=steps).map(|i| (false, 1800.0 - 25.0 * f64::from(i), 70.0)));
        f.extend((1..=steps).map(|i| (false, 500.0 + bottom + 25.0 * f64::from(i), 70.0)));
        let end = |number| RunwayEnd {
            number,
            designator: Designator::None,
            displaced_m: 0.0,
        };
        let mut track = LiveTrack::default();
        track.airport(&Airport {
            icao: "XXXX".into(),
            region: String::new(),
            lat_deg: 0.0,
            lon_deg: 0.0,
            elevation_m: 0.0,
            magvar_deg: 0.0,
            runways: vec![Runway {
                lat_deg: 0.0,
                lon_deg: 0.0,
                elevation_m: 0.0,
                heading_deg: 0.0,
                length_m: 1000.0,
                width_m: 30.0,
                primary: end(36),
                secondary: end(18),
            }],
        });
        let points: Vec<_> = (f.iter().enumerate())
            .map(|(i, &(g, alt, gs))| {
                let x = LiveInput {
                    t: SimTime::from_micros(i as i64 * 125_000),
                    on_ground: Some(g),
                    alt_ft: alt,
                    gs_kt: gs,
                    ra_ft: f64::NAN,
                    nz_g: 1.0,
                    pitch_deg: 0.0,
                    roll_deg: 0.0,
                    pos: Some(pos(i)),
                };
                track.step(&x, &PhaseConfig::LIGHT)
            })
            .collect();
        phases(&points)
    }

    #[test]
    fn a_go_around_climbing_out_above_the_approach_height_is_one_live() {
        use Phase::*;
        let toward = |i: usize| (0.1 - 0.0005 * i as f64, 0.0);
        let climb = [Standing, Takeoff, InitialClimb, EnRoute, Approach];
        // From 805 ft: through 1000 ft before the climb of 200 ft.
        let ga = [&climb[..], &[GoAround, EnRoute]].concat();
        assert_eq!(high_go_around(805.0, toward), ga);
        // A dip to 960 ft is a levelling-off.
        let level = [&climb[..], &[EnRoute]].concat();
        assert_eq!(high_go_around(960.0, toward), level);
        // Away from the runway, a dip to 700 ft is no approach.
        let away = |i: usize| (0.1 + 0.0005 * i as f64, 0.0);
        assert_eq!(high_go_around(700.0, away), level);
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
