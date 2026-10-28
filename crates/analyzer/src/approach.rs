//! The values a landing assessment grades beyond the touchdown's own
//! (docs/scoring.md §2.5): the approach window and its series, the
//! threshold crossing, the float, crab and drift at contact, configuration
//! changes in the window, the deceleration after contact and where the
//! aircraft slowed to taxi speed. The bands that grade them are not here.

use std::ops::Range;

use fdm_core::SimTime;
use fdm_core::geo::distance_m;

use crate::landing::{BOUNCE_AIRBORNE_MAX_US, position};
use crate::phase::runway_height;
use crate::runway::{RunwayFix, ground_track_deg, wrap180};
use crate::{Flight, Touchdown, Track};

const KT_MS: f64 = 1852.0 / 3600.0;
const G_MS2: f64 = 9.806_65;
/// The float is timed from this height above the runway to contact (EOFDM
/// RE28, long flare).
const FLOAT_FROM_FT: f64 = 50.0;
/// Ground track for the drift at contact is taken over this last stretch.
const DRIFT_SPAN_US: i64 = 1_000_000;
/// Less ground covered over the drift span gives no track, m.
const DRIFT_MIN_M: f64 = 1.0;
/// Smallest movement that is a configuration change: Data4Safety's late
/// flap (2 deg); any gear travel past 1%.
const FLAP_CHANGE_DEG: f64 = 2.0;
const GEAR_CHANGE_PCT: f64 = 1.0;
/// A deceleration means is in use from: ground spoilers past 50%, brakes
/// past 30% (docs/scoring.md F1); a reverser once unlocked.
const SPOILERS_PCT: f64 = 50.0;
const BRAKES_PCT: f64 = 30.0;
const REVERSERS: [&str; 4] = [
    "eng1.reverser",
    "eng2.reverser",
    "eng3.reverser",
    "eng4.reverser",
];
/// The first deceleration means is looked for this long after contact.
const DECEL_SEARCH_US: i64 = 30_000_000;
/// The rollout's centreline tracking ends below this ground speed (RE.7).
const ROLLOUT_END_KT: f64 = 30.0;
/// Taxi speed on a dry runway (FAA-S-ACS-11A): stopped on the runway.
const TAXI_KT: f64 = 20.0;

/// The approach before one touchdown, from `top_ft` down to `bottom_ft`
/// above the runway (RA, else AAL), as series on the track's time base.
/// `NaN` where a value is unknown (no ILS signal, a dropout).
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    /// Track indices.
    pub range: Range<usize>,
    pub t: Vec<SimTime>,
    pub height_ft: Vec<f64>,
    pub ias_kt: Vec<f64>,
    pub gs_kt: Vec<f64>,
    /// Rate of descent, positive down, fpm.
    pub sink_fpm: Vec<f64>,
    pub roll_deg: Vec<f64>,
    pub pitch_deg: Vec<f64>,
    pub gs_dev_dot: Vec<f64>,
    pub loc_dev_dot: Vec<f64>,
    pub flap_deg: Vec<f64>,
    pub gear_pct: Vec<f64>,
}

/// The height from which every point stayed `ok` down to the end of a
/// series (the stable height of a window's `height_ft`): the first point
/// after the last one that is not. `None` if the last one is not, or the
/// series is empty.
pub fn stable_from(height_ft: &[f64], ok: &[bool]) -> Option<f64> {
    let first = ok.iter().rposition(|&o| !o).map_or(0, |i| i + 1);
    height_ft.get(first).copied()
}

/// What moved: the trailing-edge flaps (deg) or the landing gear (%).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Config {
    Flaps,
    Gear,
}

impl Config {
    /// Its parameter and the smallest movement that counts.
    fn parameter(self) -> (&'static str, f64) {
        match self {
            Self::Flaps => ("flap.te_angle", FLAP_CHANGE_DEG),
            Self::Gear => ("gear.extended", GEAR_CHANGE_PCT),
        }
    }
}

/// A configuration change in the approach window.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigChange {
    pub what: Config,
    /// The first recorded point of the movement.
    pub t: SimTime,
    pub height_ft: f64,
    pub from: f64,
    pub to: f64,
}

/// Where the approach crossed the landing threshold.
#[derive(Debug, Clone, PartialEq)]
pub struct Crossing {
    pub t: SimTime,
    /// Above the runway (RA, else AAL).
    pub height_ft: f64,
    pub ias_kt: f64,
    pub gs_kt: f64,
}

/// Where the rollout first reached taxi speed.
#[derive(Debug, Clone, PartialEq)]
pub struct Slowed {
    pub t: SimTime,
    /// From the landing threshold, as `RunwayFix`.
    pub along_m: f64,
    pub cross_m: f64,
}

/// The values of one landing. The runway ones are `None` without a
/// runway fix.
#[derive(Debug, Clone, PartialEq)]
pub struct Approach {
    pub window: Window,
    pub config_changes: Vec<ConfigChange>,
    pub threshold: Option<Crossing>,
    /// From 50 ft above the runway to contact, s.
    pub float_s: Option<f64>,
    /// True heading less the runway heading at contact, nose right
    /// positive, deg.
    pub crab_deg: Option<f64>,
    /// Ground speed across the runway over the last second, rightward
    /// positive, kt.
    pub drift_kt: Option<f64>,
    /// From contact to the first deceleration means in use (reverser,
    /// ground spoilers, brakes), s; known to the 1-2 Hz recording rate.
    pub decel_delay_s: Option<f64>,
    /// Constant deceleration that stops in the remaining landing distance
    /// from the ground speed at contact, GS² / 2d, g (EAFDM RE.8); `None`
    /// with no distance left.
    pub required_decel_g: Option<f64>,
    /// Where the aircraft next left the ground for longer than a bounce:
    /// a touch-and-go, or the recording ending airborne; `None` if it ends
    /// on the ground. The rollout ends there.
    pub liftoff: Option<SimTime>,
    /// Largest centreline offset from contact down to 30 kt or the
    /// liftoff, m.
    pub rollout_cross_m: Option<f64>,
    /// Taxi speed reached before the liftoff.
    pub slowed: Option<Slowed>,
}

impl Approach {
    /// The values of the landing at `td`, its window from `top_ft` to
    /// `bottom_ft` above the runway.
    pub fn of(flight: &Flight, track: &Track, td: &Touchdown, top_ft: f64, bottom_ft: f64) -> Self {
        let fix = td.runway.as_ref();
        let window = Window::of(flight, track, td.t, top_ft, bottom_ft);
        let mut config_changes: Vec<_> = window
            .t
            .first()
            .zip(window.t.last())
            .into_iter()
            .flat_map(|(&from, &to)| {
                [Config::Flaps, Config::Gear]
                    .into_iter()
                    .flat_map(move |what| changes(flight, track, what, from, to))
            })
            .collect();
        config_changes.sort_by_key(|c| c.t);
        let gs = flight.at("groundspeed", td.t);
        let td_i = track.index_at(td.t);
        let liftoff = liftoff(track, td_i);
        // The rollout also ends where the sim next moves the aircraft.
        let moved = (flight.breaks().into_iter())
            .filter(|b| b.start > td.t)
            .map(|b| track.t.partition_point(|&x| x < b.start))
            .min();
        let ground_end = liftoff
            .unwrap_or(track.len())
            .min(moved.unwrap_or(track.len()));
        Self {
            config_changes,
            threshold: fix.and_then(|f| crossing(flight, track, f, td.t)),
            float_s: float_s(flight, track, td.t),
            crab_deg: fix.and_then(|f| {
                let magnetic = flight.at("heading.mag", td.t)?;
                Some(wrap180(magnetic - f.magvar_deg - f.heading_deg))
            }),
            drift_kt: fix.zip(gs).and_then(|(f, gs)| {
                let a = position(flight, td.t.add_us(-DRIFT_SPAN_US))?;
                let b = position(flight, td.t)?;
                (distance_m(a, b) >= DRIFT_MIN_M)
                    .then(|| gs * (ground_track_deg(a, b) - f.heading_deg).to_radians().sin())
            }),
            decel_delay_s: decel_delay_s(flight, td.t),
            required_decel_g: fix.zip(gs).and_then(|(f, gs)| {
                let left_m = f.lda_m - f.along_m;
                (left_m > 0.0).then(|| (gs * KT_MS).powi(2) / (2.0 * left_m) / G_MS2)
            }),
            rollout_cross_m: fix.and_then(|f| {
                let end = (td_i..ground_end)
                    .find(|&i| track.gs_kt[i] < ROLLOUT_END_KT)
                    .unwrap_or(ground_end);
                (td_i..end)
                    .filter_map(|i| position(flight, track.t[i]))
                    .map(|(lat, lon)| f.offsets(lat, lon).1.abs())
                    .reduce(f64::max)
            }),
            slowed: fix.and_then(|f| {
                let i = (td_i..ground_end).find(|&i| track.gs_kt[i] <= TAXI_KT)?;
                let (along_m, cross_m) =
                    position(flight, track.t[i]).map(|(a, b)| f.offsets(a, b))?;
                Some(Slowed {
                    t: track.t[i],
                    along_m,
                    cross_m,
                })
            }),
            liftoff: liftoff.map(|i| track.t[i]),
            window,
        }
    }
}

fn height(track: &Track, i: usize) -> f64 {
    runway_height(track, i).0
}

/// The airborne points of the leg that ends at the contact `t`, up to the
/// last one before it, from the end of the last `Flight::breaks` span in
/// it (an approach slewed or teleported into is graded from there).
fn before_contact(flight: &Flight, track: &Track, t: SimTime) -> Range<usize> {
    let last = track.t.partition_point(|&x| x < t);
    let start = track
        .legs
        .iter()
        .find(|l| l.airborne.contains(&last.saturating_sub(1)))
        .map_or(0, |l| l.airborne.start);
    let resumed = (flight.breaks().into_iter())
        .filter(|b| b.end <= t)
        .map(|b| track.t.partition_point(|&x| x < b.end))
        .max();
    start.max(resumed.unwrap_or(0))..last
}

/// The first point from `from` that leaves the ground for longer than a
/// bounce (`landing.rs`); `None` if the track ends on the ground.
fn liftoff(track: &Track, from: usize) -> Option<usize> {
    let mut i = from;
    loop {
        let off = (i..track.len()).find(|&k| !track.on_ground[k])?;
        match (off..track.len()).find(|&k| track.on_ground[k]) {
            Some(on) if track.t[on].us_since(track.t[off]) <= BOUNCE_AIRBORNE_MAX_US => i = on,
            _ => return Some(off),
        }
    }
}

impl Window {
    /// The window before the contact at `td`: from just below the last point
    /// above `top_ft` to the first point under `bottom_ft`.
    pub fn of(flight: &Flight, track: &Track, td: SimTime, top_ft: f64, bottom_ft: f64) -> Self {
        let leg = before_contact(flight, track, td);
        let start = leg
            .clone()
            .rev()
            .find(|&i| height(track, i) > top_ft)
            .map_or(leg.start, |i| i + 1);
        let end = (start..leg.end)
            .find(|&i| height(track, i) < bottom_ft)
            .unwrap_or(leg.end);
        let range = start..end;
        let of = |v: &[f64]| v[range.clone()].to_vec();
        let at = |name| {
            range
                .clone()
                .map(|i| flight.at(name, track.t[i]).unwrap_or(f64::NAN))
                .collect()
        };
        Self {
            t: track.t[range.clone()].to_vec(),
            height_ft: range.clone().map(|i| height(track, i)).collect(),
            ias_kt: of(&track.ias_kt),
            gs_kt: of(&track.gs_kt),
            sink_fpm: track.vs_fpm[range.clone()].iter().map(|v| -v).collect(),
            roll_deg: of(&track.roll_deg),
            pitch_deg: of(&track.pitch_deg),
            gs_dev_dot: at("nav.gs_dev"),
            loc_dev_dot: at("nav.loc_dev"),
            flap_deg: at("flap.te_angle"),
            gear_pct: at("gear.extended"),
            range,
        }
    }
}

/// Movements of `what` between `from` and `to` larger than its minimum,
/// timed at their first moved sample. A movement lasts until two samples
/// in a row agree.
fn changes(
    flight: &Flight,
    track: &Track,
    what: Config,
    from: SimTime,
    to: SimTime,
) -> Vec<ConfigChange> {
    let (name, min) = what.parameter();
    let Some(mut settled) = flight.at(name, from) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let (mut last, mut moving) = (settled, None);
    let samples = flight.series(name).iter();
    let inside = samples.filter(|s| s.is_valid() && s.t > from && s.t <= to);
    for s in inside.map(|s| (s.t, s.value)).chain([(to, f64::NAN)]) {
        if s.1 != last && !s.1.is_nan() {
            moving.get_or_insert(s.0);
        } else if let Some(t) = moving.take() {
            if (last - settled).abs() > min {
                out.push(ConfigChange {
                    what,
                    t,
                    height_ft: height(track, track.index_at(t)),
                    from: settled,
                    to: last,
                });
            }
            settled = last;
        }
        last = if s.1.is_nan() { last } else { s.1 };
    }
    out
}

/// The last crossing of the threshold before contact (along 0 m).
fn crossing(flight: &Flight, track: &Track, fix: &RunwayFix, td: SimTime) -> Option<Crossing> {
    let along = |i: usize| position(flight, track.t[i]).map(|(lat, lon)| fix.offsets(lat, lon).0);
    let leg = before_contact(flight, track, td);
    let mut next: Option<(usize, f64)> = None;
    for i in leg.rev() {
        let Some(a) = along(i) else {
            next = None;
            continue;
        };
        if a < 0.0 {
            let (j, b) = next?;
            let f = -a / (b - a);
            let mix = |v: &dyn Fn(usize) -> f64| v(i) + f * (v(j) - v(i));
            let dt = track.t[j].us_since(track.t[i]) as f64;
            return Some(Crossing {
                t: track.t[i].add_us((f * dt).round() as i64),
                height_ft: mix(&|k| height(track, k)),
                ias_kt: mix(&|k| track.ias_kt[k]),
                gs_kt: mix(&|k| track.gs_kt[k]),
            });
        }
        next = Some((i, a));
    }
    None
}

/// Time from the last descent through 50 ft above the runway to contact.
fn float_s(flight: &Flight, track: &Track, td: SimTime) -> Option<f64> {
    let leg = before_contact(flight, track, td);
    let i = leg
        .clone()
        .rev()
        .find(|&i| height(track, i) >= FLOAT_FROM_FT)?;
    // The next point, or the contact itself at 0 ft.
    let (h1, t1) = Some(i + 1)
        .filter(|&j| j < leg.end && height(track, j).is_finite())
        .map_or((0.0, td), |j| (height(track, j), track.t[j]));
    let h0 = height(track, i);
    let t50 = (h0 - FLOAT_FROM_FT) / (h0 - h1) * t1.us_since(track.t[i]) as f64;
    Some((td.us_since(track.t[i]) as f64 - t50) / 1e6)
}

/// Time from contact to the first deceleration means in use.
fn decel_delay_s(flight: &Flight, td: SimTime) -> Option<f64> {
    let used = |name: &str, past: f64| {
        flight
            .series(name)
            .iter()
            .filter(|s| s.is_valid() && (0..=DECEL_SEARCH_US).contains(&s.t.us_since(td)))
            .find(|s| s.value > past)
            .map(|s| s.t)
    };
    let means = REVERSERS.iter().map(|r| (*r, 0.5)).chain([
        ("spoiler.handle", SPOILERS_PCT),
        ("brake.left", BRAKES_PCT),
        ("brake.right", BRAKES_PCT),
    ]);
    means
        .filter_map(|(name, past)| used(name, past))
        .min()
        .map(|t| t.us_since(td) as f64 / 1e6)
}

/// The value held for at least `sustain_us` at each point: the least over
/// the stretch ending there that spans `sustain_us` (the rule engine's
/// `sustain`, first to last point). `NaN` while no such stretch exists or
/// one of its values is unknown. Negate a measure where lower is worse.
pub fn held(t: &[SimTime], x: &[f64], sustain_us: i64) -> Vec<f64> {
    (0..x.len())
        .map(|i| {
            let k = t[..=i].partition_point(|&s| t[i].us_since(s) >= sustain_us);
            let Some(k) = k.checked_sub(1) else {
                return f64::NAN;
            };
            x[k..=i]
                .iter()
                .try_fold(f64::INFINITY, |m, &v| (!v.is_nan()).then(|| m.min(v)))
                .unwrap_or(f64::NAN)
        })
        .collect()
}

/// The sustained worst: the largest value `held` for `sustain_us`, with its
/// index; `None` if none is held that long.
pub fn sustained_worst(t: &[SimTime], x: &[f64], sustain_us: i64) -> Option<(usize, f64)> {
    held(t, x, sustain_us)
        .into_iter()
        .enumerate()
        .filter(|(_, v)| !v.is_nan())
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAN: f64 = f64::NAN;

    /// `n` points 1 s apart.
    fn secs(n: usize) -> Vec<SimTime> {
        (0..n as i64)
            .map(|s| SimTime::from_micros(s * 1_000_000))
            .collect()
    }

    #[test]
    fn held_values_last_from_first_to_last_point() {
        let t = secs(6);
        let x = [1.0, 5.0, 4.0, 6.0, 2.0, 3.0];
        // 2 s: three points, the least of them.
        let h = held(&t, &x, 2_000_000);
        assert!(h[0].is_nan() && h[1].is_nan());
        assert_eq!(h[2..], [1.0, 4.0, 2.0, 2.0]);
        assert_eq!(sustained_worst(&t, &x, 2_000_000), Some((3, 4.0)));
        // Without sustain every point holds its own value.
        assert_eq!(held(&t, &x, 0), x);
        // An unknown value breaks the stretches through it.
        let h = held(&t, &[3.0, 3.0, NAN, 3.0, 3.0, 3.0], 1_000_000);
        assert_eq!(h.iter().filter(|v| !v.is_nan()).count(), 3);
        assert_eq!(sustained_worst(&t, &x, 9_000_000), None);
    }

    #[test]
    fn the_stable_height_follows_the_last_unstable_point() {
        let h = [1000.0, 800.0, 600.0, 400.0];
        assert_eq!(stable_from(&h, &[true; 4]), Some(1000.0));
        assert_eq!(stable_from(&h, &[true, false, true, true]), Some(600.0));
        assert_eq!(stable_from(&h, &[true, true, true, false]), None);
        assert_eq!(stable_from(&[], &[]), None);
    }
}
