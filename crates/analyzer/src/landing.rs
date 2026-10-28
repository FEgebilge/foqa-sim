use std::ops::RangeInclusive;

use fdm_core::geo::distance_m;
use fdm_core::{Airport, SimTime};

use crate::runway::{RunwayFix, ground_track_deg, locate};
use crate::track::{ground_edges, hops};
use crate::{Flight, Track};

/// Airborne hop between two touchdowns up to which the second makes the
/// landing a bounced one. Longer than track.rs's `BOUNCE_MAX_US`, which only
/// decides whether a brief ground contact still belongs to the same leg.
pub(crate) const BOUNCE_AIRBORNE_MAX_US: i64 = 5_000_000;
/// A flight loaded in the air that is on the ground within this long was
/// placed there by the sim (spawn, slew), not landed.
const SETTLE_MAX_US: i64 = 5_000_000;
/// Window for the touchdown load: just before contact to gear compression.
const PEAK_G_BEFORE_US: i64 = 500_000;
const PEAK_G_AFTER_US: i64 = 1_500_000;
/// The touchdown load is read as a 16 samples/s DFDR records it, the rate
/// the 737 AMM hard-landing indication (2.2 g) assumes; slower recorders
/// have other values (Flight Safety Digest, Aug 2004, p. 6). A mean over
/// one sample period, a boxcar with its -3 dB point (0.44 / T, 7 Hz) near
/// that recorder's Nyquist, keeps a frame-rate spike from setting the peak.
const FDR_NZ_PERIOD_US: i64 = 62_500;
/// Radio height rate over this last stretch before contact gives the sink
/// rate; the touchdown window records RA at frame rate.
const SINK_WINDOW_US: i64 = 500_000;
/// The sim's touchdown snapshot updates on contact; read it just after.
const SIM_SNAPSHOT_DELAY_US: i64 = 500_000;
/// Ground track is taken over this span after contact: long enough for a
/// usable baseline at landing speed, short before the turn-off.
const TRACK_SPAN_US: i64 = 3_000_000;
/// Less ground covered than this over the span (a stopped or stationary
/// aircraft, ~3 m/s) gives no meaningful track.
const TRACK_MIN_M: f64 = 10.0;

/// One air-to-ground transition and the values that grade it.
#[derive(Debug, Clone, PartialEq)]
pub struct Touchdown {
    pub t: SimTime,
    /// Follows a short hop off the ground: part of a bounced landing.
    pub bounce: bool,
    /// Rate of descent at contact (positive down): radio height rate over the
    /// last 0.5 s, or inertial VS where RA is missing.
    pub sink_fpm: f64,
    /// Highest normal load around contact, at the 16 samples/s of a DFDR
    /// (the recording holds every frame in the touchdown window).
    pub peak_g: f64,
    pub pitch_deg: Option<f64>,
    pub roll_deg: Option<f64>,
    pub ias_kt: Option<f64>,
    /// The sim's own touchdown velocity (truth channel), for comparison.
    pub sim_sink_fpm: Option<f64>,
    /// Where on which runway it touched down; `None` without recorded airport
    /// data or a runway match.
    pub runway: Option<RunwayFix>,
}

/// A touchdown found in the air/ground edges, before grading.
struct Contact {
    t: SimTime,
    bounce: bool,
    /// Where to look for the peak load: this impact only.
    window: RangeInclusive<SimTime>,
}

/// Touchdowns from the valid air/ground samples only; a failed sample is not
/// a state change. Each touchdown's peak-g window is cut at the neighbouring
/// liftoffs so a bounce's hard impact does not grade the light one before it.
pub fn touchdowns(flight: &Flight, track: &Track) -> Vec<Touchdown> {
    let airports = flight.airports();
    contacts(flight, track)
        .into_iter()
        .map(|c| grade(flight, track, &airports, c))
        .collect()
}

/// Edge pass: states alternate, so every touchdown's neighbours are liftoffs.
fn contacts(flight: &Flight, track: &Track) -> Vec<Contact> {
    let mut edges = ground_edges(flight);
    let first = flight.series("air_ground").iter().find(|s| s.is_valid());
    let settled = matches!((first, edges.first()), (Some(s), Some(&(t, true)))
        if t.us_since(s.t) < SETTLE_MAX_US);
    let hops = hops(&track.t, &track.ra_ft, &edges);
    edges.retain(|&(t, _)| !hops.iter().any(|h| h.start == t || h.end == t));
    let edges = &edges[usize::from(settled)..];
    let mut out = Vec::new();
    let (mut prev, mut liftoff): (Option<SimTime>, Option<SimTime>) = (None, None);
    for (i, &(t, is_touchdown)) in edges.iter().enumerate() {
        if !is_touchdown {
            liftoff = Some(t);
            continue;
        }
        // A bounce needs an earlier touchdown and a short hop since it.
        let hop = prev.and(liftoff);
        let bounce = hop.is_some_and(|off| t.us_since(off) <= BOUNCE_AIRBORNE_MAX_US);
        let mut from = t.add_us(-PEAK_G_BEFORE_US);
        if let Some((prev, off)) = prev.zip(hop) {
            from = from.max(prev.add_us(PEAK_G_AFTER_US).min(off));
        }
        let mut to = t.add_us(PEAK_G_AFTER_US);
        if let Some(next) = edges.get(i + 1) {
            to = to.min(next.0);
        }
        out.push(Contact {
            t,
            bounce,
            window: from..=to,
        });
        prev = Some(t);
    }
    out
}

/// The values that grade one touchdown.
fn grade(flight: &Flight, track: &Track, airports: &[Airport], c: Contact) -> Touchdown {
    let t = c.t;
    let nz: Vec<_> = flight
        .series("accel.normal")
        .iter()
        .filter(|s| s.is_valid() && c.window.contains(&s.t))
        .collect();
    let peak_g = (0..nz.len())
        .map(|i| {
            let k = nz[..=i].partition_point(|s| nz[i].t.us_since(s.t) >= FDR_NZ_PERIOD_US);
            nz[k..=i].iter().map(|s| s.value).sum::<f64>() / (i + 1 - k) as f64
        })
        .fold(f64::NAN, f64::max);
    // Without RA, the inertial VS at the last point before contact.
    let inertial_sink = || {
        let last_airborne = track.t.partition_point(|&x| x < t).saturating_sub(1);
        -track.vs_fpm[last_airborne]
    };
    Touchdown {
        t,
        bounce: c.bounce,
        sink_fpm: ra_sink_fpm(flight, t).unwrap_or_else(inertial_sink),
        peak_g,
        pitch_deg: flight.at("pitch", t),
        roll_deg: flight.at("roll", t),
        ias_kt: flight.at("ias", t),
        sim_sink_fpm: flight.at("sim.td.vs", t.add_us(SIM_SNAPSHOT_DELAY_US)),
        runway: runway_fix(flight, airports, t),
    }
}

/// (lat, lon) at `t`.
pub(crate) fn position(flight: &Flight, t: SimTime) -> Option<(f64, f64)> {
    Some((flight.at("pos.lat", t)?, flight.at("pos.lon", t)?))
}

/// True ground track at contact, from the positions after it, or before it
/// if the aircraft is stopped or the data ends. Runway matching gates on it
/// to tell crossing runways apart.
fn track_at(flight: &Flight, t: SimTime) -> Option<f64> {
    let here = position(flight, t)?;
    let after = position(flight, t.add_us(TRACK_SPAN_US)).map(|p| (here, p));
    let before = position(flight, t.add_us(-TRACK_SPAN_US)).map(|p| (p, here));
    [after, before]
        .into_iter()
        .flatten()
        .find(|&(a, b)| distance_m(a, b) >= TRACK_MIN_M)
        .map(|(a, b)| ground_track_deg(a, b))
}

fn runway_fix(flight: &Flight, airports: &[Airport], t: SimTime) -> Option<RunwayFix> {
    let (lat, lon) = position(flight, t)?;
    locate(airports, lat, lon, track_at(flight, t)?)
}

fn ra_sink_fpm(flight: &Flight, t: SimTime) -> Option<f64> {
    let ra = flight.series("alt.radio");
    let window: Vec<_> = ra
        .iter()
        .filter(|s| s.is_valid() && (1..=SINK_WINDOW_US).contains(&t.us_since(s.t)))
        .collect();
    let (first, last) = (window.first()?, window.last()?);
    let dt_us = last.t.us_since(first.t);
    (dt_us >= SINK_WINDOW_US / 2).then(|| (first.value - last.value) / dt_us as f64 * 60e6)
}

#[cfg(test)]
mod tests {
    use fdm_core::Ssm;

    use crate::flight::tests::{Row, flight, ok};

    use super::*;

    /// 10 Hz air/ground and normal load from `(secs, on_ground)` and
    /// `(secs, g)` points.
    fn rows(ground: &[(f64, bool)], g: &[(f64, f64)]) -> Vec<Row> {
        let mut rows: Vec<_> = (0..=150)
            .map(|k| {
                let t = f64::from(k) * 0.1;
                let on = ground
                    .iter()
                    .rfind(|p| p.0 <= t + 1e-9)
                    .is_some_and(|p| p.1);
                ok("air_ground", t, f64::from(u8::from(on)))
            })
            .collect();
        rows.extend((0..=150).map(|k| {
            let t = f64::from(k) * 0.1;
            let hit = g.iter().find(|p| (p.0 - t).abs() < 1e-9);
            ok("accel.normal", t, hit.map_or(1.0, |p| p.1))
        }));
        rows
    }

    fn run(rows: &[Row]) -> Vec<Touchdown> {
        let flight = flight(rows);
        touchdowns(&flight, &Track::new(&flight).unwrap())
    }

    /// 1 Hz positions moving `step` degrees of latitude per second (north)
    /// from 38.0, 0 to 20 s.
    fn pos_rows(step: f64) -> Vec<Row> {
        (0..=20)
            .flat_map(|s| {
                let t = f64::from(s);
                [ok("pos.lat", t, 38.0 + step * t), ok("pos.lon", t, 27.0)]
            })
            .collect()
    }

    #[test]
    fn track_comes_from_the_positions_after_contact() {
        let f = flight(&pos_rows(0.0001));
        let t = SimTime::from_secs_f64(10.0);
        let track = track_at(&f, t).unwrap();
        assert!(!(0.01..359.99).contains(&track), "{track}");
    }

    #[test]
    fn track_falls_back_to_before_contact() {
        // Data ends 1 s after contact: no baseline ahead, 3 s behind.
        let mut rows = pos_rows(0.0001);
        rows.retain(|r| r.1 <= 11.0);
        let f = flight(&rows);
        let t = SimTime::from_secs_f64(10.0);
        assert!(track_at(&f, t).is_some());
        // Stopped: neither side moved.
        assert_eq!(track_at(&flight(&pos_rows(0.0)), t), None);
    }

    #[test]
    fn no_airports_means_no_runway() {
        let f = flight(&pos_rows(0.0001));
        assert_eq!(runway_fix(&f, &[], SimTime::from_secs_f64(10.0)), None);
    }

    #[test]
    fn invalid_air_ground_samples_make_no_touchdown() {
        let mut rows = rows(&[(0.0, false), (10.0, true)], &[]);
        // A failed sample reading "ground" in mid-air.
        rows.push(("air_ground", 5.05, 1.0, Ssm::Failure));
        let td = run(&rows);
        assert_eq!(td.len(), 1);
        assert!((td[0].t.as_secs_f64() - 10.0).abs() < 0.11);
    }

    #[test]
    fn settling_after_load_is_not_a_landing() {
        // Loaded in the air, on the ground 1 s later: placed by the sim.
        assert!(run(&rows(&[(0.0, false), (1.0, true)], &[])).is_empty());
        // Then a real circuit: that landing counts.
        let ground = [(0.0, false), (1.0, true), (3.0, false), (12.0, true)];
        let td = run(&rows(&ground, &[]));
        assert_eq!(td.len(), 1);
        assert!(!td[0].bounce);
    }

    #[test]
    fn a_wheel_lifting_for_a_moment_is_no_bounce() {
        // 0.4 s off the ground with no radio height: one touchdown.
        let ground = [(0.0, false), (10.0, true), (12.0, false), (12.4, true)];
        let td = run(&rows(&ground, &[]));
        assert_eq!(td.len(), 1);
        assert!((td[0].t.as_secs_f64() - 10.0).abs() < 0.11);
    }

    #[test]
    fn peak_g_is_read_at_the_recorder_rate() {
        // 50 Hz load in the window: 1.2 g with one 2.4 g frame at contact,
        // then 2.0 g held for 0.2 s. A 16 Hz recorder sees ~(2.4 + 3 x 1.2) / 4
        // = 1.5 g from the spike, so the held 2.0 g sets the peak.
        let mut rows = rows(&[(0.0, false), (10.0, true)], &[]);
        rows.retain(|r| r.0 != "accel.normal");
        rows.extend((450..=560).map(|k| {
            let t = f64::from(k) * 0.02;
            let g = match k {
                500 => 2.4,
                520..=530 => 2.0,
                _ => 1.2,
            };
            ok("accel.normal", t, g)
        }));
        let td = run(&rows);
        assert!((td[0].peak_g - 2.0).abs() < 1e-9, "{}", td[0].peak_g);
    }

    #[test]
    fn a_bounce_grades_each_impact_on_its_own() {
        // Light first impact (1.2 g), a 1.1 s hop, hard second (2.5 g).
        let ground = [(0.0, false), (10.0, true), (10.4, false), (11.5, true)];
        let g = [(10.1, 1.2), (11.1, 2.5), (11.6, 2.5)];
        let td = run(&rows(&ground, &g));
        assert_eq!(td.len(), 2);
        assert!((td[0].peak_g - 1.2).abs() < 1e-9, "{}", td[0].peak_g);
        assert!((td[1].peak_g - 2.5).abs() < 1e-9);
        assert!(td[1].bounce);
    }
}
