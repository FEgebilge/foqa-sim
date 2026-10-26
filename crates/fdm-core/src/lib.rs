//! Core flight-data types shared by the recorder (aircraft segment) and the
//! analyzer (ground segment). Pure data: no I/O and no wall-clock reads, so
//! every analysis is reproducible from a recording alone.

#![forbid(unsafe_code)]

mod aircraft;
mod airport;
pub mod geo;
mod lfl_v1;
mod sample;
mod signal;
mod time;
mod unit;

pub use aircraft::AircraftId;
pub use airport::{Airport, Designator, Runway, RunwayEnd};
pub use sample::{Sample, Ssm};
pub use signal::{Channel, Lfl, SignalDef, SignalId};
pub use time::SimTime;
pub use unit::Unit;
