//! Runway-scale geodesy: a local east/north tangent plane using the WGS84
//! metres-per-degree series, accurate to centimetres over a few km. Shared by
//! the recorder (which airports are near) and the analyzer (runway matching).

pub struct LocalFrame {
    lat_deg: f64,
    lon_deg: f64,
    m_per_lat: f64,
    m_per_lon: f64,
}

impl LocalFrame {
    pub fn new(lat_deg: f64, lon_deg: f64) -> Self {
        let phi = lat_deg.to_radians();
        Self {
            lat_deg,
            lon_deg,
            m_per_lat: 111132.92 - 559.82 * (2.0 * phi).cos() + 1.175 * (4.0 * phi).cos(),
            m_per_lon: 111412.84 * phi.cos() - 93.5 * (3.0 * phi).cos(),
        }
    }

    /// (east, north) metres of a point from the reference.
    pub fn enu(&self, lat_deg: f64, lon_deg: f64) -> (f64, f64) {
        (
            (lon_deg - self.lon_deg) * self.m_per_lon,
            (lat_deg - self.lat_deg) * self.m_per_lat,
        )
    }

    /// (lat, lon) of a point `east`/`north` metres from the reference.
    pub fn point(&self, east: f64, north: f64) -> (f64, f64) {
        (
            self.lat_deg + north / self.m_per_lat,
            self.lon_deg + east / self.m_per_lon,
        )
    }
}

/// Distance in metres between two (lat, lon) points.
pub fn distance_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (east, north) = LocalFrame::new(a.0, a.1).enu(b.0, b.1);
    east.hypot(north)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_arcminute_of_latitude_is_a_nautical_mile() {
        let d = distance_m((38.5, 27.0), (38.5 + 1.0 / 60.0, 27.0));
        assert!((d - 1852.0).abs() / 1852.0 < 0.005, "{d}");
    }

    #[test]
    fn point_inverts_enu() {
        let f = LocalFrame::new(38.5, 27.0);
        let (lat, lon) = f.point(1200.0, -800.0);
        let (e, n) = f.enu(lat, lon);
        assert!((e - 1200.0).abs() < 1e-6 && (n + 800.0).abs() < 1e-6);
    }
}
