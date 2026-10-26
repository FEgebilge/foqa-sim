//! Aircraft-segment bus interface: our own SimConnect client.
//!
//! `ffi` mirrors the subset of `SimConnect.h` we use; `client` wraps it in a
//! safe API. All `unsafe` lives in these two modules. `facility` turns
//! facility-database replies into `fdm_core::Airport` and fetches them
//! without blocking.

mod client;
pub mod facility;
mod ffi;

pub use client::{AirportListEntry, Error, Facility, FacilityData, Recv, SimConnect};
