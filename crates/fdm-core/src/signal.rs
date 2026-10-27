use crate::{Unit, lfl_v1};

/// Stable parameter number within an LFL, like a label on a real data frame.
/// Recordings store this id, so an id is never reused or renumbered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SignalId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Part of the FDR data frame: what a real recorder would capture.
    Frame,
    /// `sim.*` ground truth from the simulator. Kept outside the frame and
    /// used only to check values the ground segment derives.
    SimTruth,
}

/// Definition of one recorded parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignalDef {
    pub id: SignalId,
    pub name: &'static str,
    pub unit: Unit,
    pub rate_hz: f64,
    pub channel: Channel,
    /// Parameter number in 14 CFR 121 Appendix M; `None` for FOQA additions
    /// and sim truth.
    pub app_m: Option<&'static str>,
}

impl SignalDef {
    pub(crate) const fn fdr(
        id: u16,
        name: &'static str,
        unit: Unit,
        rate_hz: f64,
        app_m: &'static str,
    ) -> Self {
        Self {
            id: SignalId(id),
            name,
            unit,
            rate_hz,
            channel: Channel::Frame,
            app_m: Some(app_m),
        }
    }

    pub(crate) const fn foqa(id: u16, name: &'static str, unit: Unit, rate_hz: f64) -> Self {
        Self {
            id: SignalId(id),
            name,
            unit,
            rate_hz,
            channel: Channel::Frame,
            app_m: None,
        }
    }

    pub(crate) const fn truth(id: u16, name: &'static str, unit: Unit, rate_hz: f64) -> Self {
        Self {
            id: SignalId(id),
            name,
            unit,
            rate_hz,
            channel: Channel::SimTruth,
            app_m: None,
        }
    }
}

/// Logical Frame Layout: the dictionary that makes a recording decodable.
pub struct Lfl {
    pub version: u16,
    signals: &'static [SignalDef],
}

impl Lfl {
    pub const V1: Self = Self {
        version: 1,
        signals: lfl_v1::SIGNALS,
    };

    pub fn signals(&self) -> &'static [SignalDef] {
        self.signals
    }

    /// Ids are dense and equal to the table position, so lookup is O(1).
    pub fn get(&self, id: SignalId) -> Option<&'static SignalDef> {
        self.signals.get(usize::from(id.0))
    }

    pub fn by_name(&self, name: &str) -> Option<&'static SignalDef> {
        self.signals.iter().find(|s| s.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_dense_and_match_position() {
        for (i, s) in Lfl::V1.signals().iter().enumerate() {
            assert_eq!(usize::from(s.id.0), i, "{} is out of order", s.name);
        }
    }

    #[test]
    fn names_are_unique() {
        let mut seen = HashSet::new();
        for s in Lfl::V1.signals() {
            assert!(seen.insert(s.name), "duplicate name {}", s.name);
        }
    }

    #[test]
    fn sim_namespace_is_exactly_the_truth_channel() {
        for s in Lfl::V1.signals() {
            assert_eq!(
                s.name.starts_with("sim."),
                s.channel == Channel::SimTruth,
                "{}",
                s.name
            );
        }
    }

    #[test]
    fn rates_are_positive() {
        assert!(Lfl::V1.signals().iter().all(|s| s.rate_hz > 0.0));
    }

    #[test]
    fn lookup_by_id_and_name_agree() {
        let s = Lfl::V1.by_name("accel.normal").unwrap();
        assert_eq!(Lfl::V1.get(s.id), Some(s));
        assert_eq!((s.unit, s.rate_hz, s.app_m), (Unit::G, 8.0, Some("5")));
    }
}
