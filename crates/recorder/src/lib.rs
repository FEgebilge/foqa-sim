//! Aircraft segment. Mirrors the on-board chain: sensors are wired to
//! parameters (`acquisition`), the FDAU samples them at LFL rates with a
//! high-resolution touchdown window (`fdau`), and the result is journaled.
//! Pure Rust: the live SimConnect source plugs in through `TelemetrySource`.

#![forbid(unsafe_code)]

mod acquisition;
mod aircraft_json;
mod airport_json;
mod compact;
mod fdau;
mod journal;
mod marker_json;
mod pipeline;
mod raw;
mod slice;
mod source;

pub use acquisition::{Acquisition, Column, Conditioned};
pub use compact::{EXT as COMPACT_EXT, FlightRecords, Recording, compact_file, read_records};
pub use fdau::{Fdau, TOUCHDOWN_WINDOW_US};
pub use journal::{JournalReader, JournalWriter, Record};
pub use pipeline::Recorder;
pub use raw::{RawWriter, ReplaySource};
pub use slice::{AFTER_US, BEFORE_US, Cut, Slice, slice};
pub use source::{
    AttemptMarker, AttemptPhase, FlowEvent, SYSTEM_EVENTS, SimEvent, Telemetry, TelemetrySource,
    is_reference,
};
