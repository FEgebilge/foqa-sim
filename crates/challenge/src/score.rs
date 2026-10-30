//! A challenge attempt's score: always the debrief's current landing
//! assessment (`CURRENT`), whatever method its definition names, so a
//! challenge and the debrief never grade one landing two ways. The
//! definition's `scoring.version` is only checked to be a known name;
//! `Score::version` says which method graded.

use serde::Serialize;

/// The shared landing assessment (docs/scoring.md), crates/assess.
pub const LANDING_3: &str = "landing-3";
/// The method every attempt is graded with.
pub const CURRENT: &str = LANDING_3;
/// Retired (0.10): its measures changed in code, so it cannot be
/// reproduced; a challenge naming it is graded with `CURRENT`.
pub const LANDING_2: &str = "landing-2";
/// Retired (0.11.3): the first challenge score (weighted curves per
/// class); a challenge naming it is graded with `CURRENT`.
pub const LANDING_1: &str = "landing-1";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Score {
    pub version: &'static str,
    /// 0 to 100, to 0.1: the assessment's score, its caps applied.
    pub total: f64,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Part {
    pub name: &'static str,
    /// Points at a perfect score.
    pub weight: f64,
    /// 0 to 1.
    pub score: f64,
    /// What was measured, in `unit`.
    pub value: Option<f64>,
    pub unit: &'static str,
}
