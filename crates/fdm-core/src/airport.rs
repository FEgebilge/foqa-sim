//! Aerodrome data as the simulator's facility database reports it: SI units,
//! unconverted. Like a navigation database snapshot on a real FDM ground
//! station; thresholds and touchdown distances are derived from it later.

#[derive(Debug, Clone, PartialEq)]
pub struct Airport {
    pub icao: String,
    pub region: String,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub elevation_m: f64,
    pub magvar_deg: f64,
    pub runways: Vec<Runway>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Runway {
    /// Centre of the full paved length.
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub elevation_m: f64,
    /// Direction of the primary end, as reported (true heading assumed;
    /// to be verified against flown ground tracks).
    pub heading_deg: f64,
    /// Full paved length including displaced thresholds, excluding blast
    /// pads and overruns.
    pub length_m: f64,
    pub width_m: f64,
    pub primary: RunwayEnd,
    pub secondary: RunwayEnd,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunwayEnd {
    /// 1–36 as painted; 37–45 are the sim's compass-point codes.
    pub number: u8,
    pub designator: Designator,
    /// Displaced threshold: paved length before the landing threshold.
    pub displaced_m: f64,
}

/// Parallel-runway letter, with the sim's codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Designator {
    None,
    Left,
    Right,
    Center,
    Water,
    A,
    B,
}

impl Designator {
    /// From the sim's code: 0 none, 1 L, 2 R, 3 C, 4 W, 5 A, 6 B.
    pub fn from_code(code: i32) -> Option<Self> {
        Some(match code {
            0 => Self::None,
            1 => Self::Left,
            2 => Self::Right,
            3 => Self::Center,
            4 => Self::Water,
            5 => Self::A,
            6 => Self::B,
            _ => return None,
        })
    }

    /// The sim's code, inverse of `from_code`.
    pub const fn code(self) -> i32 {
        self as i32
    }

    pub const fn letter(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Left => "L",
            Self::Right => "R",
            Self::Center => "C",
            Self::Water => "W",
            Self::A => "A",
            Self::B => "B",
        }
    }
}

impl RunwayEnd {
    /// Runway name as painted, e.g. `09L`.
    pub fn name(&self) -> String {
        format!("{:02}{}", self.number, self.designator.letter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_runway_ends_as_painted() {
        let end = |number, code| RunwayEnd {
            number,
            designator: Designator::from_code(code).unwrap(),
            displaced_m: 0.0,
        };
        assert_eq!(end(9, 1).name(), "09L");
        assert_eq!(end(27, 0).name(), "27");
        assert_eq!(Designator::from_code(7), None);
        assert_eq!(
            Designator::from_code(Designator::Center.code()),
            Some(Designator::Center)
        );
    }
}
