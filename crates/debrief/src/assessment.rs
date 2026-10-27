//! The flight's landing-2 assessment (docs/scoring.md): every landing
//! scored by `assess`, its findings pointing at `Debrief.events`, the
//! flight graded by its last full-stop landing (Decision 3) and the rest of
//! the flight. A flight without a scored landing keeps the points method.

use assess::{CapKind, Height, Landing, LandingScore, Measures, Method, Speeds};
use rules::{Domain, Level};

use crate::wire::{
    Assessment, AssessmentDetail, CapNote, Confidence, CriterionAssessment, DomainAssessment,
    Event, FindingAssessment, Grade, LandingAssessment, ScoreBands, SideBands,
};
use crate::{Analysis, PILOT_REPORT, secs};

/// `AssessmentDetail.method` of a flight graded by points only.
pub const POINTS: &str = "points";

impl Analysis<'_> {
    /// The assessment, `events` being this flight's `events()`.
    pub fn assessment(&self, events: &[Event]) -> AssessmentDetail {
        let legacy = Assessment::of(events);
        // Each rule event's place in `events` (pilot reports are mixed in).
        let wire = |i: usize| {
            let e = &self.events[i];
            let t = secs(e.start);
            events
                .iter()
                .position(|w| w.rule == e.rule && w.start_s == t)
        };
        let method = self.method;
        let (landings, scores): (Vec<LandingAssessment>, Vec<Option<LandingScore>>) =
            (0..self.touchdowns.len())
                .filter(|&k| !self.touchdowns[k].bounce)
                .map(|k| self.assess_landing(method, k, &wire))
                .unzip();
        let counted: Vec<usize> = landings
            .iter()
            .flat_map(|l| &l.domains)
            .flat_map(|d| &d.findings)
            .filter_map(|f| f.event)
            .collect();
        let other_findings: Vec<usize> = (0..events.len())
            .filter(|i| !counted.contains(i) && events[*i].rule != PILOT_REPORT)
            .collect();
        let other_highs = other_findings
            .iter()
            .filter(|&&i| events[i].level == Level::High)
            .count();
        let scored: Vec<(usize, &LandingScore, bool)> = scores
            .iter()
            .enumerate()
            .filter_map(|(i, s)| Some((i, s.as_ref()?, landings[i].full_stop)))
            .collect();
        let pairs: Vec<(&LandingScore, bool)> = scored.iter().map(|s| (s.1, s.2)).collect();
        let mut detail = AssessmentDetail {
            method: POINTS.into(),
            score: None,
            grade: legacy.grade,
            confidence: None,
            caps: Vec::new(),
            graded_by: None,
            landings,
            other_findings,
            legacy_points: legacy.points,
            legacy_findings: legacy.findings,
            parts: Assessment::parts(events),
            bands: Grade::BANDS,
            grades: ScoreBands {
                good: method.good,
                fair: method.fair,
            },
        };
        let Some(flight) = method.flight_grade(&pairs, other_highs) else {
            return detail;
        };
        // The landing that sets the grade: the last full stop, else the last.
        let at = pairs.iter().rposition(|p| p.1).unwrap_or(pairs.len() - 1);
        let (index, grading, _) = scored[at];
        detail.caps = grading
            .caps
            .iter()
            .map(|c| CapNote::of(c, Some(index)))
            .collect();
        detail
            .caps
            .extend(flight.reasons.iter().map(|reason| CapNote {
                kind: "flight".into(),
                score: flight.score,
                reason: reason.clone(),
                source: "docs/scoring.md Decision 3: flight grade".into(),
                landing: None,
            }));
        detail.method = method.name.clone();
        detail.score = Some(flight.score);
        detail.grade = flight.grade.into();
        detail.confidence = Some(grading.confidence.into());
        detail.graded_by = Some(index);
        detail
    }

    /// Touchdown `k` assessed, its findings by their place in the events.
    fn assess_landing(
        &self,
        method: &Method,
        k: usize,
        wire: &impl Fn(usize) -> Option<usize>,
    ) -> (LandingAssessment, Option<LandingScore>) {
        let l = Landing {
            flight: &self.flight,
            track: &self.track,
            selection: &self.selection,
            touchdowns: &self.touchdowns,
            index: k,
            events: &self.events,
            rules: self.rules,
            ranked: false,
            gate_ft: None,
            target_m: None,
        };
        let t_s = secs(self.touchdowns[k].t);
        match method.measures(&l) {
            Err(missing) => (LandingAssessment::insufficient(t_s, missing), None),
            Ok((m, speeds, full_stop)) => {
                let score = method.score(&m);
                let domains = domains(method, &m, &score, wire);
                let wire = LandingAssessment::scored(t_s, &score, speeds, full_stop, domains);
                (wire, Some(score))
            }
        }
    }
}

/// The domains of `score` with each criterion's resolved bands and each
/// finding's event, and the item that set the loss marked.
fn domains(
    method: &Method,
    m: &Measures,
    score: &LandingScore,
    wire: &impl Fn(usize) -> Option<usize>,
) -> Vec<DomainAssessment> {
    let available: f64 = score
        .domains
        .iter()
        .filter(|d| d.loss.is_some())
        .map(|d| d.weight)
        .sum();
    score
        .domains
        .iter()
        .map(|d| {
            let mut set = d.set_by.is_some();
            let mut sets = |name: String, loss: f64| {
                let hit = set && d.set_by.as_deref() == Some(&name) && Some(loss) == d.loss;
                set &= !hit;
                hit
            };
            let criteria = d
                .criteria
                .iter()
                .map(|c| {
                    let def = method.criterion(&c.id).expect("a scored criterion");
                    let band = def.band(m.class, c.height).expect("a graded band");
                    let name = match c.height {
                        Height::All => c.id.clone(),
                        Height::Above => format!("{} above", c.id),
                        Height::Below => format!("{} below", c.id),
                    };
                    CriterionAssessment {
                        sets: sets(name, c.loss),
                        id: c.id.clone(),
                        title: def.title.clone(),
                        height: c.height,
                        value: c.value,
                        unit: def.unit.map(|u| u.symbol()),
                        ell: c.ell,
                        level: c.level,
                        loss: c.loss,
                        bands: SideBands {
                            above: assess::resolve(&band.above, &m.context, 1.0),
                            below: assess::resolve(&band.below, &m.context, -1.0),
                        },
                        standard: def.source.clone(),
                    }
                })
                .collect();
            let findings = d
                .findings
                .iter()
                .map(|f| FindingAssessment {
                    sets: sets(f.rule.clone(), f.loss),
                    event: f.event.and_then(wire),
                    rule: f.rule.clone(),
                    level: f.level,
                    ell: f.ell,
                    loss: f.loss,
                })
                .collect();
            let lost = d
                .loss
                .map_or(0.0, |loss| d.weight * loss * 100.0 / available);
            DomainAssessment {
                id: d.domain,
                title: domain_title(d.domain),
                weight: d.weight,
                loss: d.loss,
                points: (lost * 10.0).round() / 10.0,
                level: d.level,
                sets_by: d.set_by.clone(),
                criteria,
                findings,
            }
        })
        .collect()
}

pub fn domain_title(d: Domain) -> &'static str {
    match d {
        Domain::Stability => "Approach stability",
        Domain::Energy => "Threshold and flare",
        Domain::Point => "Touchdown point",
        Domain::Lateral => "Lateral control",
        Domain::Contact => "Touchdown contact",
        Domain::Rollout => "Rollout and stopping",
        Domain::Other => "Other",
    }
}

impl CapNote {
    fn of(c: &assess::Cap, landing: Option<usize>) -> Self {
        let kind = match c.kind {
            CapKind::Material => "material",
            CapKind::Unstable => "unstable",
            CapKind::Inspection => "inspection",
            CapKind::Undershoot => "undershoot",
            CapKind::Excursion => "excursion",
        };
        Self {
            kind: kind.into(),
            score: c.score,
            reason: c.reason.clone(),
            source: c.source.clone(),
            landing,
        }
    }
}

impl LandingAssessment {
    fn insufficient(t_s: f64, missing: Vec<&'static str>) -> Self {
        Self {
            t_s,
            status: "insufficient",
            score: None,
            raw: None,
            grade: None,
            confidence: None,
            class: None,
            partial: Vec::new(),
            missing,
            full_stop: true,
            caps: Vec::new(),
            stable_from_ft: None,
            speeds: None,
            domains: Vec::new(),
        }
    }

    fn scored(
        t_s: f64,
        score: &LandingScore,
        speeds: Option<Speeds>,
        full_stop: bool,
        domains: Vec<DomainAssessment>,
    ) -> Self {
        Self {
            t_s,
            status: "scored",
            score: Some(score.score),
            raw: Some(score.raw),
            grade: Some(score.grade.into()),
            confidence: Some(score.confidence.into()),
            class: Some(score.class),
            partial: score.partial.clone(),
            missing: Vec::new(),
            full_stop,
            caps: score.caps.iter().map(|c| CapNote::of(c, None)).collect(),
            stable_from_ft: score.stable_from_ft,
            speeds,
            domains,
        }
    }
}

impl From<assess::Grade> for Grade {
    fn from(g: assess::Grade) -> Self {
        match g {
            assess::Grade::Good => Self::Good,
            assess::Grade::Fair => Self::Fair,
            assess::Grade::Poor => Self::Poor,
        }
    }
}

impl From<assess::Confidence> for Confidence {
    fn from(c: assess::Confidence) -> Self {
        match c {
            assess::Confidence::Full => Self::Full,
            assess::Confidence::Partial => Self::Partial,
        }
    }
}
