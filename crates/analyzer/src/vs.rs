//! Inertial vertical speed: a third-order complementary filter, the
//! baro-inertial loop of an air data/inertial reference unit. Normal
//! acceleration, rotated to the vertical, drives the estimate at high
//! frequency; pressure altitude pulls it back at low frequency, and a third
//! state learns the accelerometer bias (MSFS `G FORCE` reads ~0.013 g low,
//! which a second-order loop turns into a steady ~200 fpm error).

/// Standard gravity, ft/s².
const G_FT_S2: f64 = 32.174;
/// Loop time constant: shorter trusts baro more, longer trusts acceleration.
/// Tuned on the C172 fixture against the sim's own vertical speed.
const TAU_S: f64 = 3.0;

/// Vertical speed in ft/min, one value per input step of `dt` seconds.
/// Held at zero on the ground. Missing attitude or acceleration falls back
/// to pure baro tracking; missing altitude to pure integration.
pub fn inertial(
    alt_ft: &[f64],
    nz_g: &[f64],
    pitch_deg: &[f64],
    roll_deg: &[f64],
    on_ground: &[bool],
    dt: f64,
) -> Vec<f64> {
    let h = alt_ft.iter().copied().find(|a| a.is_finite());
    let mut filter = Inertial::new(h.unwrap_or(0.0));
    (0..alt_ft.len())
        .map(|i| {
            filter.step(
                alt_ft[i],
                nz_g[i],
                pitch_deg[i],
                roll_deg[i],
                on_ground[i],
                dt,
            )
        })
        .collect()
}

/// The filter one step at a time, for a flight still being flown.
#[derive(Debug, Clone)]
pub(crate) struct Inertial {
    h: f64,
    /// ft/s.
    v: f64,
    /// ft/s², kept across ground periods.
    bias: f64,
}

impl Inertial {
    /// Starting at pressure altitude `h`.
    pub(crate) fn new(h: f64) -> Self {
        Self {
            h,
            v: 0.0,
            bias: 0.0,
        }
    }

    /// Vertical speed in ft/min after a step of `dt` seconds.
    pub(crate) fn step(
        &mut self,
        alt_ft: f64,
        nz_g: f64,
        pitch_deg: f64,
        roll_deg: f64,
        on_ground: bool,
        dt: f64,
    ) -> f64 {
        // Triple pole at -1/TAU_S.
        let (k1, k2, k3) = (3.0 / TAU_S, 3.0 / TAU_S.powi(2), 1.0 / TAU_S.powi(3));
        if on_ground {
            self.v = 0.0;
            if alt_ft.is_finite() {
                self.h = alt_ft;
            }
        } else {
            let (theta, phi) = (pitch_deg.to_radians(), roll_deg.to_radians());
            let a_up = (nz_g * theta.cos() * phi.cos() - 1.0) * G_FT_S2;
            let a_up = if a_up.is_finite() { a_up } else { 0.0 };
            let err = if alt_ft.is_finite() {
                alt_ft - self.h
            } else {
                0.0
            };
            self.bias += k3 * err * dt;
            self.v += (a_up + self.bias + k2 * err) * dt;
            self.h += (self.v + k1 * err) * dt;
        }
        self.v * 60.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steady_descent_converges_to_the_baro_rate() {
        let n = 8 * 60;
        let dt = 1.0 / 8.0;
        let alt: Vec<f64> = (0..n).map(|i| 3000.0 - 10.0 * i as f64 * dt).collect(); // -600 fpm
        let vs = inertial(
            &alt,
            &vec![1.0; n],
            &vec![0.0; n],
            &vec![0.0; n],
            &vec![false; n],
            dt,
        );
        assert!((vs[n - 1] + 600.0).abs() < 1.0, "{}", vs[n - 1]);
    }

    #[test]
    fn learns_a_constant_accelerometer_bias() {
        let n = 8 * 120;
        let dt = 1.0 / 8.0;
        let vs = inertial(
            &vec![3000.0; n],
            &vec![0.987; n],
            &vec![0.0; n],
            &vec![0.0; n],
            &vec![false; n],
            dt,
        );
        assert!(vs[n - 1].abs() < 5.0, "{}", vs[n - 1]);
    }

    #[test]
    fn baro_noise_is_filtered_out() {
        let n = 8 * 60;
        let dt = 1.0 / 8.0;
        // Level flight with ±20 ft alternating baro noise.
        let alt: Vec<f64> = (0..n)
            .map(|i| 3000.0 + if (i / 8) % 2 == 0 { 20.0 } else { -20.0 })
            .collect();
        let vs = inertial(
            &alt,
            &vec![1.0; n],
            &vec![0.0; n],
            &vec![0.0; n],
            &vec![false; n],
            dt,
        );
        let worst = vs[8 * 20..].iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        // A plain 1 s difference of this signal reads ±2400 fpm.
        assert!(worst < 400.0, "{worst}");
    }
}
