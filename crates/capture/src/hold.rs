//! A placed start flown hands off: holds the challenge's speed with the
//! throttle and level flight with the pitch trim until the pilot takes each
//! one, or the gate, where scoring starts. A placement sets the aircraft's
//! speed and attitude, not the thrust and trim that keep them.

use std::time::Instant;

use fdm_core::geo::distance_m;

use crate::PLACE_SETTLE;

/// What the hold reads from one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoldFrame {
    pub t_s: f64,
    pub ias_kt: f64,
    pub vs_fpm: f64,
    /// Nose up positive.
    pub pitch_deg: f64,
    /// Either way; only its size counts.
    pub bank_deg: f64,
    /// Percent, 0..100.
    pub lever_pct: f64,
    /// Percent of travel, nose up positive, -100..100.
    pub trim_pct: f64,
    /// Percent of travel, -100..100.
    pub yoke_pct: f64,
}

/// What it commands after one frame; `None` leaves that control alone.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HoldOutput {
    pub lever_pct: Option<f64>,
    pub trim_pct: Option<f64>,
}

/// Lever percent per knot of speed error.
const LEVER_P: f64 = 3.0;
/// Lever percent per second per knot.
const LEVER_I: f64 = 0.35;
/// Lever percent per second at most: an engine's own lag is the rest.
const LEVER_RATE: f64 = 20.0;
/// Trim percent per second per 100 fpm off level.
const TRIM_P: f64 = 0.5;
/// Trim percent per second per degree per second of pitch: damps the
/// slow pitch and speed exchange that the sink alone would feed.
const TRIM_D: f64 = 3.0;
/// Trim percent per second at most, about a pilot's own trim wheel.
const TRIM_RATE: f64 = 6.0;
/// The lever off what was last commanded by more than this, percent, is the
/// pilot's hand: the frames answer the command within one or two.
const LEVER_TOUCH: f64 = 3.0;
/// The trim off what was last commanded by more than this, percent.
const TRIM_TOUCH: f64 = 2.0;
/// The yoke off centre by more than this, percent.
const YOKE_TOUCH: f64 = 6.0;
/// Banked more than this, degrees, nobody is keeping the wings level and the
/// aircraft is spiralling: trim up would only tighten it (bench, 2026-11-02:
/// a C172 left alone sank 1800 fpm with the trim run up to 82%).
const BANK_LIMIT: f64 = 15.0;

/// Holds one start's speed until the pilot takes over.
#[derive(Debug, Clone)]
pub struct Hold {
    ias_kt: f64,
    lever: Option<Axis>,
    trim: Option<Axis>,
    integral: f64,
    last: Option<HoldFrame>,
}

/// One control still held: what was last commanded.
#[derive(Debug, Clone, Copy)]
struct Axis {
    commanded: f64,
}

impl Hold {
    pub fn new(ias_kt: f64) -> Self {
        Self {
            ias_kt,
            lever: None,
            trim: None,
            integral: 0.0,
            last: None,
        }
    }

    /// Follows one frame.
    pub fn step(&mut self, f: HoldFrame) -> HoldOutput {
        let Some(last) = self.last.replace(f) else {
            // The first frame: the controls as the placement left them.
            self.integral = f.lever_pct;
            self.lever = Some(Axis {
                commanded: f.lever_pct,
            });
            self.trim = Some(Axis {
                commanded: f.trim_pct,
            });
            return HoldOutput::default();
        };
        let dt = f.t_s - last.t_s;
        if !(dt > 0.0 && dt < 1.0) {
            return HoldOutput::default();
        }
        if self
            .lever
            .is_some_and(|a| (f.lever_pct - a.commanded).abs() > LEVER_TOUCH)
        {
            self.lever = None;
        }
        if f.yoke_pct.abs() > YOKE_TOUCH
            || f.bank_deg.abs() > BANK_LIMIT
            || self
                .trim
                .is_some_and(|a| (f.trim_pct - a.commanded).abs() > TRIM_TOUCH)
        {
            self.trim = None;
        }
        HoldOutput {
            lever_pct: self.lever.is_some().then(|| self.lever_step(&f, dt)),
            trim_pct: self.trim.is_some().then(|| self.trim_step(&f, &last, dt)),
        }
    }

    fn lever_step(&mut self, f: &HoldFrame, dt: f64) -> f64 {
        let error = self.ias_kt - f.ias_kt;
        let wanted = self.integral + LEVER_P * error;
        // No wind-up against the lever's stops.
        if (0.0..=100.0).contains(&wanted) {
            self.integral = (self.integral + LEVER_I * error * dt).clamp(0.0, 100.0);
        }
        let axis = self.lever.as_mut().expect("held");
        let step =
            (wanted.clamp(0.0, 100.0) - axis.commanded).clamp(-LEVER_RATE * dt, LEVER_RATE * dt);
        axis.commanded += step;
        axis.commanded
    }

    fn trim_step(&mut self, f: &HoldFrame, last: &HoldFrame, dt: f64) -> f64 {
        let pitch_rate = (f.pitch_deg - last.pitch_deg) / dt;
        let rate = -(TRIM_P * f.vs_fpm / 100.0 + TRIM_D * pitch_rate);
        let axis = self.trim.as_mut().expect("held");
        axis.commanded += rate.clamp(-TRIM_RATE, TRIM_RATE) * dt;
        axis.commanded = axis.commanded.clamp(-100.0, 100.0);
        axis.commanded
    }
}

/// Where a start's hold ends for good: the gate, on the way to the threshold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoldUntil {
    pub ias_kt: f64,
    pub threshold_lat_deg: f64,
    pub threshold_lon_deg: f64,
    pub gate_m: f64,
}

/// The SimVars a hold reads, in `Holding::follow`'s order.
pub const HOLD_VARS: [(&str, &str); 11] = [
    ("SIMULATION TIME", "seconds"),
    ("PLANE LATITUDE", "degrees"),
    ("PLANE LONGITUDE", "degrees"),
    ("AIRSPEED INDICATED", "knots"),
    ("VERTICAL SPEED", "feet per minute"),
    ("PLANE PITCH DEGREES", "degrees"),
    ("PLANE BANK DEGREES", "degrees"),
    ("GENERAL ENG THROTTLE LEVER POSITION:1", "percent"),
    ("ELEVATOR TRIM PCT", "percent"),
    ("YOKE Y POSITION", "percent"),
    ("SIM ON GROUND", "bool"),
];

/// A hold under way, fed the sim's `HOLD_VARS`.
#[derive(Debug, Clone)]
pub struct Holding {
    hold: Hold,
    until: HoldUntil,
    placed: Instant,
}

impl Holding {
    pub fn new(until: HoldUntil, placed: Instant) -> Self {
        Self {
            hold: Hold::new(until.ias_kt),
            until,
            placed,
        }
    }

    /// What to command after these values at `now`, or `None` once the hold
    /// is over: past the gate or on the ground.
    pub fn follow(&mut self, v: &[f64], now: Instant) -> Option<HoldOutput> {
        let &[
            t_s,
            lat,
            lon,
            ias_kt,
            vs_fpm,
            pitch,
            bank_deg,
            lever_pct,
            trim_pct,
            yoke_pct,
            ground,
        ] = v
        else {
            return None;
        };
        // The frames of the old position drain first (`PLACE_SETTLE`).
        if now - self.placed < PLACE_SETTLE {
            return Some(HoldOutput::default());
        }
        let u = self.until;
        let to_threshold = distance_m((lat, lon), (u.threshold_lat_deg, u.threshold_lon_deg));
        if ground >= 0.5 || to_threshold <= u.gate_m {
            return None;
        }
        Some(self.hold.step(HoldFrame {
            t_s,
            ias_kt,
            vs_fpm,
            pitch_deg: -pitch,
            bank_deg,
            lever_pct,
            trim_pct,
            yoke_pct,
        }))
    }
}

/// `THROTTLE_SET` data (0 idle, 16383 full) for a lever percent.
pub fn throttle_set_data(lever_pct: f64) -> u32 {
    (lever_pct.clamp(0.0, 100.0) / 100.0 * 16383.0).round() as u32
}

/// `ELEVATOR_TRIM_SET` data (-16383 nose down to 16383 nose up) for a trim
/// percent, as the event's unsigned data.
pub fn trim_set_data(trim_pct: f64) -> u32 {
    (trim_pct.clamp(-100.0, 100.0) / 100.0 * 16383.0).round() as i32 as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(t_s: f64) -> HoldFrame {
        HoldFrame {
            t_s,
            ias_kt: 100.0,
            vs_fpm: 0.0,
            pitch_deg: 2.0,
            bank_deg: 0.0,
            lever_pct: 60.0,
            trim_pct: 10.0,
            yoke_pct: 0.0,
        }
    }

    /// A held hold after two frames at the start values.
    fn holding() -> Hold {
        let mut h = Hold::new(100.0);
        h.step(frame(0.0));
        h
    }

    #[test]
    fn slow_opens_the_throttle_and_sinking_trims_up() {
        let mut h = holding();
        let out = h.step(HoldFrame {
            ias_kt: 95.0,
            vs_fpm: -500.0,
            ..frame(0.1)
        });
        assert!(out.lever_pct.unwrap() > 60.0);
        assert!(out.trim_pct.unwrap() > 10.0);
    }

    #[test]
    fn the_pilots_throttle_ends_the_throttle_alone() {
        let mut h = holding();
        let out = h.step(HoldFrame {
            lever_pct: 40.0,
            ..frame(0.1)
        });
        assert_eq!(out.lever_pct, None);
        assert!(out.trim_pct.is_some());
        // For good, even back where it was.
        assert_eq!(h.step(frame(0.2)).lever_pct, None);
    }

    #[test]
    fn the_pilots_yoke_or_trim_ends_the_trim_alone() {
        for touched in [
            HoldFrame {
                yoke_pct: -20.0,
                ..frame(0.1)
            },
            HoldFrame {
                trim_pct: 20.0,
                ..frame(0.1)
            },
            HoldFrame {
                bank_deg: -20.0,
                ..frame(0.1)
            },
        ] {
            let mut h = holding();
            let out = h.step(touched);
            assert_eq!(out.trim_pct, None);
            assert!(out.lever_pct.is_some());
        }
    }

    #[test]
    fn a_holding_ends_at_the_gate_or_on_the_ground() {
        let placed = Instant::now();
        let now = placed + PLACE_SETTLE;
        let until = HoldUntil {
            ias_kt: 100.0,
            threshold_lat_deg: 0.0,
            threshold_lon_deg: 0.0,
            gate_m: 5000.0,
        };
        // lat 0.1 is about 11 km out.
        let values = |lat: f64, ground: f64| {
            [
                1.0, lat, 0.0, 100.0, 0.0, -2.0, 0.0, 60.0, 10.0, 0.0, ground,
            ]
        };
        let mut h = Holding::new(until, placed);
        assert!(h.follow(&values(0.1, 0.0), now).is_some());
        assert_eq!(h.follow(&values(0.01, 0.0), now), None, "past the gate");
        assert_eq!(h.follow(&values(0.1, 1.0), now), None, "on the ground");
        assert_eq!(h.follow(&[1.0], now), None, "not its values");
        // Just placed: the old position's frames are not followed.
        let mut h = Holding::new(until, placed);
        assert_eq!(
            h.follow(&values(0.01, 1.0), placed),
            Some(HoldOutput::default())
        );
    }

    #[test]
    fn a_jump_in_sim_time_commands_nothing() {
        let mut h = holding();
        assert_eq!(h.step(frame(5.0)), HoldOutput::default());
    }

    #[test]
    fn commands_stay_within_their_rates_and_stops() {
        let mut h = holding();
        let out = h.step(HoldFrame {
            ias_kt: 50.0,
            vs_fpm: -3000.0,
            pitch_deg: -30.0,
            ..frame(0.1)
        });
        assert!(out.lever_pct.unwrap() <= 60.0 + LEVER_RATE * 0.1 + 1e-9);
        assert!(out.trim_pct.unwrap() <= 10.0 + TRIM_RATE * 0.1 + 1e-9);
        assert_eq!(throttle_set_data(150.0), 16383);
        assert_eq!(trim_set_data(-100.0), (-16383i32) as u32);
    }
}
