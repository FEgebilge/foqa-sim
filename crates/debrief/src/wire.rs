//! The objects of `docs/debrief-api.md`, field for field. Optional fields
//! are left out when unknown; a non-finite number (a parameter with no
//! valid value) goes out as `null`, which is how `serde_json` writes it.

use std::collections::BTreeMap;

use analyzer::RunwayFix;
pub use rules::Level;
use rules::Sourced;
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

/// The flight graded as a whole. Each rule it broke counts once, at its
/// worst level, so a repeated finding does not pile up but different ones
/// do: high 4 points, medium 2, low 1, advisories (pilot reports among
/// them) none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assessment {
    pub grade: Grade,
    pub points: u32,
    /// Rules broken at low or above.
    pub findings: usize,
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

/// How the assessment was reached, for the debrief: the parts, most points
/// first, and the scale they are read on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssessmentDetail {
    pub parts: Vec<AssessmentPart>,
    pub bands: [GradeBand; 3],
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

    pub fn of(events: &[Event]) -> Self {
        let parts = Self::parts(events);
        let points = parts.iter().map(|p| p.points).sum();
        Self {
            grade: Grade::of(points),
            points,
            findings: parts.iter().filter(|p| p.points > 0).count(),
        }
    }
}

impl AssessmentDetail {
    pub fn of(events: &[Event]) -> Self {
        Self {
            parts: Assessment::parts(events),
            bands: Grade::BANDS,
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
