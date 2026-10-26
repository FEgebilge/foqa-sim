//! Aircraft identity as the simulator reports it, unconverted. Like the
//! aircraft identification words a real recorder frames alongside the data,
//! it tells the ground station which type profile and limits apply.

/// The user aircraft's identification SimVars, strings exactly as reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AircraftId {
    /// `TITLE`: the livery/variant name, e.g. `C172SP G1000 Passengers`.
    pub title: String,
    /// `ATC MODEL`: carries the ICAO type designator; see `icao_type`.
    pub atc_model: String,
    /// `ATC TYPE`: the manufacturer name for ATC, often a localisation key.
    pub atc_type: String,
    /// `ATC ID`: the tail number / call sign.
    pub atc_id: String,
}

impl AircraftId {
    /// The ICAO Doc 8643 type designator (`C172`, `B38M`, `SF50`...) from
    /// `ATC MODEL`, the key type profiles are matched by. MSFS 2024 reports
    /// a localisation key, `ATCCOM.AC_MODEL C172.0.text` (C172 observed
    /// live, 2026-10-26); older builds wrote `TT:ATCCOM.AC_MODEL_C172.0.text`
    /// and some aircraft configs give the bare designator. `None` unless the
    /// result has the designator form: 2–4 letters or digits, the first a
    /// letter.
    pub fn icao_type(&self) -> Option<String> {
        let model = self.atc_model.trim();
        let model = model.strip_prefix("TT:").unwrap_or(model);
        let code = match model.strip_prefix("ATCCOM.AC_MODEL") {
            Some(key) => key.trim_start_matches(['_', ' ']).split('.').next()?,
            None => model,
        };
        let designator = (2..=4).contains(&code.len())
            && code.starts_with(|c: char| c.is_ascii_alphabetic())
            && code.chars().all(|c| c.is_ascii_alphanumeric());
        designator.then(|| code.to_ascii_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(atc_model: &str) -> Option<String> {
        AircraftId {
            title: String::new(),
            atc_model: atc_model.into(),
            atc_type: String::new(),
            atc_id: String::new(),
        }
        .icao_type()
    }

    #[test]
    fn designator_from_each_known_atc_model_form() {
        // MSFS 2024, Cessna 172 observed live.
        assert_eq!(
            model("ATCCOM.AC_MODEL C172.0.text").as_deref(),
            Some("C172")
        );
        assert_eq!(
            model("TT:ATCCOM.AC_MODEL_B38M.0.text").as_deref(),
            Some("B38M")
        );
        assert_eq!(model("SF50").as_deref(), Some("SF50"));
        assert_eq!(model(" c25c ").as_deref(), Some("C25C"));
    }

    #[test]
    fn non_designators_are_rejected() {
        for bad in [
            "",
            "Cessna Skyhawk",
            "ATCCOM.AC_MODEL .0.text",
            "172",
            "B",
            "B7878",
        ] {
            assert_eq!(model(bad), None, "{bad:?}");
        }
    }
}
