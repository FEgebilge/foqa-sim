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
}

impl From<&analyzer::Touchdown> for Touchdown {
    fn from(td: &analyzer::Touchdown) -> Self {
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
