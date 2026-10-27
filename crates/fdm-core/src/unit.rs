/// Engineering unit of a parameter. Values are stored as `f64` in the unit
/// declared by the LFL; conversion from sim units happens once, at ingest.
/// Aviation units are canonical so rule thresholds read like the sources
/// (e.g. `VS <= 1000 fpm`, `VAPP + 10 kt`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Unit {
    Feet,
    Knots,
    Degrees,
    /// Acceleration in multiples of standard gravity.
    G,
    FeetPerMinute,
    /// ILS deviation in indicator dots.
    Dots,
    Percent,
    Celsius,
    Hectopascal,
    Pounds,
    Seconds,
    /// Dimensionless ratio, e.g. simulation rate.
    Ratio,
    /// Two-state value recorded as 0.0 / 1.0.
    Discrete,
    /// Detent position, e.g. flap handle index.
    Index,
}

impl Unit {
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Feet => "ft",
            Self::Knots => "kt",
            Self::Degrees => "deg",
            Self::G => "g",
            Self::FeetPerMinute => "fpm",
            Self::Dots => "dot",
            Self::Percent => "%",
            Self::Celsius => "°C",
            Self::Hectopascal => "hPa",
            Self::Pounds => "lb",
            Self::Seconds => "s",
            Self::Ratio | Self::Discrete | Self::Index => "",
        }
    }
}
