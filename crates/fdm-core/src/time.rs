/// Monotonic simulation time since the start of a recording, in microseconds.
///
/// Derived from the sim's own clock, so it stops while paused and follows the
/// simulation rate: durations in rules ("sustained for 3 s") are measured in
/// the time the aircraft experienced. Zulu time is recorded as a separate
/// parameter, as on a real FDR.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimTime(i64);

impl SimTime {
    pub const ZERO: Self = Self(0);

    pub const fn from_micros(us: i64) -> Self {
        Self(us)
    }

    pub fn from_secs_f64(secs: f64) -> Self {
        Self((secs * 1e6).round() as i64)
    }

    pub const fn as_micros(self) -> i64 {
        self.0
    }

    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / 1e6
    }

    /// Microseconds from `earlier` to `self` (negative if `earlier` is later).
    pub const fn us_since(self, earlier: Self) -> i64 {
        self.0 - earlier.0
    }

    /// This time shifted by `us` microseconds, held at the ends of the
    /// range: a crafted time near them never wraps.
    pub const fn add_us(self, us: i64) -> Self {
        Self(self.0.saturating_add(us))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_round_trip_at_microsecond_resolution() {
        let t = SimTime::from_secs_f64(12.345_678);
        assert_eq!(t.as_micros(), 12_345_678);
        assert_eq!(t.as_secs_f64(), 12.345_678);
    }

    #[test]
    fn orders_chronologically() {
        assert!(SimTime::from_micros(1) > SimTime::ZERO);
    }

    #[test]
    fn measures_and_shifts_in_microseconds() {
        let t = SimTime::from_micros(5_000_000);
        assert_eq!(t.us_since(SimTime::from_micros(2_000_000)), 3_000_000);
        assert_eq!(t.add_us(-500_000), SimTime::from_micros(4_500_000));
        let last = SimTime::from_micros(i64::MAX - 1);
        assert_eq!(last.add_us(5_000_000), SimTime::from_micros(i64::MAX));
        assert_eq!(
            SimTime::from_micros(i64::MIN + 1).add_us(-10_000_000),
            SimTime::from_micros(i64::MIN)
        );
    }
}
