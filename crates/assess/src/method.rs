//! A scoring method as data (`scoring/<method>.yaml`): weights, the loss
//! curve, caps, approach windows and each criterion's anchors, validated
//! when loaded, the way rules are. Anchors are unit-checked expressions;
//! `levels_from` takes L1-L3 from a rule so a threshold has one home.

use std::collections::HashMap;
use std::fmt;

use fdm_core::Unit;
use rules::expr::{self, Expr, Scope, Ty, Values};
use rules::{Domain, Level, Limits, Rule, Scalar, ScoringClass, Side};
use serde::Deserialize;

/// The shipped `landing-3` and the core rules it takes levels from.
const LANDING_3: &str = include_str!("../../../scoring/landing-3.yaml");
const CORE_RULES: &str = include_str!("../../../rules/core.yaml");

/// The scored domains, in their order on the page.
pub const DOMAINS: [Domain; 6] = [
    Domain::Stability,
    Domain::Energy,
    Domain::Point,
    Domain::Lateral,
    Domain::Contact,
    Domain::Rollout,
];

/// The levels of the scale, at the anchors ideal, tolerance, L1, L2, L3.
pub const ANCHOR_ELL: [f64; 5] = [0.0, 0.5, 1.0, 2.0, 3.0];

/// A name an anchor may use (`scoring/landing-3.yaml` header).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Var {
    Add,
    Gust,
    Rod3,
    Target,
    Tdz,
    Lda,
    Width,
    Track,
    HardG,
}

impl Var {
    const ALL: [(Self, &'static str, Unit); 9] = [
        (Self::Add, "add", Unit::Knots),
        (Self::Gust, "gust", Unit::Knots),
        (Self::Rod3, "rod3", Unit::FeetPerMinute),
        (Self::Target, "target", Unit::Feet),
        (Self::Tdz, "tdz", Unit::Feet),
        (Self::Lda, "lda", Unit::Feet),
        (Self::Width, "width", Unit::Feet),
        (Self::Track, "track", Unit::Feet),
        (Self::HardG, "hard_g", Unit::G),
    ];
}

struct AnchorScope;

impl Scope for AnchorScope {
    type Var = Var;

    fn resolve(&self, name: &str) -> Option<(Var, Ty)> {
        let (var, _, unit) = Var::ALL.into_iter().find(|v| v.1 == name)?;
        Some((var, Ty::Num(Some(unit))))
    }
}

/// The values of the anchor names for one landing; `None` where unknown.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Context {
    pub add_kt: Option<f64>,
    pub gust_kt: Option<f64>,
    pub rod3_fpm: Option<f64>,
    pub target_ft: Option<f64>,
    pub tdz_ft: Option<f64>,
    pub lda_ft: Option<f64>,
    pub width_ft: Option<f64>,
    pub track_ft: Option<f64>,
    pub hard_g: Option<f64>,
}

impl Values<Var> for Context {
    fn get(&self, var: Var) -> Option<f64> {
        match var {
            Var::Add => self.add_kt,
            Var::Gust => self.gust_kt,
            Var::Rod3 => self.rod3_fpm,
            Var::Target => self.target_ft,
            Var::Tdz => self.tdz_ft,
            Var::Lda => self.lda_ft,
            Var::Width => self.width_ft,
            Var::Track => self.track_ft,
            Var::HardG => self.hard_g,
        }
    }
}

/// One anchor: its expression, and the level threshold of the rule named
/// in `levels_from`, used where the expression is absent or unknown.
#[derive(Debug, Clone, Default)]
pub struct Anchor {
    expr: Option<Expr<Var>>,
    from_rule: Option<f64>,
}

impl Anchor {
    pub fn at(&self, ctx: &Context) -> Option<f64> {
        self.expr
            .as_ref()
            .and_then(|e| e.eval(ctx))
            .or(self.from_rule)
    }
}

/// Where in the approach window a band applies: everywhere, or at or
/// above, or below, the window's split height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Height {
    All,
    Above,
    Below,
}

#[derive(Debug, Clone)]
pub struct Band {
    pub classes: Vec<ScoringClass>,
    pub height: Height,
    /// [ideal, tolerance, L1, L2, L3] beyond which larger is worse.
    pub above: [Anchor; 5],
    /// The same where smaller is worse.
    pub below: [Anchor; 5],
}

#[derive(Debug, Clone)]
pub struct Criterion {
    pub id: String,
    pub title: String,
    pub domain: Domain,
    pub unit: Option<Unit>,
    /// How long a series value must be held (0: scalar values).
    pub sustain_us: i64,
    /// Values at or above the first, in the criterion's unit, need only be
    /// held the second, in us (S1: 3 s past the second descent threshold).
    pub beyond: Option<(f64, i64)>,
    pub source: String,
    pub bands: Vec<Band>,
}

impl Criterion {
    /// The band for `class` at `height`; a band for all heights serves
    /// either half.
    pub fn band(&self, class: ScoringClass, height: Height) -> Option<&Band> {
        self.bands
            .iter()
            .find(|b| b.classes.contains(&class) && (b.height == Height::All || b.height == height))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CapDef {
    pub score: f64,
    pub source: String,
}

/// The approach window of a scoring class, ft above the runway; without a
/// `bottom` it ends at the flight's flare height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowDef {
    pub top_ft: f64,
    pub split_ft: f64,
    pub bottom_ft: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Additive {
    pub min_kt: f64,
    pub max_kt: f64,
    pub autothrottle_kt: f64,
}

/// A loaded scoring method.
#[derive(Debug, Clone)]
pub struct Method {
    pub name: String,
    /// (ell, loss) points.
    pub loss: (Vec<f64>, Vec<f64>),
    pub good: f64,
    pub fair: f64,
    pub weights: Vec<(Domain, f64)>,
    pub caps: Caps,
    pub windows: HashMap<ScoringClass, WindowDef>,
    pub fallback_track_ft: HashMap<ScoringClass, f64>,
    pub additive: Additive,
    pub criteria: Vec<Criterion>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Caps {
    pub material: CapDef,
    pub unstable: CapDef,
    pub inspection: CapDef,
    pub undershoot: CapDef,
    pub excursion: CapDef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MethodError(pub String);

impl fmt::Display for MethodError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "scoring method: {}", self.0)
    }
}

impl std::error::Error for MethodError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Doc {
    method: String,
    loss: LossDoc,
    grades: GradesDoc,
    weights: HashMap<Domain, f64>,
    caps: CapsDoc,
    windows: HashMap<ScoringClass, WindowDoc>,
    fallback_track: HashMap<ScoringClass, Scalar>,
    wind_additive: AdditiveDoc,
    criteria: Vec<CriterionDoc>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LossDoc {
    ell: Vec<f64>,
    loss: Vec<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GradesDoc {
    good: f64,
    fair: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapsDoc {
    material: CapDoc,
    unstable: CapDoc,
    inspection: CapDoc,
    undershoot: CapDoc,
    excursion: CapDoc,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapDoc {
    score: f64,
    source: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowDoc {
    top: Scalar,
    split: Scalar,
    bottom: Option<Scalar>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdditiveDoc {
    min: Scalar,
    max: Scalar,
    autothrottle: Scalar,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CriterionDoc {
    id: String,
    title: String,
    domain: Domain,
    unit: String,
    sustain: Option<Scalar>,
    sustain_beyond: Option<BeyondDoc>,
    source: String,
    bands: Vec<BandDoc>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BeyondDoc {
    value: Scalar,
    sustain: Scalar,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BandDoc {
    classes: Vec<ScoringClass>,
    #[serde(default = "all_heights")]
    height: Height,
    levels_from: Option<String>,
    above: Option<[Option<Scalar>; 5]>,
    below: Option<[Option<Scalar>; 5]>,
}

fn all_heights() -> Height {
    Height::All
}

/// A unit named in a criterion: an expression unit, or `level` for a
/// plain number.
fn unit_named(name: &str) -> Result<Option<Unit>, String> {
    if name == "level" {
        return Ok(None);
    }
    match expr::literal(&format!("1 {name}")) {
        Ok((_, Some(unit))) => Ok(Some(unit)),
        _ => Err(format!("unknown unit `{name}`")),
    }
}

/// `value` in `unit`.
fn quantity(path: &str, value: &Scalar, unit: Unit) -> Result<f64, MethodError> {
    expr::quantity(&value.to_string(), Some(unit), "value", "the field")
        .map_err(|m| MethodError(format!("`{path}`: {m}")))
}

impl Method {
    /// The shipped `landing-3`, its levels taken from the core rules.
    pub fn landing_3() -> Result<Self, MethodError> {
        let rules = rules::load(CORE_RULES).map_err(|e| MethodError(e.to_string()))?;
        Self::load(LANDING_3, &rules)
    }

    /// Loads a method, taking `levels_from` thresholds from `rules`.
    pub fn load(yaml: &str, rules: &[Rule]) -> Result<Self, MethodError> {
        let doc: Doc = serde_yaml_ng::from_str(yaml).map_err(|e| MethodError(e.to_string()))?;
        let err = |m: String| MethodError(m);
        if doc.loss.ell != ANCHOR_ELL || doc.loss.loss.len() != 5 {
            return Err(err("`loss` needs a loss at ell 0, 0.5, 1, 2, 3".into()));
        }
        if doc.loss.loss.windows(2).any(|w| w[1] < w[0]) {
            return Err(err("`loss` must not decrease".into()));
        }
        let weights = DOMAINS
            .iter()
            .map(|&d| match doc.weights.get(&d) {
                Some(&w) if w > 0.0 => Ok((d, w)),
                _ => Err(err(format!("`weights` needs a positive weight for {d:?}"))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if doc.weights.len() != DOMAINS.len() {
            return Err(err("`weights` has a domain that is not scored".into()));
        }
        let cap = |c: CapDoc| CapDef {
            score: c.score,
            source: c.source,
        };
        let classes = [
            ScoringClass::Light,
            ScoringClass::Turboprop,
            ScoringClass::Jet,
        ];
        let mut windows = HashMap::new();
        let mut fallback_track_ft = HashMap::new();
        for class in classes {
            let missing = |what: &str| err(format!("`{what}` needs the {class:?} class"));
            let w = doc.windows.get(&class).ok_or_else(|| missing("windows"))?;
            let ft = |s: &Scalar| quantity("windows", s, Unit::Feet);
            windows.insert(
                class,
                WindowDef {
                    top_ft: ft(&w.top)?,
                    split_ft: ft(&w.split)?,
                    bottom_ft: w.bottom.as_ref().map(ft).transpose()?,
                },
            );
            let track = doc
                .fallback_track
                .get(&class)
                .ok_or_else(|| missing("fallback_track"))?;
            fallback_track_ft.insert(class, quantity("fallback_track", track, Unit::Feet)?);
        }
        let kt = |s: &Scalar| quantity("wind_additive", s, Unit::Knots);
        let a = &doc.wind_additive;
        let additive = Additive {
            min_kt: kt(&a.min)?,
            max_kt: kt(&a.max)?,
            autothrottle_kt: kt(&a.autothrottle)?,
        };
        let mut criteria: Vec<Criterion> = Vec::new();
        for c in doc.criteria {
            if criteria.iter().any(|k| k.id == c.id) {
                return Err(err(format!("duplicate criterion `{}`", c.id)));
            }
            criteria.push(criterion(c, rules)?);
        }
        Ok(Self {
            name: doc.method,
            loss: (doc.loss.ell, doc.loss.loss),
            good: doc.grades.good,
            fair: doc.grades.fair,
            weights,
            caps: Caps {
                material: cap(doc.caps.material),
                unstable: cap(doc.caps.unstable),
                inspection: cap(doc.caps.inspection),
                undershoot: cap(doc.caps.undershoot),
                excursion: cap(doc.caps.excursion),
            },
            windows,
            fallback_track_ft,
            additive,
            criteria,
        })
    }

    pub fn criterion(&self, id: &str) -> Option<&Criterion> {
        self.criteria.iter().find(|c| c.id == id)
    }
}

/// Compiles one criterion, attributing errors to it.
fn criterion(doc: CriterionDoc, rules: &[Rule]) -> Result<Criterion, MethodError> {
    let id = doc.id.clone();
    let err = |m: String| MethodError(format!("criterion `{id}`: {m}"));
    let unit = unit_named(&doc.unit).map_err(err)?;
    let us = |s: &Scalar| {
        expr::quantity(&s.to_string(), Some(Unit::Seconds), "sustain", "it")
            .map(|secs| (secs * 1e6).round() as i64)
            .map_err(err)
    };
    let sustain_us = doc.sustain.as_ref().map_or(Ok(0), us)?;
    let beyond = match &doc.sustain_beyond {
        None => None,
        Some(b) => {
            let value = expr::quantity(&b.value.to_string(), unit, "value", "the criterion")
                .map_err(err)?;
            Some((value, us(&b.sustain)?))
        }
    };
    if doc.bands.is_empty() {
        return Err(err("needs at least one band".into()));
    }
    let bands = doc
        .bands
        .into_iter()
        .map(|b| {
            let levels = match &b.levels_from {
                None => None,
                Some(rule) => Some(rule_levels(rules, rule, unit).map_err(err)?),
            };
            let side = |anchors: &Option<[Option<Scalar>; 5]>, side: Side| {
                let from = levels.as_ref().map(|l: &[Vec<(Level, f64)>; 2]| {
                    let l = &l[usize::from(side == Side::Below)];
                    [
                        None,
                        None,
                        find(l, Level::Low),
                        find(l, Level::Medium),
                        find(l, Level::High),
                    ]
                });
                (0..5)
                    .map(|k| {
                        let src = anchors.as_ref().and_then(|a| a[k].as_ref());
                        let expr = src
                            .map(|s| anchor(&s.to_string(), unit))
                            .transpose()
                            .map_err(err)?;
                        Ok(Anchor {
                            expr,
                            from_rule: from.and_then(|f| f[k]),
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map(|v| <[Anchor; 5]>::try_from(v).expect("five anchors"))
            };
            Ok(Band {
                above: side(&b.above, Side::Above)?,
                below: side(&b.below, Side::Below)?,
                classes: b.classes,
                height: b.height,
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(Criterion {
        id: doc.id,
        title: doc.title,
        domain: doc.domain,
        unit,
        sustain_us,
        beyond,
        source: doc.source,
        bands,
    })
}

fn find(levels: &[(Level, f64)], level: Level) -> Option<f64> {
    levels.iter().find(|l| l.0 == level).map(|l| l.1)
}

/// A rule's thresholds as [above, below]; it must exist and be in `unit`.
fn rule_levels(
    rules: &[Rule],
    id: &str,
    unit: Option<Unit>,
) -> Result<[Vec<(Level, f64)>; 2], String> {
    let rule = rules
        .iter()
        .find(|r| r.id == id)
        .ok_or_else(|| format!("`levels_from`: no rule `{id}`"))?;
    match &rule.limits {
        Limits::Thresholds {
            above,
            below,
            unit: u,
        } if *u == unit => Ok([above.clone(), below.clone()]),
        _ => Err(format!(
            "`levels_from`: rule `{id}` has no thresholds in {}",
            Ty::Num(unit)
        )),
    }
}

/// An anchor expression, which must be in `unit`.
fn anchor(src: &str, unit: Option<Unit>) -> Result<Expr<Var>, String> {
    let e = expr::compile(src, &AnchorScope).map_err(|e| e.render(src))?;
    match e.ty() {
        Ty::Num(u) if u == unit => Ok(e),
        ty => Err(format!(
            "anchor `{src}` is {ty}, but the criterion is {}",
            Ty::Num(unit)
        )),
    }
}
