//! Landing assessment `landing-2` (docs/scoring.md): every approach and
//! landing graded in six domains against published tolerances and FDM
//! event thresholds, each measured value placed on one 0-3 scale, each
//! domain losing by its worst item, then capped by safety outcomes. The
//! method is data (`scoring/landing-2.yaml`, `method`), the scale is
//! `scale`, the measures of a recorded landing come from `measure` and the
//! score from `score`; the debrief and challenges share it.

#![forbid(unsafe_code)]

mod measure;
mod method;
mod scale;
mod score;
mod text;

pub use measure::{Assessment, Landing, SpeedSource, Speeds, scoring_class};
pub use method::{
    ANCHOR_ELL, Additive, Anchor, Band, CapDef, Caps, Context, Criterion, DOMAINS, Height, Method,
    MethodError, WindowDef,
};
pub use scale::{ell, level, resolve, side_ell, sides};
pub use score::{
    Cap, CapKind, Confidence, CriterionScore, DomainScore, Finding, FindingScore, FlightGrade,
    Grade, LandingScore, Measures, Outcome, Value,
};
pub use text::{height_text, item_text, list, number, reading_text, value_text, with_level};
