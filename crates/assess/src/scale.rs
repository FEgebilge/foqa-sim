//! The scale of docs/scoring.md §2.1: a measured value to a continuous
//! level ℓ (0-3) through a band's anchors, and ℓ to the loss of a domain.

use rules::interpolate;

use crate::method::{ANCHOR_ELL, Anchor, Band, Context};

/// One side's anchors as known for a landing, `None` where absent. Each
/// anchor is held to never go past the next one (e.g. on a short runway
/// the long-landing L1 stops at the end of the touchdown zone).
pub fn resolve(anchors: &[Anchor; 5], ctx: &Context, sign: f64) -> [Option<f64>; 5] {
    let mut out = anchors.clone().map(|a| a.at(ctx));
    let mut next: Option<f64> = None;
    for a in out.iter_mut().rev().flatten() {
        if let Some(n) = next
            && sign * *a > sign * n
        {
            *a = n;
        }
        next = Some(*a);
    }
    out
}

/// ℓ of `x` on one side (`sign` 1 for above, -1 for below), linear
/// between the present anchors and held at the last. Before the first
/// present anchor ℓ is 0, so a band without an ideal steps up there.
pub fn side_ell(anchors: &[Option<f64>; 5], x: f64, sign: f64) -> f64 {
    let (mut xs, mut ys) = (Vec::with_capacity(6), Vec::with_capacity(6));
    for (a, ell) in anchors.iter().zip(ANCHOR_ELL) {
        if let Some(a) = a {
            if xs.is_empty() && ell > 0.0 {
                xs.push(sign * a);
                ys.push(0.0);
            }
            xs.push(sign * a);
            ys.push(ell);
        }
    }
    if xs.is_empty() || x.is_nan() {
        return 0.0;
    }
    interpolate(&xs, &ys, sign * x)
}

/// ℓ of a band on each side, (above, below), for the value worst above
/// (`high`) and worst below (`low`); for a single value both are that
/// value.
pub fn sides(band: &Band, ctx: &Context, high: f64, low: f64) -> (f64, f64) {
    (
        side_ell(&resolve(&band.above, ctx, 1.0), high, 1.0),
        side_ell(&resolve(&band.below, ctx, -1.0), low, -1.0),
    )
}

/// The band's ℓ: the worse side.
pub fn ell(band: &Band, ctx: &Context, high: f64, low: f64) -> f64 {
    let (above, below) = sides(band, ctx, high, low);
    above.max(below)
}

/// The level shown: whole levels, 0 to 3.
pub fn level(ell: f64) -> u8 {
    ell.clamp(0.0, 3.0).floor() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVELS: [Option<f64>; 5] = [
        Some(240.0),
        Some(360.0),
        Some(400.0),
        Some(480.0),
        Some(600.0),
    ];

    #[test]
    fn ell_runs_through_the_anchors_and_holds_at_three() {
        let ell = |x| side_ell(&LEVELS, x, 1.0);
        assert_eq!(ell(100.0), 0.0);
        assert_eq!(ell(240.0), 0.0);
        assert_eq!(ell(300.0), 0.25);
        assert_eq!(ell(360.0), 0.5);
        assert_eq!(ell(440.0), 1.5);
        assert_eq!(ell(900.0), 3.0);
    }

    #[test]
    fn a_missing_anchor_interpolates_to_the_next() {
        // Data4Safety has no L1 below 500 ft: tol 1000, L2 1200.
        let below_500 = [Some(529.0), Some(1000.0), None, Some(1200.0), Some(1500.0)];
        assert_eq!(side_ell(&below_500, 1100.0, 1.0), 1.25);
        // No ideal or tolerance: nothing until the first anchor.
        let pitch = [None, None, Some(10.0), Some(15.0), None];
        assert_eq!(side_ell(&pitch, 9.9, 1.0), 0.0);
        assert_eq!(side_ell(&pitch, 10.0, 1.0), 1.0);
        assert_eq!(side_ell(&pitch, 20.0, 1.0), 2.0);
    }

    #[test]
    fn below_anchors_grade_smaller_values() {
        let speed = [Some(-2.0), Some(-5.0), Some(-7.0), Some(-10.0), Some(-15.0)];
        assert_eq!(side_ell(&speed, 0.0, -1.0), 0.0);
        assert_eq!(side_ell(&speed, -6.0, -1.0), 0.75);
        assert_eq!(side_ell(&speed, -20.0, -1.0), 3.0);
    }

    #[test]
    fn levels_are_whole() {
        assert_eq!(
            [0.0, 0.5, 0.99, 1.0, 2.7, 3.0, 4.0].map(level),
            [0, 0, 0, 1, 2, 3, 3]
        );
    }
}
