//! The flight's landing-2 assessment (docs/scoring.md): every landing
//! scored by `assess`, its findings pointing at `Debrief.events`, the
//! flight graded by its last full-stop landing (Decision 3) and the rest of
//! the flight. A flight without a scored landing keeps the points method.

use assess::{
    CapKind, Height, Landing, LandingScore, Measures, Method, SpeedSource, Speeds, height_text,
    item_text, number,
};
pub use assess::{reading_text, value_text};
use rules::{Domain, Level, ScoringClass};

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
        let (mut landings, scores): (Vec<LandingAssessment>, Vec<Option<LandingScore>>) =
            (0..self.touchdowns.len())
                .filter(|&k| !self.touchdowns[k].bounce)
                .map(|k| self.assess_landing(method, k, &wire))
                .unzip();
        for d in landings.iter_mut().flat_map(|l| &mut l.domains) {
            d.set_by_text = set_by_text(d, events);
        }
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
            partial_note: None,
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
                title: "The flight grade is held below the landing's score".into(),
                reason: reason.clone(),
                source: "docs/scoring.md Decision 3: flight grade".into(),
                landing: None,
            }));
        detail.partial_note = partial_note(&detail.landings);
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
                let split_ft = method.windows[&m.class].split_ft;
                let domains = domains(method, &m, &score, split_ft, wire);
                let mut wire = LandingAssessment::scored(t_s, &score, speeds, full_stop, domains);
                wire.split_ft = Some(split_ft);
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
    split_ft: f64,
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
                    let height_text = height_text(c.height, split_ft);
                    let unit = def.unit.map(|u| u.symbol());
                    let text = |x: f64| value_text(&c.id, unit, x);
                    let bands = SideBands {
                        above: assess::resolve(&band.above, &m.context, 1.0),
                        below: assess::resolve(&band.below, &m.context, -1.0),
                    };
                    CriterionAssessment {
                        sets: sets(name, c.loss),
                        id: c.id.clone(),
                        title: def.title.clone(),
                        height: c.height,
                        value: c.value,
                        value_text: reading_text(&c.id, unit, c.value),
                        height_text,
                        unit,
                        ell: c.ell,
                        level: c.level,
                        loss: c.loss,
                        bands_text: SideBands {
                            above: bands.above.map(|a| a.map(text)),
                            below: bands.below.map(|a| a.map(text)),
                        },
                        bands,
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
                short: domain_short(d.domain),
                summary: domain_summary(d.domain),
                weight: d.weight,
                loss: d.loss,
                points: (lost * 10.0).round() / 10.0,
                level: d.level,
                sets_by: d.set_by.clone(),
                set_by_text: None,
                criteria,
                findings,
            }
        })
        .collect()
}

/// The item that set a domain's deduction in words: the criterion with its
/// value and half ("Descent rate 1,562 fpm above 300 ft"), else the
/// finding's event title (its rule without one).
fn set_by_text(d: &DomainAssessment, events: &[Event]) -> Option<String> {
    if let Some(c) = d.criteria.iter().find(|c| c.sets) {
        return Some(item_text(&c.title, &c.value_text, c.height_text.as_deref()));
    }
    let f = d.findings.iter().find(|f| f.sets)?;
    let event = f.event.and_then(|i| events.get(i));
    Some(event.map_or_else(|| f.rule.clone(), |e| e.title.clone()))
}

/// Why the data is partial, from every landing's missing items.
fn partial_note(landings: &[LandingAssessment]) -> Option<String> {
    let items: Vec<&str> = landings
        .iter()
        .flat_map(|l| &l.partial)
        .map(String::as_str)
        .collect();
    (!items.is_empty()).then(|| {
        format!(
            "Some expected data was missing, so these items were not graded and never get full credit: {}. A challenge would not rank this flight.",
            items.join("; ")
        )
    })
}

/// The class's grading limits, the speeds and the stable height: the facts
/// under a scored landing's title.
fn facts(class: ScoringClass, speeds: &Option<Speeds>, stable_from_ft: Option<f64>) -> Vec<String> {
    let class = match class {
        ScoringClass::Light => "light-aircraft",
        ScoringClass::Turboprop => "turboprop",
        ScoringClass::Jet => "jet",
    };
    let speeds = match speeds {
        None => "No reference speed for this aircraft: speed was not graded".to_owned(),
        Some(s) => {
            let note = match (s.source, s.approximate) {
                (SpeedSource::Selected, _) => " (the selected speed: debrief only)",
                (_, true) => " (approximate, from the aircraft profile)",
                _ => "",
            };
            format!(
                "Reference speed {} kt, approach speed {} kt{note}",
                number(s.vref_kt, 0),
                number(s.vapp_kt, 0)
            )
        }
    };
    let stable = stable_from_ft.map_or_else(
        || "Approach never fully stable".to_owned(),
        |ft| format!("Approach stable from {} ft down", number(ft, 0)),
    );
    vec![format!("Graded with {class} limits"), speeds, stable]
}

/// A domain's title in a word or two, for the EFB's small bars.
pub fn domain_short(d: Domain) -> &'static str {
    match d {
        Domain::Stability => "Approach",
        Domain::Energy => "Flare",
        Domain::Point => "Landing spot",
        Domain::Lateral => "Centreline",
        Domain::Contact => "Touchdown",
        Domain::Rollout => "Rollout",
        Domain::Other => "Other",
    }
}

/// What a domain looks at, in one line.
pub fn domain_summary(d: Domain) -> &'static str {
    match d {
        Domain::Stability => {
            "Speed, descent rate, bank, pitch, glideslope and configuration on final"
        }
        Domain::Energy => "Height over the threshold, touchdown speed and float down the runway",
        Domain::Point => "How far from the aiming point the wheels touched down",
        Domain::Lateral => {
            "Offset from the centreline, crab and drift at touchdown, then on the rollout"
        }
        Domain::Contact => "Load, sink rate, bounce, tail clearance and bank at touchdown",
        Domain::Rollout => "How soon braking began and how hard it had to be to stop",
        Domain::Other => "Findings outside the other five",
    }
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
    /// `c` as the wire shows it; `landing` is its place in `landings`.
    pub fn of(c: &assess::Cap, landing: Option<usize>) -> Self {
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
            title: c.title.clone(),
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
            note: Some(assess::not_scored_text(&missing)),
            facts: Vec::new(),
            missing,
            full_stop: true,
            caps: Vec::new(),
            stable_from_ft: None,
            split_ft: None,
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
            note: None,
            facts: facts(score.class, &speeds, score.stable_from_ft),
            full_stop,
            caps: score.caps.iter().map(|c| CapNote::of(c, None)).collect(),
            stable_from_ft: score.stable_from_ft,
            split_ft: None,
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
