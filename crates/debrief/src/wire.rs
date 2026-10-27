//! The objects of `docs/debrief-api.md`, field for field. Optional fields
//! are left out when unknown; a non-finite number (a parameter with no
//! valid value) goes out as `null`, which is how `serde_json` writes it.

use std::collections::BTreeMap;

use analyzer::RunwayFix;
use assess::{Height, Speeds};
pub use rules::Level;
use rules::{Domain, ScoringClass, Sourced};
use serde::{Deserialize, Serialize};

/// What the recorder is doing, as the tray and the app header show it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RecorderStatus {
    WaitingForSim,
    Connected,
    Recording { flight_id: String, elapsed_s: f64 },
    Error { message: String },
}

/// Disk use of the flights folder.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StorageUsage {
    pub flights: usize,
    pub bytes: u64,
    pub folder: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Aircraft {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icao: Option<String>,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightSummary {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_utc: Option<String>,
    pub duration_s: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aircraft: Option<Aircraft>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub departure: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrival: Option<String>,
    pub landings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worst_level: Option<Level>,
    /// Every level, zero included.
    pub event_counts: BTreeMap<Level, usize>,
    pub assessment: Assessment,
}

/// The flight graded as a whole, as the flight list shows it: by
/// `landing-2` (docs/scoring.md) when a landing was scored, else by points.
/// `points` and `findings` are the points method's (each rule it broke
/// counts once, at its worst level: high 4 points, medium 2, low 1,
/// advisories none), kept for one release.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assessment {
    pub grade: Grade,
    pub points: u32,
    /// Rules broken at low or above.
    pub findings: usize,
    /// `landing-2`, or `points`.
    #[serde(default = "points_method")]
    pub method: String,
    /// 0-100, with a scored landing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<Confidence>,
}

fn points_method() -> String {
    crate::assessment::POINTS.into()
}

/// Whether every value the score expects was there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Full,
    Partial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Grade {
    /// Up to 1 point: nothing, or one low finding.
    Good,
    /// Up to 5: one high, or a few medium or low findings.
    Fair,
    /// 6 or more: a high and more, or many findings.
    Poor,
}

/// One rule's share of the assessment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssessmentPart {
    pub rule: String,
    pub title: String,
    /// Its worst level in the flight, which alone counts.
    pub level: Level,
    pub points: u32,
    /// How many times it fired.
    pub count: usize,
}

/// The grades by points: each covers up to `up_to` (inclusive), the last
/// any more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GradeBand {
    pub grade: Grade,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub up_to: Option<u32>,
}

/// How the assessment was reached (docs/scoring.md §7.5): each landing in
/// six domains, the caps and the rest of the flight; and, for one release,
/// the points method's parts and bands.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AssessmentDetail {
    /// `landing-2`, or `points` without a scored landing.
    pub method: String,
    pub score: Option<f64>,
    pub grade: Grade,
    pub confidence: Option<Confidence>,
    /// What holds the flight's score down beyond its domains: the grading
    /// landing's caps, then the flight's own (`kind: flight`).
    pub caps: Vec<CapNote>,
    /// The landing in `landings` that sets the grade.
    pub graded_by: Option<usize>,
    pub landings: Vec<LandingAssessment>,
    /// Indices in `Debrief.events` of the findings outside every landing.
    pub other_findings: Vec<usize>,
    pub legacy_points: u32,
    pub legacy_findings: usize,
    pub parts: Vec<AssessmentPart>,
    pub bands: [GradeBand; 3],
    /// landing-2's grade edges: Good from `good`, Fair from `fair`.
    pub grades: ScoreBands,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ScoreBands {
    pub good: f64,
    pub fair: f64,
}

/// A safety cap, or a reason the flight's score is below its landing's.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CapNote {
    /// `material`, `unstable`, `inspection`, `undershoot`, `excursion` or
    /// `flight`.
    pub kind: String,
    pub score: f64,
    pub reason: String,
    pub source: String,
    /// The landing in `AssessmentDetail.landings` it comes from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landing: Option<usize>,
}

/// One landing, `scored` or `insufficient` (data any score needs missing).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LandingAssessment {
    /// Its first touchdown.
    pub t_s: f64,
    pub status: &'static str,
    pub score: Option<f64>,
    /// Before the caps.
    pub raw: Option<f64>,
    pub grade: Option<Grade>,
    pub confidence: Option<Confidence>,
    pub class: Option<ScoringClass>,
    /// Expected data that was missing.
    pub partial: Vec<String>,
    /// Required data that was missing.
    pub missing: Vec<&'static str>,
    /// `false` for a touch-and-go: no rollout scored, and it does not set
    /// the flight's grade.
    pub full_stop: bool,
    pub caps: Vec<CapNote>,
    pub stable_from_ft: Option<f64>,
    /// The height splitting the approach window into its two halves, by
    /// class (`windows` of scoring/landing-2.yaml); `None` when insufficient.
    pub split_ft: Option<f64>,
    pub speeds: Option<Speeds>,
    pub domains: Vec<DomainAssessment>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DomainAssessment {
    pub id: Domain,
    pub title: &'static str,
    pub weight: f64,
    /// Of the weight, 0 to 1; `None`: n/a, its weight shared out.
    pub loss: Option<f64>,
    /// Points it cost, n/a domains' weight shared out.
    pub points: f64,
    pub level: u8,
    pub criteria: Vec<CriterionAssessment>,
    pub findings: Vec<FindingAssessment>,
    /// The criterion (`A1 below`) or rule that set the loss.
    pub sets_by: Option<String>,
    /// That item in words, as the app and the EFB show it: "Descent rate
    /// 1,562 fpm at or above 300 ft", or its event's title.
    pub set_by_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CriterionAssessment {
    pub id: String,
    pub title: String,
    /// The half of the approach window it was graded in, or `all`.
    pub height: Height,
    /// The worst value, in `unit` (none: a level).
    pub value: f64,
    /// `value` in its unit as shown: "+12 kt", "1.48 g", "0.8 dot", "L2".
    pub value_text: String,
    /// The half it was graded in, in words ("at or above 500 ft"); `None`
    /// for `all`.
    pub height_text: Option<String>,
    pub unit: Option<&'static str>,
    pub ell: f64,
    pub level: u8,
    pub loss: f64,
    pub bands: SideBands,
    /// Each anchor of `bands` as shown, like `value_text`.
    pub bands_text: SideBands<String>,
    pub standard: String,
    /// It set its domain's loss.
    pub sets: bool,
}

/// The anchors [ideal, tolerance, L1, L2, L3] as resolved for the landing:
/// `above` grades larger values, `below` smaller; `null` where absent.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SideBands<T = f64> {
    pub above: [Option<T>; 5],
    pub below: [Option<T>; 5],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FindingAssessment {
    /// Its index in `Debrief.events`.
    pub event: Option<usize>,
    pub rule: String,
    pub level: Level,
    pub ell: f64,
    pub loss: f64,
    pub sets: bool,
}

impl Grade {
    pub const BANDS: [GradeBand; 3] = [
        GradeBand {
            grade: Grade::Good,
            up_to: Some(1),
        },
        GradeBand {
            grade: Grade::Fair,
            up_to: Some(5),
        },
        GradeBand {
            grade: Grade::Poor,
            up_to: None,
        },
    ];

    fn of(points: u32) -> Self {
        Self::BANDS
            .iter()
            .find(|b| b.up_to.is_none_or(|max| points <= max))
            .map_or(Grade::Poor, |b| b.grade)
    }
}

impl Assessment {
    /// Points a rule brings at its worst `level`.
    pub fn points(level: Level) -> u32 {
        match level {
            Level::High => 4,
            Level::Medium => 2,
            Level::Low => 1,
            Level::Advisory => 0,
        }
    }

    /// Each rule that fired, at its worst level, most points first.
    pub fn parts(events: &[Event]) -> Vec<AssessmentPart> {
        let mut parts: BTreeMap<&str, AssessmentPart> = BTreeMap::new();
        for e in events {
            let part = parts.entry(&e.rule).or_insert_with(|| AssessmentPart {
                rule: e.rule.clone(),
                title: e.title.clone(),
                level: e.level,
                points: 0,
                count: 0,
            });
            part.level = part.level.max(e.level);
            part.count += 1;
        }
        let mut parts: Vec<_> = parts
            .into_values()
            .map(|p| AssessmentPart {
                points: Self::points(p.level),
                ..p
            })
            .collect();
        parts.sort_by_key(|p| std::cmp::Reverse(p.points));
        parts
    }

    /// By the points method alone.
    pub fn of(events: &[Event]) -> Self {
        let parts = Self::parts(events);
        let points = parts.iter().map(|p| p.points).sum();
        Self {
            grade: Grade::of(points),
            points,
            findings: parts.iter().filter(|p| p.points > 0).count(),
            method: points_method(),
            score: None,
            confidence: None,
        }
    }
}

impl From<&AssessmentDetail> for Assessment {
    fn from(d: &AssessmentDetail) -> Self {
        Self {
            grade: d.grade,
            points: d.legacy_points,
            findings: d.legacy_findings,
            method: d.method.clone(),
            score: d.score,
            confidence: d.confidence,
        }
    }
}

/// `Debrief.flight`: the summary plus how the flight was graded.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FlightHeader {
    #[serde(flatten)]
    pub summary: FlightSummary,
    pub lfl_version: u16,
    pub profile: String,
    /// `light` or `transport`: the profile's class, else the class by weight.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// The type profile's values that do not depend on the flight state,
    /// by their `profile.<name>` in rules.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub profile_limits: BTreeMap<&'static str, Sourced>,
    /// The phase boundaries in effect (`docs/phases.md`), by their
    /// `PhaseConfig` name, each from the type profile or the class default.
    pub phase_values: BTreeMap<&'static str, Sourced>,
    /// How `assessment` was reached.
    pub assessment_detail: AssessmentDetail,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damaged_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Debrief {
    pub flight: FlightHeader,
    pub phases: Vec<PhaseSpan>,
    pub touchdowns: Vec<Touchdown>,
    pub events: Vec<Event>,
    pub series: Series,
    pub airports: Vec<Airport>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PhaseSpan {
    pub phase: &'static str,
    pub start_s: f64,
    pub end_s: f64,
    /// What opened it, with the value there, e.g. "descending through
    /// 1000 ft above the aerodrome (998 ft AAL)".
    pub trigger: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Touchdown {
    pub t_s: f64,
    pub bounce: bool,
    pub sink_fpm: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sim_sink_fpm: Option<f64>,
    pub peak_g: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch_deg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roll_deg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ias_kt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runway: Option<TouchdownRunway>,
    /// The wind at contact on the matched runway; `None` without a runway
    /// fix or recorded wind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind: Option<TouchdownWind>,
}

impl Touchdown {
    /// The graded touchdown, with the wind sampled from `flight` at contact.
    pub fn new(td: &analyzer::Touchdown, flight: &analyzer::Flight) -> Self {
        let wind = td.runway.as_ref().and_then(|r| {
            Some(TouchdownWind::on_runway(
                flight.at("wind.speed", td.t)?,
                flight.at("wind.dir", td.t)?,
                r.heading_deg,
            ))
        });
        Self {
            t_s: td.t.as_secs_f64(),
            bounce: td.bounce,
            sink_fpm: td.sink_fpm,
            sim_sink_fpm: td.sim_sink_fpm,
            peak_g: td.peak_g,
            pitch_deg: td.pitch_deg,
            roll_deg: td.roll_deg,
            ias_kt: td.ias_kt,
            runway: td.runway.as_ref().map(TouchdownRunway::from),
            wind,
        }
    }
}

/// The wind at touchdown, resolved on the runway heading.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TouchdownWind {
    /// True direction the wind blows from.
    pub from_deg: f64,
    pub speed_kt: f64,
    /// Along the runway; negative is a tailwind.
    pub headwind_kt: f64,
    /// Across the runway; positive is from the right (looking along the landing), negative from the left.
    pub crosswind_kt: f64,
}

impl TouchdownWind {
    pub fn on_runway(speed_kt: f64, from_deg: f64, heading_deg: f64) -> Self {
        let (headwind_kt, crosswind_kt) =
            analyzer::wind_components(speed_kt, from_deg, heading_deg);
        Self {
            from_deg,
            speed_kt,
            headwind_kt,
            crosswind_kt,
        }
    }
}

/// Where on which runway a touchdown was, from the landing threshold.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TouchdownRunway {
    pub icao: String,
    pub runway: String,
    pub along_m: f64,
    pub cross_m: f64,
    pub lda_m: f64,
    pub tdz_m: f64,
    pub aiming_point_m: f64,
    pub in_tdz: bool,
    pub long_landing: bool,
    pub short_landing: bool,
    /// `In TDZ`, `Long` or `Short`.
    pub zone: &'static str,
}

impl From<&RunwayFix> for TouchdownRunway {
    fn from(f: &RunwayFix) -> Self {
        Self {
            icao: f.icao.clone(),
            runway: f.runway.clone(),
            along_m: f.along_m,
            cross_m: f.cross_m,
            lda_m: f.lda_m,
            tdz_m: f.tdz_m,
            aiming_point_m: f.aiming_point_m,
            in_tdz: f.in_tdz,
            long_landing: f.long_landing,
            short_landing: f.short_landing,
            zone: f.zone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    pub rule: String,
    pub version: u32,
    pub title: String,
    pub description: String,
    pub level: Level,
    pub start_s: f64,
    pub end_s: f64,
    pub peak_s: f64,
    pub measure: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why_it_matters: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advice: Option<String>,
    /// The event in words, e.g. "111 kt at flaps 12°, limit 85 kt".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Parallel arrays on one time base; `NaN` (sent as `null`) where a
/// parameter has no valid value.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Series {
    pub t_s: Vec<f64>,
    pub alt_ft: Vec<f64>,
    pub aal_ft: Vec<f64>,
    pub ias_kt: Vec<f64>,
    pub gs_kt: Vec<f64>,
    pub vs_fpm: Vec<f64>,
    pub nz_g: Vec<f64>,
    pub pitch_deg: Vec<f64>,
    pub roll_deg: Vec<f64>,
    pub flap_deg: Vec<f64>,
    /// The type profile's flap-speed limit at that flap angle.
    pub vfe_kt: Vec<f64>,
    pub lat: Vec<f64>,
    pub lon: Vec<f64>,
}

/// Runway geometry of one recorded airport snapshot, for the map.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Airport {
    pub icao: String,
    pub runways: Vec<Runway>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Runway {
    pub width_m: f64,
    /// The two landing thresholds.
    pub ends: [RunwayEnd; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunwayEnd {
    pub runway: String,
    pub lat: f64,
    pub lon: f64,
    pub heading_deg: f64,
    pub lda_m: f64,
    pub tdz_m: f64,
}

impl From<analyzer::Threshold> for RunwayEnd {
    fn from(t: analyzer::Threshold) -> Self {
        Self {
            runway: t.runway,
            lat: t.lat_deg,
            lon: t.lon_deg,
            heading_deg: t.heading_deg,
            lda_m: t.lda_m,
            tdz_m: t.tdz_m,
        }
    }
}

/// One landing, seconds after its touchdown: what the notification says.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LandingNotice {
    pub flight_id: String,
    pub touchdown: Touchdown,
    pub events: Vec<Event>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(rule: &str, level: Level) -> Event {
        Event {
            rule: rule.into(),
            version: 1,
            title: rule.into(),
            description: String::new(),
            level,
            start_s: 0.0,
            end_s: 0.0,
            peak_s: 0.0,
            measure: String::new(),
            value: None,
            threshold: None,
            unit: None,
            why_it_matters: None,
            advice: None,
            detail: None,
        }
    }

    #[test]
    fn each_rule_counts_once_at_its_worst() {
        use Level::*;
        let grade = |events: &[Event]| {
            let a = Assessment::of(events);
            (a.grade, a.points, a.findings)
        };
        assert_eq!(grade(&[]), (Grade::Good, 0, 0));
        assert_eq!(
            grade(&[event("pilot-report", Advisory), event("a", Low)]),
            (Grade::Good, 1, 1)
        );
        // One high finding, repeated and also seen at low.
        let one = [event("a", High), event("a", High), event("a", Low)];
        assert_eq!(grade(&one), (Grade::Fair, 4, 1));
        assert_eq!(
            grade(&[event("a", High), event("b", Medium)]),
            (Grade::Poor, 6, 2)
        );
    }

    /// The parts add up to the points, most first, each rule once.
    #[test]
    fn the_parts_explain_the_points() {
        use Level::*;
        let events = [
            event("pilot-report", Advisory),
            event("a", Low),
            event("b", Medium),
            event("a", High),
        ];
        let parts = Assessment::parts(&events);
        let rows: Vec<_> = parts
            .iter()
            .map(|p| (p.rule.as_str(), p.level, p.points, p.count))
            .collect();
        assert_eq!(
            rows,
            [
                ("a", High, 4, 2),
                ("b", Medium, 2, 1),
                ("pilot-report", Advisory, 0, 1)
            ]
        );
        let total: u32 = parts.iter().map(|p| p.points).sum();
        assert_eq!(total, Assessment::of(&events).points);
        // The bands' edges.
        let grades = [0, 1, 2, 5, 6].map(Grade::of);
        assert_eq!(
            grades,
            [
                Grade::Good,
                Grade::Good,
                Grade::Fair,
                Grade::Fair,
                Grade::Poor
            ]
        );
    }
}

/// docs/scoring.md §3: one finding grades a clean landing as the points
/// method grades it alone.
#[cfg(test)]
mod grade_band_tests {
    use super::*;
    use assess::{Finding, Measures, Method, Value};
    use rules::{Domain, Level::*, ScoringClass};

    /// A jet landing with every domain at its ideal, plus `findings`.
    fn landing(findings: &[(Domain, Level)]) -> f64 {
        let scalar = |id: &str, x: f64| (id.to_owned(), Value::Scalar(Height::All, Some(x)));
        let m = Measures {
            class: ScoringClass::Jet,
            context: Default::default(),
            heights_ft: Vec::new(),
            values: vec![
                scalar("A6", 0.0),
                scalar("B3", 5.0),
                scalar("C1", 0.0),
                scalar("D1", 0.0),
                scalar("E1", 1.1),
                scalar("F2", 0.05),
            ],
            findings: findings
                .iter()
                .map(|&(domain, level)| Finding {
                    rule: format!("{domain:?}"),
                    domain,
                    level,
                    height_ft: None,
                    event: None,
                })
                .collect(),
            outcome: Default::default(),
            partial: Vec::new(),
        };
        Method::landing_2().unwrap().score(&m).score
    }

    #[test]
    fn single_findings_keep_todays_grade() {
        let method = Method::landing_2().unwrap();
        assert_eq!(landing(&[]), 100.0);
        for domain in assess::DOMAINS {
            for level in [Low, Medium, High] {
                let score = landing(&[(domain, level)]);
                let today = Grade::of(Assessment::points(level));
                assert_eq!(
                    Grade::from(method.grade(score)),
                    today,
                    "{domain:?} {level:?}: {score}"
                );
            }
        }
        // One low costs a third of its domain: 91.25 to 96.5.
        assert_eq!(landing(&[(Domain::Stability, Low)]), 91.3);
        assert_eq!(landing(&[(Domain::Energy, Low)]), 96.5);
        // A high loses its domain and caps at 84.9.
        assert_eq!(landing(&[(Domain::Stability, High)]), 75.0);
        assert_eq!(landing(&[(Domain::Energy, High)]), 84.9);
    }
}

#[cfg(test)]
mod wind_tests {

    use super::TouchdownWind;

    #[test]
    fn wind_is_resolved_on_the_runway_heading() {
        // Runway 27 (270 deg): wind from 290 at 20 kt is a 19 kt headwind, 7 kt from the right.
        let w = TouchdownWind::on_runway(20.0, 290.0, 270.0);
        assert!(
            (w.headwind_kt - 18.79).abs() < 0.01 && (w.crosswind_kt - 6.84).abs() < 0.01,
            "{w:?}"
        );
        // From behind and to the left: tailwind, negative crosswind.
        let w = TouchdownWind::on_runway(10.0, 100.0, 270.0 + 10.0);
        assert!(w.headwind_kt < 0.0 && w.crosswind_kt < 0.0, "{w:?}");
    }
}
