//! Rule engine: event definitions in YAML with a unit-checked expression
//! language (`expr`), loaded and validated up front (`rule`) and evaluated
//! over an analysed flight (`engine`), with the limits of the aircraft type
//! from its profile (`profile`). Every event names its rule and version, the
//! threshold it crossed and the value that crossed it.

#![forbid(unsafe_code)]

mod engine;
pub mod expr;
mod profile;
mod rule;

pub use engine::{Event, aircraft, evaluate};
pub use profile::{
    FLAP_TOLERANCE_DEG, FlapBand, PhaseField, Profile, ProfileError, ProfileField, ProfileSet,
    Selection, Sourced, class_phases,
};
pub use rule::{Aircraft, Level, Limits, LoadError, Rule, Side, TdField, Trigger, Var, load};
