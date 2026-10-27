//! The landing score from its measured values and findings
//! (docs/scoring.md §2-4): ℓ per criterion, each domain's loss its worst
//! item, n/a domains' weight redistributed, then the safety caps.

use analyzer::stable_from;
use rules::{Domain, Level, ScoringClass};
use serde::Serialize;

use crate::method::{Context, Criterion, Height, Method};
use crate::scale::{ell, level, sides};

/// A measured value of one criterion.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// One value, graded at `height` (`All` unless split by height).
    Scalar(Height, Option<f64>),
    /// Per point of the approach window (`Measures::heights_ft`): the
    /// value held for the criterion's persistence going up (`high`) and
    /// going down (`low`); `NaN` where unknown or not yet held.
    Series { high: Vec<f64>, low: Vec<f64> },
}

/// A rule event that counts toward this landing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub rule: String,
    pub domain: Domain,
    pub level: Level,
    /// Above the aerodrome where it was measured.
    pub height_ft: Option<f64>,
    /// Its index in the events the landing was assessed with.
    pub event: Option<usize>,
}

/// How the landing ended, for the caps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    /// Touchdown before the threshold.
    pub undershoot: bool,
    /// Left the runway or crashed, in words.
    pub excursion: Option<String>,
}

/// Everything a landing is scored from.
#[derive(Debug, Clone, PartialEq)]
pub struct Measures {
    pub class: ScoringClass,
    pub context: Context,
    /// The approach window's heights above the runway, for the series.
    pub heights_ft: Vec<f64>,
    pub values: Vec<(String, Value)>,
    pub findings: Vec<Finding>,
    pub outcome: Outcome,
    /// Data expected but missing; the score is then `partial`.
    pub partial: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Grade {
    Good,
    Fair,
    Poor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Full,
    Partial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CapKind {
    Material,
    Unstable,
    Inspection,
    Undershoot,
    Excursion,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cap {
    pub kind: CapKind,
    pub score: f64,
    /// What caused it.
    pub reason: String,
    pub source: String,
}

/// One graded criterion (a series criterion once per half of the window).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CriterionScore {
    pub id: String,
    pub height: Height,
    /// The worst value, in the criterion's unit.
    pub value: f64,
    pub ell: f64,
    pub level: u8,
    pub loss: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FindingScore {
    pub rule: String,
    pub event: Option<usize>,
    pub level: Level,
    pub ell: f64,
    pub loss: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DomainScore {
    pub domain: Domain,
    pub weight: f64,
    /// `None`: n/a, its weight redistributed.
    pub loss: Option<f64>,
    pub level: u8,
    pub criteria: Vec<CriterionScore>,
    pub findings: Vec<FindingScore>,
    /// The criterion (`A1`, with its height) or rule that set the loss.
    pub set_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LandingScore {
    pub method: String,
    pub class: ScoringClass,
    /// After the caps, to 0.1.
    pub score: f64,
    pub raw: f64,
    pub grade: Grade,
    pub confidence: Confidence,
    pub partial: Vec<String>,
    pub caps: Vec<Cap>,
    pub domains: Vec<DomainScore>,
    /// The lowest height above which every approach criterion stayed
    /// within tolerance; `None` if it never was.
    pub stable_from_ft: Option<f64>,
}

/// A finding's ℓ: low 1, medium 2, high 3; an advisory costs nothing.
fn finding_ell(level: Level) -> f64 {
    match level {
        Level::Advisory => 0.0,
        Level::Low => 1.0,
        Level::Medium => 2.0,
        Level::High => 3.0,
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

impl Method {
    pub fn loss(&self, ell: f64) -> f64 {
        rules::interpolate(&self.loss.0, &self.loss.1, ell)
    }

    pub fn grade(&self, score: f64) -> Grade {
        if score >= self.good {
            Grade::Good
        } else if score >= self.fair {
            Grade::Fair
        } else {
            Grade::Poor
        }
    }

    /// The landing's score.
    pub fn score(&self, m: &Measures) -> LandingScore {
        let split = self.windows[&m.class].split_ft;
        let mut scored = Vec::new();
        let mut ok = vec![true; m.heights_ft.len()];
        for (id, value) in &m.values {
            let Some(c) = self.criterion(id) else {
                continue;
            };
            scored.extend(self.graded(c, m, value, split, &mut ok));
        }
        let domains: Vec<DomainScore> = self
            .weights
            .iter()
            .map(|&(domain, weight)| self.domain(domain, weight, &scored, &m.findings))
            .collect();
        let available: f64 = domains
            .iter()
            .filter(|d| d.loss.is_some())
            .map(|d| d.weight)
            .sum();
        let lost: f64 = domains
            .iter()
            .filter_map(|d| Some(d.weight * d.loss?))
            .sum();
        let raw = if available > 0.0 {
            100.0 - lost * 100.0 / available
        } else {
            100.0
        };
        let caps = self.caps(m, &scored, split);
        let score = round1(caps.iter().map(|c| c.score).fold(raw, f64::min));
        LandingScore {
            method: self.name.clone(),
            class: m.class,
            score,
            raw: round1(raw),
            grade: self.grade(score),
            confidence: if m.partial.is_empty() {
                Confidence::Full
            } else {
                Confidence::Partial
            },
            partial: m.partial.clone(),
            caps,
            domains,
            stable_from_ft: stable_from(&m.heights_ft, &ok),
        }
    }

    /// One criterion's scores; a series is graded on each side of the
    /// split where it has a band, and marks the points outside tolerance
    /// in `ok` (approach criteria only).
    fn graded(
        &self,
        c: &Criterion,
        m: &Measures,
        value: &Value,
        split: f64,
        ok: &mut [bool],
    ) -> Vec<(Domain, CriterionScore)> {
        let score = |height, value: f64, ell: f64| {
            let s = CriterionScore {
                id: c.id.clone(),
                height,
                value,
                ell,
                level: level(ell),
                loss: self.loss(ell),
            };
            (c.domain, s)
        };
        match value {
            Value::Scalar(height, x) => {
                let (Some(x), Some(band)) = (x, c.band(m.class, *height)) else {
                    return Vec::new();
                };
                vec![score(*height, *x, ell(band, &m.context, *x, *x))]
            }
            Value::Series { high, low } => [Height::Above, Height::Below]
                .into_iter()
                .filter_map(|height| {
                    let band = c.band(m.class, height)?;
                    let side = |i: &usize| (m.heights_ft[*i] >= split) == (height == Height::Above);
                    let points: Vec<usize> = (0..m.heights_ft.len()).filter(side).collect();
                    let point_ell = |i: usize| ell(band, &m.context, high[i], low[i]);
                    for &i in &points {
                        if c.domain == Domain::Stability && point_ell(i) > 0.5 {
                            ok[i] = false;
                        }
                    }
                    let worst = points
                        .iter()
                        .copied()
                        .filter(|&i| !high[i].is_nan() || !low[i].is_nan())
                        .max_by(|&a, &b| {
                            let key = |i: usize| (point_ell(i), high[i].max(-low[i]));
                            let (ka, kb) = (key(a), key(b));
                            ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
                        })?;
                    let (above, below) = sides(band, &m.context, high[worst], low[worst]);
                    let x = if below > above {
                        low[worst]
                    } else {
                        high[worst]
                    };
                    Some(score(height, x, above.max(below)))
                })
                .collect(),
        }
    }

    /// A domain's loss: the worst of its criteria and findings.
    fn domain(
        &self,
        domain: Domain,
        weight: f64,
        scored: &[(Domain, CriterionScore)],
        findings: &[Finding],
    ) -> DomainScore {
        let criteria: Vec<CriterionScore> = scored
            .iter()
            .filter(|(d, _)| *d == domain)
            .map(|(_, s)| s.clone())
            .collect();
        let findings: Vec<FindingScore> = findings
            .iter()
            .filter(|f| f.domain == domain)
            .map(|f| {
                let ell = finding_ell(f.level);
                FindingScore {
                    rule: f.rule.clone(),
                    event: f.event,

                    level: f.level,
                    ell,
                    loss: self.loss(ell),
                }
            })
            .collect();
        let items = criteria
            .iter()
            .map(|c| (c.ell, c.loss, height_name(&c.id, c.height)))
            .chain(findings.iter().map(|f| (f.ell, f.loss, f.rule.clone())));
        let worst = items.max_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)));
        let available = !criteria.is_empty() || !findings.is_empty();
        DomainScore {
            domain,
            weight,
            loss: available.then(|| worst.as_ref().map_or(0.0, |w| w.1)),
            level: worst.as_ref().map_or(0, |w| level(w.0)),
            set_by: worst.filter(|w| w.1 > 0.0).map(|w| w.2),
            criteria,
            findings,
        }
    }

    /// The caps that apply (docs/scoring.md §4), with their causes.
    fn caps(&self, m: &Measures, scored: &[(Domain, CriterionScore)], split: f64) -> Vec<Cap> {
        let mut caps = Vec::new();
        let mut cap = |kind, def: &crate::method::CapDef, reason: String| {
            caps.push(Cap {
                kind,
                score: def.score,
                reason,
                source: def.source.clone(),
            })
        };
        let material = scored
            .iter()
            .map(|(_, c)| (c.ell, height_name(&c.id, c.height)))
            .chain(
                m.findings
                    .iter()
                    .map(|f| (finding_ell(f.level), f.rule.clone())),
            )
            .filter(|(ell, _)| *ell >= 2.0)
            .map(|(_, what)| what)
            .collect::<Vec<_>>();
        if !material.is_empty() {
            let reason = format!("level 2 or more: {}", material.join(", "));
            cap(CapKind::Material, &self.caps.material, reason);
        }
        if let Some(reason) = unstable(m, scored, split) {
            cap(CapKind::Unstable, &self.caps.unstable, reason);
        }
        let inspection = m
            .findings
            .iter()
            .find(|f| f.rule == "hard-landing-inspection");
        let tail_strike = scored.iter().find(|(_, c)| c.id == "E4" && c.value < 0.0);
        if let Some(f) = inspection {
            cap(
                CapKind::Inspection,
                &self.caps.inspection,
                format!("{}: hard-landing inspection", f.rule),
            );
        } else if tail_strike.is_some() {
            cap(
                CapKind::Inspection,
                &self.caps.inspection,
                "tail strike at touchdown".into(),
            );
        }
        if m.outcome.undershoot {
            cap(
                CapKind::Undershoot,
                &self.caps.undershoot,
                "touchdown before the threshold".into(),
            );
        }
        if let Some(reason) = &m.outcome.excursion {
            cap(CapKind::Excursion, &self.caps.excursion, reason.clone());
        }
        caps
    }
}

/// `A1 below`, or the id of a criterion graded once.
fn height_name(id: &str, height: Height) -> String {
    match height {
        Height::All => id.to_owned(),
        Height::Above => format!("{id} above"),
        Height::Below => format!("{id} below"),
    }
}

/// A continued unstable approach, Data4Safety's rule (S1): any approach
/// criterion at L2/L3 below the split (SL2B/SL3), or three at L1 or more
/// above it (SL1/SL2A); a light aircraft, any at L2 below its split or
/// three at L1 anywhere. A finding of the stability domain at medium or
/// above below the split counts as an L2 there; a criterion graded for the
/// whole window (late configuration) counts at L3 as below the split.
fn unstable(m: &Measures, scored: &[(Domain, CriterionScore)], split: f64) -> Option<String> {
    let approach: Vec<&CriterionScore> = scored
        .iter()
        .filter(|(d, _)| *d == Domain::Stability)
        .map(|(_, c)| c)
        .collect();
    let low = approach.iter().find(|c| match c.height {
        Height::Below => c.ell >= 2.0,
        Height::All => c.ell >= 3.0,
        Height::Above => false,
    });
    if let Some(c) = low {
        return Some(format!(
            "{} at level {} below {split} ft, landing continued",
            c.id, c.level
        ));
    }
    let finding = m.findings.iter().find(|f| {
        f.domain == Domain::Stability
            && f.level >= Level::Medium
            && f.height_ft.is_some_and(|h| h < split)
    });
    if let Some(f) = finding {
        return Some(format!("{} below {split} ft, landing continued", f.rule));
    }
    let light = m.class == ScoringClass::Light;
    let mut ids: Vec<&str> = approach
        .iter()
        .filter(|c| c.ell >= 1.0 && (light || c.height != Height::Below))
        .map(|c| c.id.as_str())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    (ids.len() >= 3).then(|| format!("{} at level 1 or more, landing continued", ids.join(", ")))
}

/// The flight's grade from its landings (Decision 3).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FlightGrade {
    pub score: f64,
    pub grade: Grade,
    /// Why it is below the grading landing's score.
    pub reasons: Vec<String>,
}

impl Method {
    /// The last full-stop landing sets the flight's score (the last
    /// landing without one); an earlier landing's safety cap and high
    /// findings elsewhere in the flight still cap it: one high at Fair, two
    /// at Poor. `landings` in flight order as (score, full stop).
    pub fn flight_grade(
        &self,
        landings: &[(&LandingScore, bool)],
        other_highs: usize,
    ) -> Option<FlightGrade> {
        let last = landings
            .iter()
            .rposition(|l| l.1)
            .unwrap_or(landings.len().checked_sub(1)?);
        let mut score = landings[last].0.score;
        let mut reasons = Vec::new();
        let mut cap = |limit: f64, reason: String| {
            if limit < score {
                score = limit;
                reasons.push(reason);
            }
        };
        let earlier = landings.iter().enumerate().filter(|(i, _)| *i != last);
        for (i, (l, _)) in earlier {
            for c in l.caps.iter().filter(|c| c.kind != CapKind::Material) {
                cap(c.score, format!("landing {}: {}", i + 1, c.reason));
            }
        }
        match other_highs {
            0 => {}
            1 => cap(
                round1(self.good - 0.1),
                "a high finding elsewhere in the flight".into(),
            ),
            n => cap(
                round1(self.fair - 0.1),
                format!("{n} high findings elsewhere in the flight"),
            ),
        }
        Some(FlightGrade {
            score,
            grade: self.grade(score),
            reasons,
        })
    }
}
