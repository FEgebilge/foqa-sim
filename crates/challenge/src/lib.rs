//! Landing challenges (docs/challenges.md): one curated landing everyone
//! flies under the same conditions. A `Challenge` is what the site
//! publishes; `start_state` is where the aircraft is placed; `evaluate`
//! grades one attempt from the recorded flight alone, so the app and the
//! server reach the same result.

mod attempt;
mod score;

use fdm_core::Airport;
use fdm_core::geo::LocalFrame;
use rules::{Aircraft, ProfileSet};

use serde::{Deserialize, Serialize};

pub use attempt::{Attempt, Begin, Check, Flown, Marker, aircraft_check, evaluate, evaluate_after};
pub use score::{CURRENT, LANDING_1, LANDING_2, LANDING_3, Part, Score};

const M_PER_NM: f64 = 1852.0;
const FT_PER_M: f64 = 1.0 / 0.3048;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub id: String,
    /// Grows when the definition changes; attempts name the one they flew.
    pub version: u32,
    pub title: String,
    /// When the challenge is open, RFC 3339; enforced by the site.
    #[serde(default)]
    pub window: Option<Window>,
    /// ICAO code.
    pub airport: String,
    /// Landing end as painted: `06`, `27L`.
    pub runway: String,
    #[serde(default)]
    pub aircraft: AircraftRule,
    pub start: Start,
    pub gate: Gate,
    #[serde(default)]
    pub conditions: Conditions,
    #[serde(default)]
    pub rules: AttemptRules,
    pub scoring: Scoring,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub from: String,
    pub to: String,
}

/// Who may fly it; empty lists allow any.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AircraftRule {
    #[serde(default)]
    pub classes: Vec<Aircraft>,
    /// ICAO type designators (`profiles/`).
    #[serde(default)]
    pub types: Vec<String>,
}

/// Where the attempt starts: on the extended centreline.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Start {
    pub distance_nm: f64,
    /// Above the threshold.
    pub height_ft: f64,
    pub ias_kt: f64,
    #[serde(default)]
    pub gear_down: bool,
    /// Fraction of full flap travel, 1 = landing flaps.
    #[serde(default)]
    pub flaps: f64,
    pub mode: Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// The app places the aircraft (`start_state`).
    Teleport,
    /// The pilot gets there any way; the attempt starts at the gate.
    #[serde(rename = "self")]
    SelfFlown,
}

/// Where scoring starts, so settling after a teleport never counts.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    /// From the landing threshold.
    pub distance_nm: f64,
}

/// The bands are measured: an attempt counts only inside them. `weather` and
/// `time` are set: the clean start every attempt loads carries them, and
/// with `weather` set the band's wind is the one blown.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Conditions {
    pub weather: Option<Sky>,
    pub time: Option<Clock>,
    /// The mean wind from the gate to touchdown.
    pub wind: Option<WindBand>,
    /// At touchdown, on the runway.
    pub crosswind_kt: Option<Band>,
    pub tailwind_kt: Option<Band>,
    /// A weather preset shipped for the pilot to pick; a convenience.
    pub preset: Option<String>,
}

/// The sky an attempt is flown under, the sim's own weather types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sky {
    Clear,
    Few,
    Scattered,
    Broken,
    High,
    Overcast,
    Rain,
    Storm,
    Snow,
}

impl Sky {
    pub fn name(self) -> &'static str {
        match self {
            Sky::Clear => "clear sky",
            Sky::Few => "few clouds",
            Sky::Scattered => "scattered clouds",
            Sky::Broken => "broken clouds",
            Sky::High => "high clouds",
            Sky::Overcast => "overcast",
            Sky::Rain => "rain",
            Sky::Storm => "thunderstorm",
            Sky::Snow => "snow",
        }
    }
}

/// The date and the time at the airport an attempt starts at.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clock {
    /// `2026-07-15`.
    pub date: String,
    /// `18:30`, local at the airport.
    pub local: String,
}

impl Clock {
    /// Year and day of the year (1 is 1 January), as a saved flight has them.
    pub fn day_of_year(&self) -> Option<(u32, u32)> {
        let mut parts = self.date.split('-');
        let year = fixed_digits(parts.next()?, 4)?;
        let month = fixed_digits(parts.next()?, 2)?;
        let day = fixed_digits(parts.next()?, 2)?;
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let feb = if leap { 29 } else { 28 };
        let months = [31, feb, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let length = *months.get(month.checked_sub(1)? as usize)?;
        let before: u32 = months[..month as usize - 1].iter().sum();
        (parts.next().is_none() && (1..=length).contains(&day)).then_some((year, before + day))
    }

    /// Hours and minutes.
    pub fn hour_minute(&self) -> Option<(u32, u32)> {
        let (hours, minutes) = self.local.split_once(':')?;
        let (hours, minutes) = (fixed_digits(hours, 2)?, fixed_digits(minutes, 2)?);
        (hours < 24 && minutes < 60).then_some((hours, minutes))
    }
}

fn fixed_digits(s: &str, len: usize) -> Option<u32> {
    (s.len() == len && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindBand {
    /// Where it blows from, true.
    pub from_deg: f64,
    pub speed_kt: f64,
    pub tolerance_deg: f64,
    pub tolerance_kt: f64,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Band {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl Band {
    fn contains(self, x: f64) -> bool {
        self.min.is_none_or(|m| x >= m) && self.max.is_none_or(|m| x <= m)
    }
}

/// What makes an attempt valid besides the always-checked integrity (sim
/// rate 1, no pause, no slew, continuous data).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptRules {
    /// Autopilot off below this height above the runway.
    pub ap_off_by_height_ft: Option<f64>,
    /// Slowed to taxi speed still on the runway.
    #[serde(default)]
    pub stop_on_runway: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scoring {
    pub version: String,
}

impl Challenge {
    /// Reads and checks a published definition.
    pub fn parse(json: &str) -> Result<Self, String> {
        let c: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let positive = |name: &str, x: f64| {
            (x.is_finite() && x > 0.0)
                .then_some(())
                .ok_or(format!("`{name}` must be positive"))
        };
        positive("start.distance_nm", c.start.distance_nm)?;
        positive("start.height_ft", c.start.height_ft)?;
        positive("start.ias_kt", c.start.ias_kt)?;
        positive("gate.distance_nm", c.gate.distance_nm)?;
        if c.gate.distance_nm >= c.start.distance_nm {
            return Err("the gate must be closer to the runway than the start".into());
        }
        if !(0.0..=1.0).contains(&c.start.flaps) {
            return Err("`start.flaps` is a fraction from 0 to 1".into());
        }
        if !is_runway(&c.runway) {
            return Err(format!("runway `{}` is not like `06` or `27L`", c.runway));
        }
        if let Some(clock) = &c.conditions.time {
            if clock.day_of_year().is_none() {
                return Err("`conditions.time.date` is a date like `2026-07-15`".into());
            }
            if clock.hour_minute().is_none() {
                return Err("`conditions.time.local` is a time like `18:30`".into());
            }
        }
        match c.scoring.version.as_str() {
            // Graded with `CURRENT` all the same; only a definition naming
            // landing-2 or landing-3 was written for it, so only theirs must
            // name types with a Vref source (a landing-1 file still loads).
            LANDING_1 => {}
            LANDING_3 | LANDING_2 => c.ranked_on_speed()?,
            other => return Err(format!("unknown scoring `{other}`")),
        }
        Ok(c)
    }

    /// landing-3 ranks on the approach speed, so every type allowed needs
    /// a Vref source in its profile: never the pilot's selected speed
    /// (docs/scoring.md §5, Decision 1).
    fn ranked_on_speed(&self) -> Result<(), String> {
        let types = &self.aircraft.types;
        if types.is_empty() {
            return Err("landing-3 needs `aircraft.types`, each with a Vref source".into());
        }
        let profiles = ProfileSet::shipped().map_err(|e| e.to_string())?;
        let lacking: Vec<&str> = types
            .iter()
            .filter(|t| {
                let p = profiles.select_type(Some(t.to_ascii_uppercase()), || None);
                !p.profile.is_some_and(|p| {
                    p.vref.is_some() || p.approach_speed.is_some() || p.vso.is_some()
                })
            })
            .map(String::as_str)
            .collect();
        match lacking[..] {
            [] => Ok(()),
            _ => Err(format!(
                "no Vref source for {}: landing-3 cannot rank it",
                lacking.join(", ")
            )),
        }
    }

    /// The challenge's landing threshold at `airport`, with the runway's
    /// elevation in m.
    fn threshold(&self, airport: &Airport) -> Option<(analyzer::Threshold, f64)> {
        if !airport.icao.eq_ignore_ascii_case(&self.airport) {
            return None;
        }
        airport.runways.iter().find_map(|r| {
            analyzer::thresholds(r, airport.magvar_deg)
                .into_iter()
                .find(|t| t.runway.eq_ignore_ascii_case(&self.runway))
                .map(|t| (t, r.elevation_m))
        })
    }

    /// The landing threshold at `airport`, latitude and longitude, and the
    /// gate's distance from it, m.
    pub fn gate_fix(&self, airport: &Airport) -> Option<(f64, f64, f64)> {
        let (t, _) = self.threshold(airport)?;
        Some((t.lat_deg, t.lon_deg, self.gate.distance_nm * M_PER_NM))
    }

    /// Why `start_state` found no start at `airport`, with the runways the
    /// sim has there: its scenery is what counts, and a new parallel runway
    /// renames the old one (LTFJ 06 became 06L).
    pub fn no_runway(&self, airport: &Airport) -> String {
        let mut names: Vec<String> = (airport.runways.iter())
            .flat_map(|r| analyzer::thresholds(r, airport.magvar_deg))
            .map(|t| t.runway)
            .collect();
        names.sort();
        let (icao, runway) = (&self.airport, &self.runway);
        match names.join(", ") {
            none if none.is_empty() => format!("{icao} has no runway {runway} in the sim"),
            names => format!("{icao} has no runway {runway} in the sim, only {names}"),
        }
    }
}

/// `06`, `27L`: two digits 01 to 36 and an optional L, C or R. `get`, not
/// `split_at`: a multi-byte character at byte 2 (`1é`) is not a boundary.
fn is_runway(s: &str) -> bool {
    let (Some(digits), Some(letter)) = (s.get(..2), s.get(2..)) else {
        return false;
    };
    digits.bytes().all(|b| b.is_ascii_digit())
        && digits.parse::<u8>().is_ok_and(|n| (1..=36).contains(&n))
        && matches!(letter, "" | "L" | "C" | "R")
}

/// Where a teleport start puts the aircraft.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct StartState {
    pub lat_deg: f64,
    pub lon_deg: f64,
    /// MSL.
    pub alt_ft: f64,
    /// True; the runway's.
    pub heading_deg: f64,
    pub ias_kt: f64,
}

/// The start of `challenge` at `airport`: on the extended centreline,
/// `distance_nm` before the threshold and `height_ft` above it. `None` when
/// the airport is not the challenge's or lacks its runway.
pub fn start_state(challenge: &Challenge, airport: &Airport) -> Option<StartState> {
    let (threshold, elevation_m) = challenge.threshold(airport)?;
    let back_m = challenge.start.distance_nm * M_PER_NM;
    let (sin, cos) = threshold.heading_deg.to_radians().sin_cos();
    let (lat_deg, lon_deg) =
        LocalFrame::new(threshold.lat_deg, threshold.lon_deg).point(-back_m * sin, -back_m * cos);
    Some(StartState {
        lat_deg,
        lon_deg,
        alt_ft: elevation_m * FT_PER_M + challenge.start.height_ft,
        heading_deg: threshold.heading_deg,
        ias_kt: challenge.start.ias_kt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fdm_core::geo::distance_m;
    use fdm_core::{Designator, Runway, RunwayEnd};

    pub(crate) const JSON: &str = r#"{
      "id": "test-20", "version": 1, "title": "Runway 20",
      "airport": "TEST", "runway": "20",
      "aircraft": { "classes": ["transport"] },
      "start": { "distance_nm": 8, "height_ft": 2500, "ias_kt": 160, "gear_down": true, "flaps": 1, "mode": "teleport" },
      "gate": { "distance_nm": 3 },
      "scoring": { "version": "landing-1" }
    }"#;

    #[test]
    fn a_clock_is_read_as_a_saved_flight_has_it() {
        let clock = |date: &str, local: &str| Clock {
            date: date.into(),
            local: local.into(),
        };
        assert_eq!(
            clock("2026-11-02", "").day_of_year(),
            Some((2026, 306)),
            "as the sim saved that day"
        );
        assert_eq!(clock("2026-01-01", "").day_of_year(), Some((2026, 1)));
        assert_eq!(clock("2024-12-31", "").day_of_year(), Some((2024, 366)));
        for bad in [
            "2026-02-29",
            "2026-13-01",
            "2026-00-10",
            "26-01-01",
            "2026-1-1",
            "x",
        ] {
            assert_eq!(clock(bad, "").day_of_year(), None, "{bad}");
        }
        assert_eq!(clock("", "18:30").hour_minute(), Some((18, 30)));
        for bad in ["24:00", "18:60", "8:30", "1830"] {
            assert_eq!(clock("", bad).hour_minute(), None, "{bad}");
        }
    }

    #[test]
    fn a_definition_sets_its_sky_and_clock() {
        let with = |conditions: &str| {
            let json = JSON.replace(
                r#""gate": { "distance_nm": 3 },"#,
                &format!(r#""gate": {{ "distance_nm": 3 }}, "conditions": {conditions},"#),
            );
            Challenge::parse(&json)
        };
        let c = with(r#"{"weather":"storm","time":{"date":"2026-07-15","local":"18:30"}}"#);
        let c = c.unwrap().conditions;
        assert_eq!(c.weather, Some(Sky::Storm));
        assert_eq!(c.time.unwrap().local, "18:30");
        assert!(with(r#"{"weather":"hail"}"#).is_err());
        let bad = with(r#"{"time":{"date":"2026-02-30","local":"18:30"}}"#);
        assert!(bad.unwrap_err().contains("conditions.time.date"));
        let bad = with(r#"{"time":{"date":"2026-02-20","local":"25:00"}}"#);
        assert!(bad.unwrap_err().contains("conditions.time.local"));
    }

    fn airport() -> Airport {
        let end = |number| RunwayEnd {
            number,
            designator: Designator::None,
            displaced_m: 0.0,
        };
        Airport {
            icao: "TEST".into(),
            region: String::new(),
            lat_deg: 38.5,
            lon_deg: 27.0,
            elevation_m: 30.0,
            magvar_deg: 0.0,
            runways: vec![Runway {
                lat_deg: 38.5,
                lon_deg: 27.0,
                elevation_m: 30.0,
                heading_deg: 20.0,
                length_m: 3000.0,
                width_m: 45.0,
                primary: end(2),
                secondary: end(20),
            }],
        }
    }

    #[test]
    fn starts_on_the_extended_centreline() {
        let c = Challenge::parse(JSON).unwrap();
        let s = start_state(&c, &airport()).unwrap();
        assert_eq!(s.heading_deg, 200.0);
        assert!((s.alt_ft - (30.0 * FT_PER_M + 2500.0)).abs() < 1e-6);
        // Runway 20's threshold is the north end, 1500 m from the centre;
        // the start is 8 NM further north.
        let from_centre = distance_m((38.5, 27.0), (s.lat_deg, s.lon_deg));
        assert!(
            (from_centre - (1500.0 + 8.0 * M_PER_NM)).abs() < 5.0,
            "{from_centre}"
        );
        assert!(s.lat_deg > 38.5);
        let mut other = c.clone();
        other.runway = "09".into();
        assert_eq!(start_state(&other, &airport()), None);
        assert_eq!(
            other.no_runway(&airport()),
            "TEST has no runway 09 in the sim, only 02, 20"
        );
    }

    #[test]
    fn rejects_an_inconsistent_definition() {
        let err = |from: &str, to: &str| Challenge::parse(&JSON.replace(from, to)).unwrap_err();
        assert_eq!(
            err(r#""distance_nm": 3"#, r#""distance_nm": 9"#),
            "the gate must be closer to the runway than the start"
        );
        assert_eq!(
            err(r#""runway": "20""#, r#""runway": "2""#),
            "runway `2` is not like `06` or `27L`"
        );
        assert_eq!(err("landing-1", "landing-0"), "unknown scoring `landing-0`");
        assert_eq!(
            err(r#""flaps": 1"#, r#""flaps": 30"#),
            "`start.flaps` is a fraction from 0 to 1"
        );
        assert!(err(r#""title""#, r#""name""#).contains("unknown field `name`"));
        assert!(is_runway("27L") && is_runway("06") && !is_runway("37") && !is_runway("27X"));
    }

    #[test]
    fn a_non_ascii_runway_is_rejected_not_a_panic() {
        assert!(
            ["1é", "", "é", "2", "+9", "09é"]
                .iter()
                .all(|s| !is_runway(s))
        );
    }
}
