//! The `landing-1` score, 0 to 100: weighted parts, each a smooth curve
//! from 0 to 1 rather than bands, per aircraft class (docs/challenges.md).
//! A new formula gets a new version, so old attempts keep their score.

use analyzer::{RunwayFix, Touchdown};
use rules::{Aircraft, Level};
use serde::Serialize;

pub const SCORING: &str = "landing-1";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Score {
    pub version: &'static str,
    /// Sum of the parts' weighted scores, to 0.1.
    pub total: f64,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Part {
    pub name: &'static str,
    /// Points at a perfect score.
    pub weight: f64,
    /// 0 to 1.
    pub score: f64,
    /// What was measured, in `unit`.
    pub value: Option<f64>,
    pub unit: &'static str,
}

/// What a landing is scored on.
pub(crate) struct Landing<'a> {
    pub class: Aircraft,
    pub td: &'a Touchdown,
    pub fix: &'a RunwayFix,
    pub bounced: bool,
    /// The worst rule event from the gate to touchdown.
    pub worst: Option<Level>,
    /// Largest centreline offset on the rollout, m; `None` off the runway.
    pub rollout_cross_m: Option<f64>,
}

/// Piecewise-linear through `(x, score)` keys, held at both ends.
fn curve(keys: &[(f64, f64)], x: f64) -> f64 {
    let next = keys.partition_point(|k| k.0 <= x);
    match (next.checked_sub(1).map(|i| keys[i]), keys.get(next)) {
        (Some((x0, y0)), Some(&(x1, y1))) => y0 + (x - x0) / (x1 - x0) * (y1 - y0),
        (Some((_, y)), None) | (None, Some(&(_, y))) => y,
        (None, None) => 0.0,
    }
}

pub(crate) fn landing_1(l: &Landing) -> Score {
    let transport = l.class == Aircraft::Transport;
    let fix = l.fix;
    let part = |name, weight, score: f64, value, unit| Part {
        name,
        weight,
        score: score.clamp(0.0, 1.0),
        value,
        unit,
    };
    // From the aiming point, as a fraction of the touchdown zone; nothing
    // outside the zone.
    let off_aim = (fix.along_m - fix.aiming_point_m).abs();
    let point = if fix.in_tdz {
        curve(&[(0.08, 1.0), (0.33, 0.4), (1.0, 0.0)], off_aim / fix.tdz_m)
    } else {
        0.0
    };
    // Too soft floats past the aiming point: it loses points too.
    let sink = if transport {
        &[
            (0.0, 0.4),
            (60.0, 0.7),
            (100.0, 1.0),
            (250.0, 1.0),
            (400.0, 0.5),
            (600.0, 0.0),
        ][..]
    } else {
        &[
            (0.0, 0.5),
            (60.0, 1.0),
            (180.0, 1.0),
            (350.0, 0.4),
            (500.0, 0.0),
        ][..]
    };
    let g = if transport {
        &[(1.4, 1.0), (1.7, 0.5), (2.0, 0.0)][..]
    } else {
        &[(1.6, 1.0), (2.0, 0.5), (2.5, 0.0)][..]
    };
    let centreline = [(1.5, 1.0), (10.0, 0.3), (22.5, 0.0)];
    let stability = match l.worst {
        None | Some(Level::Advisory) => 1.0,
        Some(Level::Low) => 0.7,
        Some(Level::Medium) => 0.3,
        Some(Level::High) => 0.0,
    };
    let parts = vec![
        part("touchdown_point", 25.0, point, Some(fix.along_m), "m"),
        part(
            "sink_rate",
            25.0,
            curve(sink, l.td.sink_fpm),
            Some(l.td.sink_fpm),
            "fpm",
        ),
        part(
            "centreline",
            15.0,
            curve(&centreline, fix.cross_m.abs()),
            Some(fix.cross_m),
            "m",
        ),
        part("stability", 10.0, stability, None, ""),
        part(
            "peak_g",
            10.0,
            curve(g, l.td.peak_g),
            Some(l.td.peak_g),
            "g",
        ),
        part("bounce", 10.0, if l.bounced { 0.0 } else { 1.0 }, None, ""),
        part(
            "rollout",
            5.0,
            l.rollout_cross_m
                .map_or(0.0, |m| curve(&[(3.0, 1.0), (15.0, 0.3), (22.5, 0.0)], m)),
            l.rollout_cross_m,
            "m",
        ),
    ];
    let total = parts.iter().map(|p| p.weight * p.score).sum::<f64>();
    Score {
        version: SCORING,
        total: (total * 10.0).round() / 10.0,
        parts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_interpolate_and_hold_at_the_ends() {
        let keys = [(100.0, 1.0), (300.0, 0.0)];
        assert_eq!(curve(&keys, 50.0), 1.0);
        assert_eq!(curve(&keys, 200.0), 0.5);
        assert_eq!(curve(&keys, 900.0), 0.0);
    }
}
