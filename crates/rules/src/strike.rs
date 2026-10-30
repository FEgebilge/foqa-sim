//! Ground strikes: a wing, the propeller or the nose on the runway after a
//! touchdown (a ground loop, a nose-over), told from the attitude because
//! MSFS 2024 tells SimConnect nothing of a crash (docs/challenges.md). One
//! measure for the live monitor (`capture`), the challenge check, the
//! landing assessment's cap and the `ground-strike` rule, so they never
//! disagree.

use analyzer::Track;
use fdm_core::SimTime;

use crate::rule::Aircraft;

/// How long the attitude must stay beyond the limits on the ground: a few
/// frames, so a single noisy sample or a gear-strut oscillation never
/// counts. FOQA-Sim estimate.
pub const STRIKE_SUSTAIN_S: f64 = 0.4;

/// Attitudes the aircraft cannot reach on the ground without something
/// other than its wheels touching. Strict: beyond, not at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrikeLimits {
    /// Bank either way, deg.
    pub roll_deg: f64,
    /// Nose-down pitch, deg (positive: pitch below `-nose_down_deg`).
    pub nose_down_deg: f64,
}

/// Light aircraft. Roll: with a wheel on each side a light aircraft rolls
/// no more than a few degrees on its gear; 12 deg means one main wheel is
/// off the ground and the aircraft is tipping onto a wingtip. Geometry of
/// the high-wing C172 (36 ft span, about 8 ft main-gear track, tips about
/// 7 ft up) puts tip contact well beyond it, a low wing's tip (PA-28) lower
/// but still past 12 deg: the value is the high-wing end of the 10-12 deg
/// band, so no wing-low crosswind touchdown is failed. FOQA-Sim estimate
/// from the airframes' three-view dimensions (POH Section 1).
/// Pitch: a nosewheel aircraft sits at about 0 to +2 deg on its gear;
/// below -5 deg the nose strut is bottomed or collapsed and the propeller
/// is on the runway (C172 propeller clearance about 11 in, POH Section 1).
/// A tailwheel type sits nose-up and strikes its propeller near level
/// pitch: the profiles cannot tell the gear layout, so the nosewheel value
/// (the one that never fails a normal rollout of either) is used for all.
/// FOQA-Sim estimate.
const LIGHT: StrikeLimits = StrikeLimits {
    roll_deg: 12.0,
    nose_down_deg: 5.0,
};

/// Transport aircraft. Roll: a 737-800's wingtip touched the runway at
/// 8.5 deg of bank with 9 deg nose-up (SKYbrary incident report,
/// https://skybrary.aero/sites/default/files/bookshelf/32520.pdf); its
/// engine nacelle strikes earlier at landing pitch (AAIB EW/C2010/05/10,
/// 737-8AS EI-DAL, nacelle contact in a crosswind landing,
/// https://assets.publishing.service.gov.uk/media/5422ec7de5274a131700011f/Boeing_737-8AS__EI-DAL_05-10.pdf).
/// 8 deg: past any transport's wheels-only roll. Pitch as for light types:
/// below -5 deg the nose gear has collapsed. FOQA-Sim estimate.
const TRANSPORT: StrikeLimits = StrikeLimits {
    roll_deg: 8.0,
    nose_down_deg: 5.0,
};

impl StrikeLimits {
    /// The class's limits; an unknown class gets the light ones, the wider
    /// roll limit, so an unknown aircraft is never failed by the stricter.
    pub fn of(class: Option<Aircraft>) -> Self {
        match class {
            Some(Aircraft::Transport) => TRANSPORT,
            Some(Aircraft::Light) | None => LIGHT,
        }
    }
}

/// A ground strike as measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strike {
    /// When it was told, s, in the caller's time base.
    pub t_s: f64,
    pub roll_deg: f64,
    pub pitch_deg: f64,
    /// Roll beyond the limit: a wingtip (or an engine nacelle) touched.
    pub wing: bool,
    /// Nose-down pitch beyond the limit: the propeller or the nose touched.
    pub nose: bool,
}

impl Strike {
    /// In plain words, for the attempt log and the reports.
    pub fn reason(&self) -> &'static str {
        match (self.wing, self.nose) {
            (true, true) => "wing and prop/nose strike",
            (true, false) => "wing strike",
            _ => "prop/nose strike",
        }
    }
}

/// Follows the attitude frame by frame: after a touchdown, an attitude
/// beyond the limits on the ground for `STRIKE_SUSTAIN_S`, or a touchdown
/// frame already banked beyond the roll limit, is a strike. One per ground
/// contact; flying again re-arms it. A flight that starts on the ground
/// is not watched until its first touchdown.
#[derive(Debug, Clone, Default)]
pub struct StrikeWatch {
    /// Seen airborne since the last ground frame: the next one is a touchdown.
    airborne: bool,
    /// On the ground after a touchdown.
    landed: bool,
    /// Since when the attitude is beyond the limits.
    beyond_since: Option<f64>,
    /// This ground contact has struck already.
    struck: bool,
}

impl StrikeWatch {
    /// One frame at `t_s`: air/ground (unknown is skipped), roll (either
    /// sign) and pitch (nose-up positive), deg.
    pub fn step(
        &mut self,
        limits: StrikeLimits,
        t_s: f64,
        on_ground: Option<bool>,
        roll_deg: f64,
        pitch_deg: f64,
    ) -> Option<Strike> {
        let on_ground = on_ground?;
        if !on_ground {
            *self = Self {
                airborne: true,
                ..Self::default()
            };
            return None;
        }
        let touchdown = std::mem::take(&mut self.airborne);
        self.landed |= touchdown;
        if !self.landed || self.struck {
            return None;
        }
        // NaN compares false: an unknown attitude is no strike.
        let wing = roll_deg.abs() > limits.roll_deg;
        let nose = pitch_deg < -limits.nose_down_deg;
        if !wing && !nose {
            self.beyond_since = None;
            return None;
        }
        let since = *self.beyond_since.get_or_insert(t_s);
        // A hair under: 0.4 s of frames at 10 Hz is not 0.39999.
        let struck = (touchdown && wing) || t_s - since >= STRIKE_SUSTAIN_S - 1e-6;
        self.struck = struck;
        struck.then_some(Strike {
            t_s,
            roll_deg,
            pitch_deg,
            wing,
            nose,
        })
    }
}

/// Every strike of an analysed flight, sim seconds.
pub fn strikes(track: &Track, limits: StrikeLimits) -> Vec<Strike> {
    let mut watch = StrikeWatch::default();
    (0..track.len())
        .filter_map(|i| {
            let t_s = track.t[i].as_secs_f64();
            let (roll, pitch) = (track.roll_deg[i], track.pitch_deg[i]);
            watch.step(limits, t_s, Some(track.on_ground[i]), roll, pitch)
        })
        .collect()
}

/// The strike of `strikes` (of `track`) on the ground after the touchdown
/// at `td`, before the next liftoff.
pub fn after_touchdown<'a>(
    strikes: &'a [Strike],
    track: &Track,
    td: SimTime,
) -> Option<&'a Strike> {
    let (n, from) = (track.len(), track.index_at(td));
    let landed = (from..n).find(|&i| track.on_ground[i]).unwrap_or(n);
    let until = (landed..n)
        .find(|&i| !track.on_ground[i])
        .map_or(f64::INFINITY, |i| track.t[i].as_secs_f64());
    let since = track.t.get(from).map_or(f64::INFINITY, |t| t.as_secs_f64());
    let since = since.min(td.as_secs_f64());
    strikes.iter().find(|s| (since..until).contains(&s.t_s))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steps `frames` (t, on ground, roll, pitch) through a light watch.
    fn watch(frames: &[(f64, bool, f64, f64)]) -> Vec<Strike> {
        let mut w = StrikeWatch::default();
        let limits = StrikeLimits::of(Some(Aircraft::Light));
        frames
            .iter()
            .filter_map(|&(t, g, r, p)| w.step(limits, t, Some(g), r, p))
            .collect()
    }

    /// Airborne to 1 s, then on the ground at 10 Hz with `attitude(t)`.
    fn landing(attitude: impl Fn(f64) -> (f64, f64)) -> Vec<Strike> {
        let frames: Vec<_> = (0..60)
            .map(|i| {
                let t = f64::from(i) / 10.0;
                let (r, p) = attitude(t);
                (t, t >= 1.0, r, p)
            })
            .collect();
        watch(&frames)
    }

    #[test]
    fn a_normal_landing_strikes_nothing() {
        assert!(landing(|t| (2.0 * (t * 3.0).sin(), 6.0 - t)).is_empty());
    }

    #[test]
    fn a_sustained_roll_on_the_ground_is_a_wing_strike() {
        let s = landing(|t| (if t >= 3.0 { -20.0 } else { 0.0 }, 1.0));
        assert_eq!(s.len(), 1, "{s:?}");
        assert!((s[0].t_s - 3.4).abs() < 1e-9);
        assert_eq!((s[0].wing, s[0].nose), (true, false));
        assert_eq!(s[0].reason(), "wing strike");
    }

    #[test]
    fn a_nose_over_is_a_prop_strike() {
        let s = landing(|t| (0.0, if t >= 2.0 { -15.0 } else { 2.0 }));
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].reason(), "prop/nose strike");
    }

    #[test]
    fn a_single_noisy_frame_is_no_strike() {
        let s = landing(|t| {
            let spike = (t - 3.0).abs() < 0.05 || (t - 4.0).abs() < 0.15;
            if spike { (30.0, -20.0) } else { (0.0, 1.0) }
        });
        assert!(s.is_empty(), "{s:?}");
    }

    #[test]
    fn a_touchdown_banked_beyond_the_limit_is_a_wing_strike_at_once() {
        let s = landing(|t| (if t < 1.05 { 15.0 } else { 0.0 }, 3.0));
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].t_s, s[0].wing), (1.0, true));
    }

    #[test]
    fn a_flight_starting_on_the_ground_is_watched_from_its_first_touchdown() {
        let frames: Vec<_> = (0..20)
            .map(|i| (f64::from(i) / 10.0, true, 30.0, -20.0))
            .collect();
        assert!(watch(&frames).is_empty());
    }

    #[test]
    fn transport_limits_are_tighter_in_roll() {
        let (light, transport) = (
            StrikeLimits::of(Some(Aircraft::Light)),
            StrikeLimits::of(Some(Aircraft::Transport)),
        );
        assert!(transport.roll_deg < light.roll_deg);
        assert_eq!(StrikeLimits::of(None), light);
    }
}
