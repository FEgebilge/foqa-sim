use std::ops::Range;

use fdm_core::SimTime;
use fdm_core::geo::distance_m;

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
/// A hop off the ground shorter than this that rises less than
/// `HOP_MIN_FT` of radio height is a wheel lifting for a moment (a bump,
/// gear rebound, a stationary aircraft's flickering state): it stayed on
/// the ground, so the hop is no liftoff and its end no touchdown. Legs and
/// touchdowns share this one definition (`hops`).
const HOP_MIN_US: i64 = 1_000_000;
const HOP_MIN_FT: f64 = 3.0;
/// A lowest point the altitude descended to and climbed back from by this
/// much ended an approach (a go-around or low approach): the go-around rise
/// of every class (`PhaseConfig::go_around_rise_ft`).
const DIP_RISE_FT: f64 = 200.0;
/// ... when it lies this low above the airport flown to: the highest
/// approach start of the shipped profiles (FAA AIM 4-3-3 turbine pattern).
const DIP_MAX_FT: f64 = 1500.0;
/// An approach is flown to the airport whose reference point lies this
/// close to its lowest point: the capture's search radius around a contact
/// (`capture` `AIRPORT_RADIUS_M`), so an airport there is recorded.
const APPROACH_AIRPORT_M: f64 = 10_000.0;
const FT_PER_M: f64 = 1.0 / 0.3048;

/// One airborne period, bounces included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leg {
    /// Track indices; `airborne.end` is the first ground point after it.
    pub airborne: Range<usize>,
    /// Highest point. Without a go-around near a recorded airport, AAL
    /// refers to the departure aerodrome up to here and to the arrival
    /// aerodrome after (`aal`).
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
    /// Height above the aerodrome each stretch of a leg is flown from or
    /// to (`aal`); 0 on the ground. Known only after the flight, which is
    /// why the ground segment can use it.
    pub aal_ft: Vec<f64>,
    /// Points where the sim moved the aircraft (`Flight::breaks`).
    pub breaks: Vec<Range<usize>>,
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
            breaks: Vec::new(),
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
        let mut on_ground: Vec<_> = known
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
        for hop in hops(&t, &ra_ft, &ground_edges(flight)) {
            let from = t.partition_point(|&x| x < hop.start);
            let to = t.partition_point(|&x| x < hop.end);
            on_ground[from..to].fill(true);
        }
        let vs_fpm = vs::inertial(
            &alt_ft,
            &nz_g,
            &pitch_deg,
            &roll_deg,
            &on_ground,
            1.0 / TRACK_HZ,
        );
        let legs = legs(&on_ground, &alt_ft);
        let airports = flight.airports();
        let near = |i: usize| {
            let t = *t.get(i)?;
            let at = |name| flight.at(name, t);
            let pos = (at("pos.lat")?, at("pos.lon")?);
            let away = |a: &fdm_core::Airport| distance_m(pos, (a.lat_deg, a.lon_deg));
            let (k, a) = (airports.iter().enumerate())
                .filter(|(_, a)| away(a) <= APPROACH_AIRPORT_M)
                .min_by(|x, y| away(x.1).total_cmp(&away(y.1)))?;
            Some((k, a.elevation_m * FT_PER_M))
        };
        let aal_ft = aal(&legs, &alt_ft, near);
        let index = |at: SimTime| t.partition_point(|&x| x < at);
        let breaks = (flight.breaks().into_iter())
            .map(|b| index(b.start)..index(b.end))
            .filter(|r| !r.is_empty())
            .collect();
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
            breaks,
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

    /// The stretches the `breaks` cut the track into, in order and covering
    /// it; a break is a stretch of its own.
    pub(crate) fn segments(&self) -> Vec<Range<usize>> {
        let mut cuts: Vec<usize> = (self.breaks.iter())
            .flat_map(|b| [b.start, b.end.min(self.len())])
            .chain([0, self.len()])
            .collect();
        cuts.sort_unstable();
        cuts.dedup();
        cuts.windows(2).map(|w| w[0]..w[1]).collect()
    }

    /// Points `r` as a track of their own, without breaks: legs clipped to
    /// it, each with the highest point of its part.
    pub(crate) fn slice(&self, r: Range<usize>) -> Self {
        let cut = |v: &[f64]| v[r.clone()].to_vec();
        let alt_ft = cut(&self.alt_ft);
        // Legs wholly outside `r` are dropped before the shift, which would
        // otherwise underflow for a leg that ended before it.
        let legs = (self.legs.iter())
            .filter(|l| l.airborne.start < r.end && r.start < l.airborne.end)
            .map(|l| l.airborne.start.max(r.start) - r.start..l.airborne.end.min(r.end) - r.start)
            .map(|a| Leg {
                peak: peak(&alt_ft, a.clone()),
                airborne: a,
            })
            .collect();
        Self {
            t: self.t[r.clone()].to_vec(),
            gs_kt: cut(&self.gs_kt),
            ias_kt: cut(&self.ias_kt),
            nz_g: cut(&self.nz_g),
            pitch_deg: cut(&self.pitch_deg),
            roll_deg: cut(&self.roll_deg),
            ra_ft: cut(&self.ra_ft),
            on_ground: self.on_ground[r.clone()].to_vec(),
            vs_fpm: cut(&self.vs_fpm),
            aal_ft: cut(&self.aal_ft),
            alt_ft,
            legs,
            breaks: Vec::new(),
        }
    }
}

/// Air/ground transitions of the valid samples, as (time, true for a
/// touchdown); a failed sample is not a state change.
pub(crate) fn ground_edges(flight: &Flight) -> Vec<(SimTime, bool)> {
    let valid: Vec<_> = flight
        .series("air_ground")
        .iter()
        .filter(|s| s.is_valid())
        .collect();
    valid
        .windows(2)
        .filter(|w| (w[0].value >= 0.5) != (w[1].value >= 0.5))
        .map(|w| (w[1].t, w[1].value >= 0.5))
        .collect()
}

/// Liftoff to touchdown spans of `edges` that are no real hop: off the
/// ground under `HOP_MIN_US` and never `HOP_MIN_FT` up on the radio height
/// `ra_ft` at times `t`.
pub(crate) fn hops(t: &[SimTime], ra_ft: &[f64], edges: &[(SimTime, bool)]) -> Vec<Range<SimTime>> {
    let flown = |off: SimTime, on: SimTime| {
        let (from, to) = (
            t.partition_point(|&x| x < off),
            t.partition_point(|&x| x <= on),
        );
        on.us_since(off) >= HOP_MIN_US || ra_ft[from..to].iter().any(|&h| h >= HOP_MIN_FT)
    };
    edges
        .windows(2)
        .filter_map(|w| match (w[0], w[1]) {
            ((off, false), (on, true)) if !flown(off, on) => Some(off..on),
            _ => None,
        })
        .collect()
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
        .map(|r| Leg {
            peak: peak(alt_ft, r.clone()),
            airborne: r,
        })
        .collect()
}

/// The highest point of `r` (the last of equals), else its start.
fn peak(alt_ft: &[f64], r: Range<usize>) -> usize {
    r.clone()
        .filter(|&i| alt_ft[i].is_finite())
        .max_by(|&a, &b| alt_ft[a].total_cmp(&alt_ft[b]))
        .unwrap_or(r.start)
}

/// Height above the aerodrome, split at every approach as an FDM ground
/// station does: a leg is flown from its departure, to each approach that
/// ended low near a recorded airport (`dips`; `near` gives that airport's
/// index and elevation, ft) and to its arrival, each the reference from the
/// highest point before it to the highest point after it. The departure
/// and arrival elevations are the pressure altitude on the ground just
/// before liftoff and just after touchdown. An approach airport's is the
/// elevation difference to one of those airports added to its measurement
/// (that airport's own, else the arrival's, else the departure's), which
/// keeps it in pressure altitude. An approach with no recorded airport near
/// its lowest point, or none to measure from, changes nothing.
fn aal(legs: &[Leg], alt_ft: &[f64], near: impl Fn(usize) -> Option<(usize, f64)>) -> Vec<f64> {
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
        let measured: Vec<_> = [(start.checked_sub(1), departed), (Some(end), landed)]
            .into_iter()
            .filter_map(|(i, alt)| {
                let alt = alt?;
                Some((near(i?)?, alt))
            })
            .collect();
        let elevation = |(k, ft): (usize, f64)| {
            let ((_, from_ft), alt) = (measured.iter().find(|m| m.0.0 == k)).or(measured.last())?;
            Some(alt + ft - from_ft)
        };
        let mut nodes = vec![(start, departure)];
        nodes.extend(dips(alt_ft, start..end).into_iter().filter_map(|i| {
            let e = near(i).and_then(elevation)?;
            (alt_ft[i] - e < DIP_MAX_FT).then_some((i, e))
        }));
        nodes.push((end, arrival));
        let peaks: Vec<_> = (nodes.windows(2))
            .map(|w| peak(alt_ft, w[0].0..w[1].0))
            .collect();
        for i in start..end {
            aal[i] = alt_ft[i] - nodes[peaks.partition_point(|&p| p < i)].1;
        }
    }
    aal
}

/// The lowest points of `r` the altitude descended to by `DIP_RISE_FT` and
/// climbed back from by as much, in order.
fn dips(alt_ft: &[f64], r: Range<usize>) -> Vec<usize> {
    let (mut out, mut top, mut bottom) = (Vec::new(), f64::NEG_INFINITY, None::<usize>);
    for i in r.filter(|&i| alt_ft[i].is_finite()) {
        let a = alt_ft[i];
        match bottom {
            Some(b) if a < alt_ft[b] => bottom = Some(i),
            Some(b) if a >= alt_ft[b] + DIP_RISE_FT => {
                out.push(b);
                (top, bottom) = (a, None);
            }
            None if a > top => top = a,
            None if a <= top - DIP_RISE_FT => bottom = Some(i),
            _ => {}
        }
    }
    out
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
    fn a_wheel_lifting_on_the_rollout_is_no_leg() {
        // 4 Hz: airborne to 10 s, then rolling with 0.5 s off the ground at
        // 15 s and no radio height: one leg, on the ground throughout the hop.
        let rows: Vec<_> = (0..=80)
            .map(|k| f64::from(k) * 0.25)
            .map(|t| {
                ok(
                    "air_ground",
                    t,
                    f64::from(u8::from(t >= 10.0 && !(15.0..15.5).contains(&t))),
                )
            })
            .collect();
        let track = Track::new(&flight(&rows)).unwrap();
        assert_eq!(track.legs.len(), 1);
        assert!(track.on_ground[track.index_at(SimTime::from_secs_f64(15.25))]);
    }

    /// Off A (0 ft) up to 3000 ft, a go-around at 1200 ft near B (1000 ft
    /// elevation), up to 5000 ft and down to C (300 ft), in 100 ft steps.
    fn diversion() -> (Vec<f64>, usize, usize) {
        let steps = |a: i32, b: i32| {
            let step = if b > a { 100 } else { -100 };
            (0..=(b - a) / step).map(move |k| f64::from(a + k * step))
        };
        let mut alt = vec![0.0];
        alt.extend(steps(100, 3000).chain(steps(2900, 1200)));
        let dip = alt.len() - 1;
        alt.extend(steps(1300, 5000).chain(steps(4900, 400)));
        let end = alt.len();
        alt.push(300.0);
        (alt, dip, end)
    }

    #[test]
    fn an_approach_is_measured_from_the_airport_flown_to() {
        let (alt, dip, end) = diversion();
        let mut ground = vec![false; alt.len()];
        (ground[0], ground[end]) = (true, true);
        let legs = legs(&ground, &alt);
        let top = |from: usize| (from..end).find(|&i| alt[i + 1] < alt[i]).unwrap();
        let (first_top, peak) = (top(1), legs[0].peak);
        let airports = |b: bool| {
            move |i: usize| match i {
                0 => Some((0, 0.0)),
                _ if i == dip && b => Some((1, 1000.0)),
                _ if i == end => Some((2, 300.0)),
                _ => None,
            }
        };
        let aal = aal(&legs, &alt, airports(true));
        // The go-around 200 ft above B, before the leg's highest point; B
        // from the top before it to the top after it, C after.
        assert_eq!(aal[dip], 200.0);
        assert_eq!((aal[first_top], aal[first_top + 1]), (3000.0, 1900.0));
        assert_eq!((aal[peak], aal[peak + 1]), (4000.0, 4600.0));
        // No airport recorded there: departure up to the peak, as before.
        let today = super::aal(&legs, &alt, airports(false));
        assert_eq!(
            (today[dip], today[peak], today[peak + 1]),
            (1200.0, 5000.0, 4600.0)
        );
        // A dip far above the airport there is no approach.
        let high = |i: usize| (i == dip).then_some((1, -1000.0)).or(airports(true)(i));
        assert_eq!(super::aal(&legs, &alt, high)[dip], 1200.0);
    }

    #[test]
    fn only_a_descent_and_climb_of_the_rise_is_a_dip() {
        let alt = [0.0, 500.0, 350.0, 450.0, 250.0, 460.0, 300.0];
        assert_eq!(dips(&alt, 0..alt.len()), [4]);
        assert_eq!(
            dips(&[0.0, 500.0, 290.0, 480.0, 0.0], 0..5),
            [] as [usize; 0]
        );
    }

    #[test]
    fn no_air_ground_data_gives_no_track() {
        assert!(Track::new(&flight(&[ok("pitch", 0.0, 1.0), ok("pitch", 1.0, 1.0)])).is_none());
    }
}
