use crate::{SignalId, SimTime};

/// Validity of a sample, modelled on the ARINC 429 sign/status matrix.
/// A glideslope deviation with no glideslope received is `NoComputedData`,
/// not zero: rules must not treat it as "on the glideslope".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Ssm {
    #[default]
    Normal,
    NoComputedData,
    Failure,
}

/// One timestamped value of one parameter, in the parameter's engineering
/// unit. Self-describing in the ARINC 767 sense: each sample carries its own
/// time and parameter id, so parameters can be recorded at different rates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub t: SimTime,
    pub signal: SignalId,
    pub value: f64,
    pub ssm: Ssm,
}

impl Sample {
    pub const fn new(t: SimTime, signal: SignalId, value: f64) -> Self {
        Self {
            t,
            signal,
            value,
            ssm: Ssm::Normal,
        }
    }

    pub const fn is_valid(&self) -> bool {
        matches!(self.ssm, Ssm::Normal)
    }
}
