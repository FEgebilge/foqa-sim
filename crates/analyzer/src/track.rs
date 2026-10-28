use std::ops::Range;

use fdm_core::SimTime;

use crate::{Flight, runs, vs};

/// Rate of the common time base, the FDR rate of normal acceleration.
pub const TRACK_HZ: f64 = 8.0;
/// Radio altimeters are only trusted below this height.
pub(crate) const RA_MAX_FT: f64 = 2500.0;
/// On-ground speed below which the RA reading is taken as its bias.
pub(crate) const RA_BIAS_MAX_GS_KT: f64 = 30.0;
/// Ground contact up to this long between airborne periods is a bounce,
/// not the end of a leg. (Landing grading uses a separate, longer limit on
/// the airborne hop between two touchdowns.)
pub(crate) const BOUNCE_MAX_US: i64 = 3_000_000;

/// One airborne period, bounces included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leg {
    /// Track indices; `airborne.end` is the first ground point after it.
    pub airborne: Range<usize>,
    /// Highest point: AAL refers to the departure aerodrome up to here and to
    /// the arrival aerodrome after.
    pub peak: usize,
}

/// The flight on a uniform time base, calibrated and with derived
/// parameters. `NaN` where a parameter has no valid value.
pub struct Track {
    pub t: Vec<SimTime>,
    pub alt_ft: Vec<f64>,
    pub gs_kt: Vec<f64>,
    pub ias_kt: Vec<f64>,
    pub nz_g: Vec<f64>,
    pub pitch_deg: Vec<f64>,
    pub roll_deg: Vec<f64>,
    /// Radio height with the on-ground bias removed, `NaN` above 2500 ft.
    pub ra_ft: Vec<f64>,
    pub on_ground: Vec<bool>,
    /// Inertial vertical speed, derived (K4b).
    pub vs_fpm: Vec<f64>,
    pub legs: Vec<Leg>,
    /// Height above the departure aerodrome up to a leg's highest point,
    /// above the arrival aerodrome after it; 0 on the ground. Known only
    /// after the flight, which is why the ground segment can use it.
    pub aal_ft: Vec<f64>,
}

impl Track {
    /// Spacing of the time base.
    pub const STEP_US: i64 = (1e6 / TRACK_HZ) as i64;

    /// `n` points from time zero with every parameter missing (`NaN`), in
    /// the air, no legs: a canvas for synthetic tracks in tests and tools.
    pub fn blank(n: usize) -> Self {
        let nan = vec![f64::NAN; n];
        Self {
            t: (0..n as i64)
                .map(|i| SimTime::from_micros(i * Self::STEP_US))
                .collect(),
            alt_ft: nan.clone(),
            gs_kt: nan.clone(),
            ias_kt: nan.clone(),
            nz_g: nan.clone(),
            pitch_deg: nan.clone(),
            roll_deg: nan.clone(),
            ra_ft: nan.clone(),
            on_ground: vec![false; n],
            vs_fpm: nan.clone(),
            legs: Vec::new(),
            aal_ft: nan,
        }
    }

    /// `None` if the flight has no air/ground data. A gap in it keeps the
    /// last known state (the first known one before the first value): a
    /// dropout is far shorter than a flight phase.
    pub fn new(flight: &Flight) -> Option<Self> {
        let (first, last) = flight.span()?;
        let t: Vec<_> = (0..=last.us_since(first))
            .step_by(Self::STEP_US as usize)
            .map(|us| first.add_us(us))
            .collect();
        let sample = |name| flight.resample(name, &t);

        let known: Vec<_> = t
            .iter()
            .map(|&t| flight.at("air_ground", t).map(|v| v >= 0.5))
            .collect();
        let mut state = known.iter().flatten().next().copied()?;
        let on_ground: Vec<_> = known
            .into_iter()
            .map(|g| {
                state = g.unwrap_or(state);
                state
            })
            .collect();
        let alt_ft = sample("alt.pressure");
        let gs_kt = sample("groundspeed");
        let nz_g = sample("accel.normal");
        let pitch_deg = sample("pitch");
        let roll_deg = sample("roll");
        let ra_ft = calibrate_ra(sample("alt.radio"), &on_ground, &gs_kt);
        let vs_fpm = vs::inertial(
            &alt_ft,
            &nz_g,
            &pitch_deg,
            &roll_deg,
            &on_ground,
            1.0 / TRACK_HZ,
        );
        let legs = legs(&on_ground, &alt_ft);
        let aal_ft = aal(&legs, &alt_ft);
        Some(Self {
            ias_kt: sample("ias"),
            t,
            alt_ft,
            gs_kt,
            nz_g,
            pitch_deg,
            roll_deg,
            ra_ft,
            on_ground,
            vs_fpm,
            legs,
            aal_ft,
        })
    }

    pub fn len(&self) -> usize {
        self.t.len()
    }

    pub fn is_empty(&self) -> bool {
        self.t.is_empty()
    }

    /// The airborne points: each leg from lift-off to touchdown.
    pub fn airborne(&self) -> impl Iterator<Item = usize> + '_ {
        self.legs.iter().flat_map(|l| l.airborne.clone())
    }

    /// Index of the last point at or before `t`.
    pub fn index_at(&self, t: SimTime) -> usize {
        self.t.partition_point(|&x| x <= t).saturating_sub(1)
    }
}

fn legs(on_ground: &[bool], alt_ft: &[f64]) -> Vec<Leg> {
    let mut airborne: Vec<Range<usize>> = Vec::new();
    for r in runs(on_ground.len(), |i| !on_ground[i]) {
        match airborne.last_mut() {
            Some(prev) if (r.start - prev.end) as i64 * Track::STEP_US <= BOUNCE_MAX_US => {
                prev.end = r.end;
            }
            _ => airborne.push(r),
        }
    }
    airborne
        .into_iter()
        .map(|r| {
            let peak = r
                .clone()
                .filter(|&i| alt_ft[i].is_finite())
                .max_by(|&a, &b| alt_ft[a].total_cmp(&alt_ft[b]))
                .unwrap_or(r.start);
            Leg { airborne: r, peak }
        })
        .collect()
}

/// Aerodrome elevations come from the pressure altitude on the ground just
/// before liftoff and just after touchdown.
fn aal(legs: &[Leg], alt_ft: &[f64]) -> Vec<f64> {
    let mut aal = vec![0.0; alt_ft.len()];
    for leg in legs {
        let Range { start, end } = leg.airborne;
        let landed = alt_ft.get(end).copied().filter(|a| a.is_finite());
        let departed = start
            .checked_sub(1)
            .map(|i| alt_ft[i])
            .filter(|a| a.is_finite());
        let departure = departed.or(landed).unwrap_or(f64::NAN);
        let arrival = landed.unwrap_or(departure);
        for i in start..end {
            aal[i] = alt_ft[i] - if i <= leg.peak { departure } else { arrival };
        }
    }
    aal
}

/// A real RA reads zero with the wheels on the ground; MSFS measures from a
/// reference point (~4 ft on a C172). Removes the median on-ground reading.
fn calibrate_ra(mut ra: Vec<f64>, on_ground: &[bool], gs_kt: &[f64]) -> Vec<f64> {
    let mut parked: Vec<f64> = ra
        .iter()
        .zip(on_ground.iter().zip(gs_kt))
        .filter(|&(r, (&g, &gs))| g && gs < RA_BIAS_MAX_GS_KT && r.is_finite())
        .map(|(&r, _)| r)
        .collect();
    parked.sort_by(f64::total_cmp);
    let bias = parked.get(parked.len() / 2).copied().unwrap_or(0.0);
    for r in &mut ra {
        *r -= bias;
        if *r > RA_MAX_FT {
            *r = f64::NAN;
        }
    }
    ra
}

#[cfg(test)]
mod tests {
    use crate::flight::tests::{flight, ok};

    use super::*;

    #[test]
    fn a_nan_altitude_is_never_the_leg_peak() {
        let alt = [0.0, f64::NAN, 5.0, 3.0, 0.0];
        assert_eq!(legs(&[false; 5], &alt)[0].peak, 2);
        // No finite altitude at all: fall back to the leg start.
        assert_eq!(legs(&[false; 2], &[f64::NAN; 2])[0].peak, 0);
    }

    #[test]
    fn a_gap_in_air_ground_keeps_the_last_known_state() {
        // 4 Hz air/ground: on ground 0-1 s, nothing 1.25-3 s, airborne 3.25-4 s.
        let rows: Vec<_> = (0..=16)
            .map(|k| f64::from(k) * 0.25)
            .filter(|&t| !(1.0 < t && t < 3.0))
            .map(|t| ok("air_ground", t, if t < 2.0 { 1.0 } else { 0.0 }))
            .collect();
        let track = Track::new(&flight(&rows)).expect("gaps must not drop the track");
        let at = |s: f64| track.on_ground[track.index_at(SimTime::from_secs_f64(s))];
        assert!(at(0.0) && at(2.0));
        assert!(!at(3.5));
    }

    #[test]
    fn no_air_ground_data_gives_no_track() {
        assert!(Track::new(&flight(&[ok("pitch", 0.0, 1.0), ok("pitch", 1.0, 1.0)])).is_none());
    }
}
