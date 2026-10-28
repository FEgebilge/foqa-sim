//! Ground segment. Mirrors FDM software: decode the recording against its
//! LFL (`flight`), calibrate and derive parameters on a common time base
//! (`track`, `vs`), then find flight phases (`phase`), touchdowns
//! (`landing`) and the approach and rollout values a landing is graded
//! on (`approach`). Runs after the flight, so it may look ahead in time;
//! `live` follows a flight still being flown, for the in-sim panel.

#![forbid(unsafe_code)]

mod approach;
#[cfg(feature = "parquet")]
pub mod export;
mod flight;
mod landing;
mod live;
mod phase;
mod runs;
mod runway;
mod track;
mod vs;

pub use approach::{
    Approach, Config, ConfigChange, Crossing, Slowed, Window, held, stable_from, sustained_worst,
};
pub use flight::Flight;
pub use landing::{Touchdown, touchdowns};
pub use live::{LiveInput, LivePoint, LiveTrack};
pub use phase::{
    Basis, Cause, Phase, PhaseConfig, Span, Trigger, phase_labels, phases, rotation_rate,
};
pub use runs::runs;
pub use runway::{
    RunwayFix, Threshold, ground_track_deg, locate, magnetic_deg, thresholds, wind_components,
};
pub use track::{Leg, TRACK_HZ, Track};
