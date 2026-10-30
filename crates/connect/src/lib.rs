//! The FOQA Challenge Protocol client (docs/challenge-protocol.md, Part A
//! and B3–B6): where challenges come from and where attempts go. Optional:
//! no core crate depends on it, and the app builds without it (its
//! `connect` feature). No site is named here: the app passes its
//! built-in site in ([`sites::Policy`]); others are origins the user adds.
//!
//! - [`origin`]: what may be a site (A1 rules).
//! - [`discovery`]: the well-known document.
//! - [`auth`]: loopback + PKCE sign-in and the device flow (A2).
//! - [`session`]: tokens per site and the API calls (A4).
//! - [`sites`]: the user's sites, `sites.json` and the connect log.
//! - [`sidecar`] and [`uploader`]: an attempt's site state and its upload
//!   queue (B5, B6).
//! - [`copy`]: what every status and error says to the pilot (A6).
//! - [`deeplink`]: links from websites, parsed strictly, and the consent
//!   gate (A3, B2).

pub mod api;
pub mod auth;
pub mod copy;
pub mod deeplink;
pub mod discovery;
pub mod http;
pub mod origin;
pub mod session;
pub mod sidecar;
pub mod sites;
pub mod store;
pub mod uploader;

mod jsonl;
#[cfg(test)]
mod mock;

/// `client_id` in every auth call.
pub const CLIENT_ID: &str = "foqa-sim";
/// The protocol version this client speaks.
pub const PROTOCOL: u32 = 1;

/// Milliseconds since the Unix epoch: deadlines survive restarts.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub(crate) fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
