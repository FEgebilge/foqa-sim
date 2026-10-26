//! Rule files: YAML documents (separated by `---`), validated and compiled at
//! load time so a bad rule fails before any flight is analysed.

use std::collections::HashSet;
use std::fmt;

use analyzer::Phase;
use fdm_core::{Lfl, SignalId, Unit};
use serde::Deserialize;

use crate::expr::{self, Expr, Scope, Ty};
use crate::profile::ProfileField;

/// Event severity, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Advisory,
    Low,
    Medium,
    High,
}

impl Level {
    /// Every level, lowest first.
    pub const ALL: [Self; 4] = [Self::Advisory, Self::Low, Self::Medium, Self::High];
}

/// Aircraft a rule applies to. Transport means a maximum weight over
/// 12,500 lb, the FAA small-aircraft limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Aircraft {
    Light,
    Transport,
}

impl Aircraft {
    /// FAA small-aircraft limit: heavier aircraft are transport category.
    const TRANSPORT_MIN_LB: f64 = 12_500.0;

    /// The class of an aircraft of this gross weight.
    pub fn by_weight(lb: f64) -> Self {
        if lb > Self::TRANSPORT_MIN_LB {
            Self::Transport
        } else {
            Self::Light
        }
    }
}

impl fmt::Display for Aircraft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Light => "light",
            Self::Transport => "transport",
        })
    }
}

/// What a rule expression can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Var {
    Param(SignalId),
    /// Inertial vertical speed, fpm.
    Vs,
    /// Height above the aerodrome, ft.
    Height,
    /// Calibrated radio height, ft.
    Ra,
    Td(TdField),
    /// A value of the flight's type profile; unknown without one.
    Profile(ProfileField),
}

/// Touchdown values, available in `at: touchdown` rules as `td.*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TdField {
    Sink,
    PeakG,
    Pitch,
    Roll,
    Ias,
    Bounce,
    /// The runway values need the airport snapshot recorded in flight; they
    /// are unknown without a matched runway.
    Distance,
    Cross,
    Lda,
    Tdz,
    InTdz,
    LongLanding,
    ShortLanding,
}

/// When a rule is evaluated.
#[derive(Debug, Clone)]
pub enum Trigger {
    /// Every point of the applicable phases; the exceedance must last
    /// `sustain_us`.
    Continuous { sustain_us: i64 },
    /// Once, where the gate condition first becomes true (e.g. 500 ft).
    Gate(Expr<Var>),
    /// Once per touchdown.
    Touchdown,
}

/// Which way a threshold list is crossed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Above,
    Below,
}

impl Side {
    /// `1` where larger is worse, `-1` where smaller is: multiply a value by
    /// it to compare "further out".
    pub fn sign(self) -> f64 {
        match self {
            Self::Above => 1.0,
            Self::Below => -1.0,
        }
    }

    /// The YAML field of this side.
    fn name(self) -> &'static str {
        match self {
            Self::Above => "above",
            Self::Below => "below",
        }
    }
}

/// What a rule's measure is compared against, and the level it raises.
#[derive(Debug, Clone)]
pub enum Limits {
    /// A condition measure: the event has this level whenever it holds.
    Fixed(Level),
    /// A numeric measure: exceeding `above` (or going under `below`) a
    /// level's threshold raises that level. Sorted by level.
    Thresholds {
        above: Vec<(Level, f64)>,
        below: Vec<(Level, f64)>,
        unit: Option<Unit>,
    },
}

impl Limits {
    /// The highest level `value` crosses on `side`, with its threshold;
    /// `None` if it crosses none (or `value` is `NaN`, or the limits are
    /// `Fixed`).
    pub fn level_at(&self, side: Side, value: f64) -> Option<(Level, f64)> {
        let Self::Thresholds { above, below, .. } = self else {
            return None;
        };
        let levels = if side == Side::Above { above } else { below };
        let sign = side.sign();
        levels
            .iter()
            .rev()
            .find(|&&(_, thr)| sign * value > sign * thr)
            .copied()
    }

    /// The most severe level `value` raises, with its threshold (none for
    /// a condition, which holds from 0.5).
    pub fn crossed(&self, value: f64) -> Option<(Level, Option<f64>)> {
        match self {
            Self::Fixed(level) => (value >= 0.5).then_some((*level, None)),
            Self::Thresholds { .. } => [Side::Above, Side::Below]
                .into_iter()
                .filter_map(|side| self.level_at(side, value))
                .max_by_key(|&(level, _)| level)
                .map(|(level, thr)| (level, Some(thr))),
        }
    }

    /// The lowest level's threshold on `side`, where events start.
    pub fn first(&self, side: Side) -> Option<f64> {
        match self {
            Self::Thresholds { above, below, .. } => {
                let levels = if side == Side::Above { above } else { below };
                levels.first().map(|&(_, thr)| thr)
            }
            Self::Fixed(_) => None,
        }
    }

    pub fn unit(&self) -> Option<Unit> {
        match self {
            Self::Thresholds { unit, .. } => *unit,
            Self::Fixed(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    pub version: u32,
    pub title: String,
    pub description: String,
    /// Plain-language consequence of an exceedance, for pilots.
    pub why_it_matters: Option<String>,
    /// Plain-language technique to avoid it.
    pub advice: Option<String>,
    /// Empty for touchdown rules.
    pub phases: Vec<Phase>,
    pub aircraft: Option<Aircraft>,
    pub when: Option<Expr<Var>>,
    pub trigger: Trigger,
    pub measure: Expr<Var>,
    /// The measure as written, for explaining events.
    pub measure_src: String,
    pub limits: Limits,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadError {
    pub rule: Option<String>,
    pub field: &'static str,
    pub message: String,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.rule {
            Some(rule) => write!(f, "rule `{rule}`, `{}`:\n{}", self.field, self.message),
            None => write!(f, "{}: {}", self.field, self.message),
        }
    }
}

impl std::error::Error for LoadError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Doc {
    rule: String,
    version: u32,
    title: String,
    #[serde(default)]
    description: String,
    why_it_matters: Option<String>,
    advice: Option<String>,
    phase: Option<OneOrMany>,
    aircraft: Option<Aircraft>,
    when: Option<String>,
    gate: Option<String>,
    at: Option<String>,
    measure: String,
    sustain: Option<Scalar>,
    above: Option<Levels>,
    below: Option<Levels>,
    level: Option<Level>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    fn iter(&self) -> impl Iterator<Item = &String> {
        match self {
            Self::One(one) => std::slice::from_ref(one),
            Self::Many(many) => many,
        }
        .iter()
    }
}

/// A YAML scalar: `30 deg` arrives as text, `30` as a number.
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum Scalar {
    Num(f64),
    Text(String),
}

impl fmt::Display for Scalar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Num(x) => write!(f, "{x}"),
            Self::Text(s) => f.write_str(s),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Levels {
    advisory: Option<Scalar>,
    low: Option<Scalar>,
    medium: Option<Scalar>,
    high: Option<Scalar>,
}

/// Loads every rule in a YAML file.
pub fn load(yaml: &str) -> Result<Vec<Rule>, LoadError> {
    let mut rules = Vec::new();
    let mut ids = HashSet::new();
    for doc in serde_yaml_ng::Deserializer::from_str(yaml) {
        let doc = Doc::deserialize(doc).map_err(|e| LoadError {
            rule: None,
            field: "yaml",
            message: e.to_string(),
        })?;
        let rule = compile(doc)?;
        if !ids.insert(rule.id.clone()) {
            return Err(LoadError {
                rule: Some(rule.id),
                field: "rule",
                message: "duplicate rule id".into(),
            });
        }
        rules.push(rule);
    }
    Ok(rules)
}

struct RuleScope {
    touchdown: bool,
}

impl Scope for RuleScope {
    type Var = Var;

    fn resolve(&self, name: &str) -> Option<(Var, Ty)> {
        let num = |unit| Ty::Num(Some(unit));
        if let Some(field) = name.strip_prefix("profile.") {
            let field = ProfileField::named(field)?;
            return Some((Var::Profile(field), num(field.unit())));
        }
        Some(match name {
            "vs" => (Var::Vs, num(Unit::FeetPerMinute)),
            "height" => (Var::Height, num(Unit::Feet)),
            "ra" => (Var::Ra, num(Unit::Feet)),
            "td.sink" if self.touchdown => (Var::Td(TdField::Sink), num(Unit::FeetPerMinute)),
            "td.peak_g" if self.touchdown => (Var::Td(TdField::PeakG), num(Unit::G)),
            "td.pitch" if self.touchdown => (Var::Td(TdField::Pitch), num(Unit::Degrees)),
            "td.roll" if self.touchdown => (Var::Td(TdField::Roll), num(Unit::Degrees)),
            "td.ias" if self.touchdown => (Var::Td(TdField::Ias), num(Unit::Knots)),
            "td.bounce" if self.touchdown => (Var::Td(TdField::Bounce), Ty::Bool),
            "td.distance" if self.touchdown => (Var::Td(TdField::Distance), num(Unit::Feet)),
            "td.cross" if self.touchdown => (Var::Td(TdField::Cross), num(Unit::Feet)),
            "td.lda" if self.touchdown => (Var::Td(TdField::Lda), num(Unit::Feet)),
            "td.tdz" if self.touchdown => (Var::Td(TdField::Tdz), num(Unit::Feet)),
            "td.in_tdz" if self.touchdown => (Var::Td(TdField::InTdz), Ty::Bool),
            "td.long_landing" if self.touchdown => (Var::Td(TdField::LongLanding), Ty::Bool),
            "td.short_landing" if self.touchdown => (Var::Td(TdField::ShortLanding), Ty::Bool),
            name => {
                let def = Lfl::V1.by_name(name)?;
                (Var::Param(def.id), Ty::of(def.unit))
            }
        })
    }
}

/// Compiles the parts of one document, attributing every error to the rule
/// and field.
struct Compiler<'a> {
    id: &'a str,
    scope: RuleScope,
}

impl<'a> Compiler<'a> {
    fn new(doc: &'a Doc) -> Result<Self, LoadError> {
        let mut compiler = Self {
            id: &doc.rule,
            scope: RuleScope { touchdown: false },
        };
        match doc.at.as_deref() {
            None => {}
            Some("touchdown") => compiler.scope.touchdown = true,
            Some(other) => {
                return Err(compiler.err(
                    "at",
                    format!("unknown trigger `{other}`; the only one is `touchdown`"),
                ));
            }
        }
        Ok(compiler)
    }

    fn err(&self, field: &'static str, message: impl Into<String>) -> LoadError {
        LoadError {
            rule: Some(self.id.to_owned()),
            field,
            message: message.into(),
        }
    }

    fn expression(&self, field: &'static str, src: &str) -> Result<Expr<Var>, LoadError> {
        expr::compile(src, &self.scope).map_err(|e| self.err(field, e.render(src)))
    }

    fn condition(&self, field: &'static str, src: &str) -> Result<Expr<Var>, LoadError> {
        let e = self.expression(field, src)?;
        match e.ty() {
            Ty::Bool => Ok(e),
            ty => Err(self.err(field, format!("{src}\nmust be a condition, but is {ty}"))),
        }
    }

    fn phases(&self, doc: &Doc) -> Result<Vec<Phase>, LoadError> {
        let phases = doc
            .phase
            .iter()
            .flat_map(OneOrMany::iter)
            .map(|p| p.parse().map_err(|e: String| self.err("phase", e)))
            .collect::<Result<Vec<Phase>, _>>()?;
        if phases.is_empty() && !self.scope.touchdown {
            return Err(self.err("phase", "a rule needs the phases it applies to"));
        }
        Ok(phases)
    }

    fn trigger(&self, doc: &Doc) -> Result<Trigger, LoadError> {
        match (self.scope.touchdown, &doc.gate, &doc.sustain) {
            (true, None, None) => Ok(Trigger::Touchdown),
            (false, Some(gate), None) => self.condition("gate", gate).map(Trigger::Gate),
            (false, None, sustain) => Ok(Trigger::Continuous {
                sustain_us: sustain.as_ref().map_or(Ok(0), |s| self.duration_us(s))?,
            }),
            _ => Err(self.err("gate", "use at most one of `at`, `gate` and `sustain`")),
        }
    }

    /// A `sustain` value in microseconds: `2 s`, or a plain `0`.
    fn duration_us(&self, sustain: &Scalar) -> Result<i64, LoadError> {
        match expr::literal(&sustain.to_string()) {
            Ok((0.0, None)) => Ok(0),
            Ok((secs, Some(Unit::Seconds))) if secs >= 0.0 => Ok((secs * 1e6).round() as i64),
            _ => Err(self.err(
                "sustain",
                format!("`{sustain}` is not a duration like `2 s`"),
            )),
        }
    }

    fn limits(&self, measure: &Expr<Var>, doc: &Doc) -> Result<Limits, LoadError> {
        match (measure.ty(), doc.level, &doc.above, &doc.below) {
            (Ty::Bool, Some(level), None, None) => Ok(Limits::Fixed(level)),
            (Ty::Bool, ..) => {
                Err(self.err("level", "a condition measure takes `level`, not thresholds"))
            }
            (Ty::Num(_), Some(_), ..) => Err(self.err(
                "level",
                "a numeric measure takes `above`/`below` thresholds",
            )),
            (Ty::Num(_), None, None, None) => {
                Err(self.err("above", "a numeric measure needs `above` or `below`"))
            }
            (Ty::Num(unit), None, above, below) => {
                let side = |side: Side, levels: &Option<Levels>| {
                    levels.as_ref().map_or(Ok(vec![]), |l| {
                        thresholds(l, unit, side).map_err(|m| self.err(side.name(), m))
                    })
                };
                Ok(Limits::Thresholds {
                    above: side(Side::Above, above)?,
                    below: side(Side::Below, below)?,
                    unit,
                })
            }
        }
    }
}

fn compile(doc: Doc) -> Result<Rule, LoadError> {
    let c = Compiler::new(&doc)?;
    let phases = c.phases(&doc)?;
    let trigger = c.trigger(&doc)?;
    let measure = c.expression("measure", &doc.measure)?;
    let limits = c.limits(&measure, &doc)?;
    let when = doc
        .when
        .as_deref()
        .map(|w| c.condition("when", w))
        .transpose()?;
    Ok(Rule {
        id: doc.rule,
        version: doc.version,
        title: doc.title,
        description: doc.description,
        why_it_matters: doc.why_it_matters,
        advice: doc.advice,
        phases,
        aircraft: doc.aircraft,
        when,
        trigger,
        measure,
        measure_src: doc.measure,
        limits,
    })
}

/// The thresholds of one side. They must be strictly further out with each
/// level, or a higher level could never be reached or would be shadowed by a
/// lower one.
fn thresholds(
    levels: &Levels,
    unit: Option<Unit>,
    side: Side,
) -> Result<Vec<(Level, f64)>, String> {
    let given = [&levels.advisory, &levels.low, &levels.medium, &levels.high];
    let sign = side.sign();
    let mut out = Vec::new();
    let mut last = None;
    for (level, scalar) in Level::ALL.into_iter().zip(given) {
        let Some(scalar) = scalar else { continue };
        let text = scalar.to_string();
        let value = expr::quantity(&text, unit, &format!("{level:?} threshold"), "the measure")?;
        if let Some((prev, prev_value, prev_text)) = &last
            && sign * value <= sign * prev_value
        {
            let way = side.name();
            return Err(format!(
                "{level:?} threshold `{text}` must be {way} {prev:?} `{prev_text}`"
            ));
        }
        out.push((level, value));
        last = Some((level, value, text));
    }
    if out.is_empty() {
        return Err("needs at least one level (advisory, low, medium or high)".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BANK: &str = "
rule: excessive-bank-low
version: 1
title: Excessive bank below 1000 ft
phase: [InitialClimb, Approach]
when: height < 1000 ft
measure: abs(roll)
sustain: 2 s
above: { low: 30 deg, medium: 35 deg, high: 45 deg }
";

    fn error(yaml: &str) -> String {
        load(yaml).unwrap_err().to_string()
    }

    #[test]
    fn loads_a_continuous_rule() {
        let rules = load(BANK).unwrap();
        let r = &rules[0];
        assert_eq!((r.id.as_str(), r.version), ("excessive-bank-low", 1));
        assert_eq!(r.phases, [Phase::InitialClimb, Phase::Approach]);
        assert!(matches!(
            r.trigger,
            Trigger::Continuous {
                sustain_us: 2_000_000
            }
        ));
        let Limits::Thresholds { above, below, unit } = &r.limits else {
            panic!()
        };
        assert_eq!(
            above,
            &[
                (Level::Low, 30.0),
                (Level::Medium, 35.0),
                (Level::High, 45.0)
            ]
        );
        assert!(below.is_empty());
        assert_eq!(*unit, Some(Unit::Degrees));
    }

    #[test]
    fn reports_unit_errors_with_rule_and_field() {
        let yaml = BANK.replace("height < 1000 ft", "height < 1000 fpm");
        assert_eq!(
            error(&yaml),
            "rule `excessive-bank-low`, `when`:\nheight < 1000 fpm\n         ^^^^^^^^ compares ft with fpm"
        );
        let yaml = BANK.replace("high: 45 deg", "high: 45");
        assert!(
            error(&yaml).contains("High threshold `45` is a plain number, but the measure is deg")
        );
    }

    #[test]
    fn touchdown_values_exist_only_in_touchdown_rules() {
        let yaml = "rule: x\nversion: 1\ntitle: x\nphase: Landing\nmeasure: td.peak_g\nabove: { low: 1.6 g }";
        assert!(error(yaml).contains("unknown parameter `td.peak_g`"));
        let yaml = "rule: x\nversion: 1\ntitle: x\nat: touchdown\nmeasure: td.peak_g\nabove: { low: 1.6 g }";
        assert!(matches!(load(yaml).unwrap()[0].trigger, Trigger::Touchdown));
    }

    #[test]
    fn thresholds_must_follow_the_levels() {
        let yaml = BANK.replace("medium: 35 deg", "medium: 30 deg");
        assert_eq!(
            error(&yaml),
            "rule `excessive-bank-low`, `above`:
Medium threshold `30 deg` must be above Low `30 deg`"
        );
        let below = |levels: &str| {
            format!(
                "{BANK}below: {levels}
"
            )
        };
        assert!(
            error(&below("{ low: -5 deg, high: -5 deg }"))
                .contains("High threshold `-5 deg` must be below Low `-5 deg`")
        );
        assert!(load(&below("{ low: -5 deg, high: -9 deg }")).is_ok());
    }

    #[test]
    fn a_threshold_map_needs_a_level() {
        let yaml = BANK.replace("{ low: 30 deg, medium: 35 deg, high: 45 deg }", "{}");
        assert_eq!(
            error(&yaml),
            "rule `excessive-bank-low`, `above`:
needs at least one level (advisory, low, medium or high)"
        );
    }

    #[test]
    fn rejects_duplicates_and_unknown_fields() {
        assert!(error(&format!("{BANK}---{BANK}")).contains("duplicate rule id"));
        assert!(error(&format!("{BANK}severity: high\n")).contains("unknown field `severity`"));
    }
}
