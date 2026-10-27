//! Runway geometry and touchdown matching. A touchdown is attributed to a
//! runway end the way an FDM ground station does it: project the touchdown
//! into the runway frame and gate on distance along the landing direction,
//! offset from the centreline and ground-track alignment.

use fdm_core::geo::LocalFrame as Frame;
use fdm_core::{Airport, Runway, RunwayEnd};

/// Touchdown-zone rule constants. Sources are to be confirmed (FAA P/CG
/// 3000 ft; "first third of the runway" rule); they will become rule-file
/// tunables.
const TDZ_MAX_M: f64 = 914.4;
const TDZ_RUNWAY_FRACTION: f64 = 1.0 / 3.0;

/// Gates: a touchdown slightly before the threshold or well past the far end
/// is still a landing on this runway (position error, undershoot, overrun).
const ALONG_BEFORE_M: f64 = 150.0;
const ALONG_AFTER_M: f64 = 300.0;
/// Margin beyond the paved edge; covers the aircraft-reference-point bias.
const CROSS_MARGIN_M: f64 = 40.0;
/// Largest ground track vs runway heading difference; separates crossing
/// runways.
const TRACK_GATE_DEG: f64 = 30.0;

/// Magnetic heading from a true one. MSFS reports MAGVAR east-negative
/// (354.4 at Izmir for ~5.6 deg E), so magnetic = true + MAGVAR (verified live).
pub fn magnetic_deg(true_deg: f64, magvar_deg: f64) -> f64 {
    (true_deg + magvar_deg).rem_euclid(360.0)
}

/// Touchdown zone length of a landing distance: min(3000 ft, LDA/3).
fn tdz_m(lda_m: f64) -> f64 {
    TDZ_MAX_M.min(lda_m * TDZ_RUNWAY_FRACTION)
}

/// Aiming point distance from the threshold by landing distance available,
/// ICAO Annex 14 Vol I, Table 5-1.
fn aiming_point_m(lda_m: f64) -> f64 {
    match lda_m {
        ..800.0 => 150.0,
        ..1200.0 => 250.0,
        ..2400.0 => 300.0,
        _ => 400.0,
    }
}

fn wrap180(deg: f64) -> f64 {
    (deg + 180.0).rem_euclid(360.0) - 180.0
}

/// True ground track from `a` to `b` (lat, lon). `heading.mag` is magnetic,
/// so the direction of travel over the ground is the true-referenced one.
pub fn ground_track_deg(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (east, north) = Frame::new(a.0, a.1).enu(b.0, b.1);
    east.atan2(north).to_degrees().rem_euclid(360.0)
}

/// Degrees from the direction a painted runway number stands for to a
/// magnetic heading; `None` for the sim's compass-point codes (37-45).
fn off_number_deg(magnetic_deg: f64, number: u8) -> Option<f64> {
    (number <= 36).then(|| wrap180(magnetic_deg - 10.0 * f64::from(number)).abs())
}

/// One landing direction of a runway.
struct LandingEnd<'a> {
    runway: &'a Runway,
    name: String,
    heading_deg: f64,
    displaced_m: f64,
}

impl<'a> LandingEnd<'a> {
    /// Both landing directions. The reported (true) heading is meant for the
    /// primary end. Runway numbers are magnetic and often a few degrees stale,
    /// so the ends are swapped only if the secondary's number is the closer
    /// match to the magnetic heading.
    fn of(runway: &'a Runway, magvar_deg: f64) -> [Self; 2] {
        let magnetic = magnetic_deg(runway.heading_deg, magvar_deg);
        let off = |e: &RunwayEnd| off_number_deg(magnetic, e.number);
        let swapped = matches!(
            (off(&runway.primary), off(&runway.secondary)),
            (Some(p), Some(s)) if s < p
        );
        let (first, second) = if swapped {
            (&runway.secondary, &runway.primary)
        } else {
            (&runway.primary, &runway.secondary)
        };
        let end = |e: &RunwayEnd, heading_deg| Self {
            runway,
            name: e.name(),
            heading_deg,
            displaced_m: e.displaced_m,
        };
        [
            end(first, runway.heading_deg),
            end(second, (runway.heading_deg + 180.0) % 360.0),
        ]
    }

    /// Landing distance available: paved length minus the displaced threshold.
    fn lda_m(&self) -> f64 {
        self.runway.length_m - self.displaced_m
    }

    /// (east, north) of the landing threshold in `frame`:
    /// T = C - (L/2 - displaced)·(sin h, cos h).
    fn threshold(&self, frame: &Frame) -> (f64, f64) {
        let r = self.runway;
        let (cx, cy) = frame.enu(r.lat_deg, r.lon_deg);
        let (sin, cos) = self.heading_deg.to_radians().sin_cos();
        let back = r.length_m / 2.0 - self.displaced_m;
        (cx - back * sin, cy - back * cos)
    }

    /// (along, cross) metres of the origin of `frame` from the landing
    /// threshold, cross positive right of the centreline.
    fn offsets(&self, frame: &Frame) -> (f64, f64) {
        let (tx, ty) = self.threshold(frame);
        let (sin, cos) = self.heading_deg.to_radians().sin_cos();
        (-(tx * sin + ty * cos), -(tx * cos - ty * sin))
    }
}

/// Where a runway end's landing distance starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Threshold {
    pub runway: String,
    pub lat_deg: f64,
    pub lon_deg: f64,
    /// True heading of the landing direction.
    pub heading_deg: f64,
    pub lda_m: f64,
    /// Touchdown zone length: min(3000 ft, LDA/3).
    pub tdz_m: f64,
}

/// Both landing thresholds of a runway, the end its heading belongs to first.
pub fn thresholds(runway: &Runway, magvar_deg: f64) -> [Threshold; 2] {
    let frame = Frame::new(runway.lat_deg, runway.lon_deg);
    LandingEnd::of(runway, magvar_deg).map(|e| {
        let (east, north) = e.threshold(&frame);
        let (lat_deg, lon_deg) = frame.point(east, north);
        let lda_m = e.lda_m();
        Threshold {
            lda_m,
            tdz_m: tdz_m(lda_m),
            runway: e.name,
            lat_deg,
            lon_deg,
            heading_deg: e.heading_deg,
        }
    })
}

/// Touchdown position on a runway end.
#[derive(Debug, Clone, PartialEq)]
pub struct RunwayFix {
    pub icao: String,
    pub runway: String,
    /// Distance past the landing threshold (m); negative = before it.
    pub along_m: f64,
    /// Offset from the centreline (m), positive right of it.
    pub cross_m: f64,
    /// Landing distance available (m).
    pub lda_m: f64,
    /// Touchdown zone length: min(3000 ft, LDA/3).
    pub tdz_m: f64,
    /// Aiming point distance from the threshold (ICAO Annex 14).
    pub aiming_point_m: f64,
    pub in_tdz: bool,
    /// Beyond the touchdown zone.
    pub long_landing: bool,
    /// Before the threshold.
    pub short_landing: bool,
}

impl RunwayFix {
    /// Where on the runway: `In TDZ`, `Long` or `Short`.
    pub fn zone(&self) -> &'static str {
        if self.short_landing {
            "Short"
        } else if self.long_landing {
            "Long"
        } else {
            "In TDZ"
        }
    }
}

/// Matches a touchdown (position and true ground track) to a runway end:
/// among ends passing all gates, the smallest |cross| wins, which separates
/// parallel and reciprocal runways.
pub fn locate(
    airports: &[Airport],
    lat_deg: f64,
    lon_deg: f64,
    track_deg: f64,
) -> Option<RunwayFix> {
    let frame = Frame::new(lat_deg, lon_deg);
    airports
        .iter()
        .flat_map(|a| a.runways.iter().map(move |r| (a, r)))
        .flat_map(|(a, r)| LandingEnd::of(r, a.magvar_deg).map(|e| (a, e)))
        .filter_map(|(a, e)| {
            let (along_m, cross_m) = e.offsets(&frame);
            let lda_m = e.lda_m();
            let aligned = wrap180(track_deg - e.heading_deg).abs() <= TRACK_GATE_DEG;
            let on_runway = (-ALONG_BEFORE_M..=lda_m + ALONG_AFTER_M).contains(&along_m)
                && cross_m.abs() <= e.runway.width_m / 2.0 + CROSS_MARGIN_M;
            let tdz_m = tdz_m(lda_m);
            (aligned && on_runway).then(|| RunwayFix {
                icao: a.icao.clone(),
                runway: e.name,
                along_m,
                cross_m,
                lda_m,
                tdz_m,
                aiming_point_m: aiming_point_m(lda_m),
                in_tdz: (0.0..=tdz_m).contains(&along_m),
                long_landing: along_m > tdz_m,
                short_landing: along_m < 0.0,
            })
        })
        .min_by(|a, b| a.cross_m.abs().total_cmp(&b.cross_m.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fdm_core::Designator;
    use fdm_core::geo::distance_m;

    const LAT: f64 = 38.5;
    const LON: f64 = 27.0;

    fn end(number: u8, designator: Designator) -> RunwayEnd {
        RunwayEnd {
            number,
            designator,
            displaced_m: 0.0,
        }
    }

    fn runway(heading: f64, length: f64, number: u8, d: Designator) -> Runway {
        let other = if number > 18 {
            number - 18
        } else {
            number + 18
        };
        Runway {
            lat_deg: LAT,
            lon_deg: LON,
            elevation_m: 0.0,
            heading_deg: heading,
            length_m: length,
            width_m: 45.0,
            primary: end(number, d),
            secondary: end(other, d),
        }
    }

    fn airport(runways: Vec<Runway>) -> Vec<Airport> {
        vec![Airport {
            icao: "TEST".into(),
            region: String::new(),
            lat_deg: LAT,
            lon_deg: LON,
            elevation_m: 0.0,
            magvar_deg: 0.0,
            runways,
        }]
    }

    /// Point `along`/`cross` metres from a threshold lying `back` metres
    /// behind the test point on heading `h`.
    fn point(h: f64, back: f64, along: f64, cross: f64) -> (f64, f64) {
        let (s, c) = h.to_radians().sin_cos();
        let d = along - back;
        Frame::new(LAT, LON).point(d * s + cross * c, d * c - cross * s)
    }

    fn one() -> Vec<Airport> {
        airport(vec![runway(200.0, 3000.0, 20, Designator::None)])
    }

    #[test]
    fn recovers_offsets_on_both_ends() {
        let ap = one();
        let (lat, lon) = point(200.0, 1500.0, 400.0, 7.0);
        let fix = locate(&ap, lat, lon, 201.0).unwrap();
        assert_eq!(fix.runway, "20");
        assert!((fix.along_m - 400.0).abs() < 0.5 && (fix.cross_m - 7.0).abs() < 0.5);
        assert_eq!(
            (fix.lda_m, fix.tdz_m, fix.aiming_point_m),
            (3000.0, 914.4, 400.0)
        );
        assert!(fix.in_tdz && !fix.long_landing && !fix.short_landing);
        assert_eq!(fix.zone(), "In TDZ");
        let (lat, lon) = point(20.0, 1500.0, 1000.0, -5.0);
        let fix = locate(&ap, lat, lon, 20.0).unwrap();
        assert_eq!(fix.runway, "02");
        assert!((fix.along_m - 1000.0).abs() < 0.5 && (fix.cross_m + 5.0).abs() < 0.5);
        assert!(fix.long_landing);
    }

    #[test]
    fn displaced_threshold_shifts_along() {
        let mut ap = one();
        ap[0].runways[0].primary.displaced_m = 300.0;
        let (lat, lon) = point(200.0, 1200.0, 100.0, 0.0);
        let fix = locate(&ap, lat, lon, 200.0).unwrap();
        assert!((fix.along_m - 100.0).abs() < 0.5);
        assert_eq!(fix.lda_m, 2700.0);
    }

    #[test]
    fn short_runway_tdz_is_a_third() {
        let ap = airport(vec![runway(200.0, 1500.0, 20, Designator::None)]);
        let (lat, lon) = point(200.0, 750.0, 600.0, 0.0);
        let fix = locate(&ap, lat, lon, 200.0).unwrap();
        assert_eq!((fix.tdz_m, fix.aiming_point_m), (500.0, 300.0));
        assert!(fix.long_landing && !fix.in_tdz);
        assert_eq!(fix.zone(), "Long");
    }

    #[test]
    fn before_threshold_is_short_landing() {
        let (lat, lon) = point(200.0, 1500.0, -80.0, 0.0);
        let fix = locate(&one(), lat, lon, 200.0).unwrap();
        assert!(fix.short_landing && !fix.in_tdz);
        assert_eq!(fix.zone(), "Short");
    }

    #[test]
    fn rejects_off_runway_and_misaligned() {
        let ap = one();
        for (along, cross, track) in [
            (400.0, 100.0, 200.0),
            (-400.0, 0.0, 200.0),
            (400.0, 0.0, 90.0),
        ] {
            let (lat, lon) = point(200.0, 1500.0, along, cross);
            assert!(locate(&ap, lat, lon, track).is_none());
        }
    }

    #[test]
    fn parallel_runways_pick_the_nearer_centreline() {
        let half = Frame::new(LAT, LON).point(150.0, 0.0).1 - LON;
        let mut left = runway(200.0, 3000.0, 20, Designator::Left);
        let mut right = runway(200.0, 3000.0, 20, Designator::Right);
        // Facing 200 deg, right is west.
        left.lon_deg = LON + half;
        right.lon_deg = LON - half;
        let ap = airport(vec![left, right]);
        let pick = |cross: f64| {
            let (lat, lon) = point(200.0, 1500.0, 500.0, cross);
            locate(&ap, lat, lon, 200.0).unwrap().runway
        };
        assert_eq!(pick(-140.0), "20L");
        assert_eq!(pick(140.0), "20R");
    }

    #[test]
    fn crossing_runways_separate_by_track() {
        let ap = airport(vec![
            runway(200.0, 3000.0, 20, Designator::None),
            runway(290.0, 3000.0, 29, Designator::None),
        ]);
        assert_eq!(locate(&ap, LAT, LON, 201.0).unwrap().runway, "20");
        assert_eq!(locate(&ap, LAT, LON, 289.0).unwrap().runway, "29");
    }

    #[test]
    fn swaps_ends_when_heading_belongs_to_secondary() {
        let mut r = runway(200.0, 3000.0, 20, Designator::None);
        std::mem::swap(&mut r.primary, &mut r.secondary);
        let ends = LandingEnd::of(&r, 0.0).map(|e| (e.name, e.heading_deg));
        assert_eq!(ends, [("20".into(), 200.0), ("02".into(), 20.0)]);
    }

    #[test]
    fn numbers_are_matched_on_the_magnetic_heading() {
        // LTBJ 16L as MSFS reports it: true 166.74, MAGVAR 354.4 (~5.6 E).
        // The true heading would round to 17; the painted number is 16.
        let r = runway(166.74, 3240.0, 16, Designator::Left);
        let [first, _] = thresholds(&r, 354.4);
        assert_eq!(first.runway, "16L");
        assert!((magnetic_deg(166.74, 354.4) - 161.14).abs() < 1e-9);
        // North: 358 magnetic is runway 36; compass codes are not numbers.
        assert_eq!(off_number_deg(358.0, 36), Some(2.0));
        assert_eq!(off_number_deg(358.0, 40), None);
    }

    #[test]
    fn thresholds_lie_half_a_runway_from_the_centre() {
        let [t20, t02] = thresholds(&runway(200.0, 3000.0, 20, Designator::None), 0.0);
        let centre = (LAT, LON);
        assert!((distance_m(centre, (t20.lat_deg, t20.lon_deg)) - 1500.0).abs() < 0.5);
        let apart = distance_m((t20.lat_deg, t20.lon_deg), (t02.lat_deg, t02.lon_deg));
        assert!((apart - 3000.0).abs() < 0.5);
        assert!(
            t20.lat_deg > LAT,
            "landing on 200 starts north of the centre"
        );
    }

    #[test]
    fn ground_track_follows_displacement() {
        let (lat, lon) = point(200.0, 0.0, 500.0, 0.0);
        assert!((ground_track_deg((LAT, LON), (lat, lon)) - 200.0).abs() < 0.01);
        assert!((ground_track_deg((LAT, LON), (LAT, LON - 0.01)) - 270.0).abs() < 0.01);
    }
}
