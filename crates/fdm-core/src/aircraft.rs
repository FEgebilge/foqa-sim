//! Aircraft identity as the simulator reports it, unconverted. Like the
//! aircraft identification words a real recorder frames alongside the data,
//! it tells the ground station which type profile and limits apply.

/// The user aircraft's identification SimVars, strings exactly as reported.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AircraftId {
    /// `TITLE`: the livery/variant name, e.g. `C172SP G1000 Passengers`.
    pub title: String,
    /// `ATC MODEL`: carries the ICAO type designator; see `icao_type`.
    pub atc_model: String,
    /// `ATC TYPE`: the manufacturer name for ATC, often a localisation key.
    pub atc_type: String,
    /// `ATC ID`: the tail number / call sign.
    pub atc_id: String,
    /// What the sim says about the airframe; all unknown in recordings
    /// made before 0.11.
    pub facts: SimFacts,
}

/// The sim's own description of the airframe (MSFS SDK, SimVars), each
/// `None` where it was not recorded or out of its documented range.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SimFacts {
    /// `CATEGORY`: `Airplane`, `Helicopter`, `Boat`, ...
    pub category: Option<String>,
    /// `ENGINE TYPE`: 0 piston, 1 jet, 2 none, 3 helo (Bell) turbine,
    /// 4 unsupported, 5 turboprop.
    pub engine_type: Option<u8>,
    /// `NUMBER OF ENGINES`, 0 to 4.
    pub engines: Option<u8>,
    /// `IS GEAR RETRACTABLE`, `IS GEAR FLOATS`, `IS GEAR SKIDS`.
    pub retractable: Option<bool>,
    pub floats: Option<bool>,
    pub skids: Option<bool>,
    /// `MAX GROSS WEIGHT`, lb, rounded.
    pub max_gross_lb: Option<u32>,
}

impl SimFacts {
    /// The numeric SimVars, with their unit, in `from_values` order.
    pub const SIMVARS: [(&'static str, &'static str); 6] = [
        ("ENGINE TYPE", "enum"),
        ("NUMBER OF ENGINES", "number"),
        ("IS GEAR RETRACTABLE", "bool"),
        ("IS GEAR FLOATS", "bool"),
        ("IS GEAR SKIDS", "bool"),
        ("MAX GROSS WEIGHT", "pounds"),
    ];
    /// Heavier than any aircraft flies (An-225: 1.4 million lb): garbage.
    const MAX_LB: f64 = 2_000_000.0;

    /// The facts from `SIMVARS` values as the sim sent them; a value that
    /// is not finite or outside its documented range stays unknown, so a
    /// garbage SimVar never reads as a fact. `category` is kept as is.
    pub fn from_values(&mut self, values: &[f64]) {
        let get = |i: usize, max: f64| {
            values
                .get(i)
                .copied()
                .filter(|v| v.fract() == 0.0 && (0.0..=max).contains(v))
        };
        let flag = |i| get(i, 1.0).map(|v| v == 1.0);
        self.engine_type = get(0, 5.0).map(|v| v as u8);
        self.engines = get(1, 4.0).map(|v| v as u8);
        self.retractable = flag(2);
        self.floats = flag(3);
        self.skids = flag(4);
        self.max_gross_lb = values
            .get(5)
            .filter(|v| **v > 0.0 && **v < Self::MAX_LB)
            .map(|v| v.round() as u32);
    }
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
        let code = self.model();
        let designator = (2..=4).contains(&code.len())
            && code.starts_with(|c: char| c.is_ascii_alphabetic())
            && code.chars().all(|c| c.is_ascii_alphanumeric());
        designator.then(|| code.to_ascii_uppercase())
    }

    /// `ATC MODEL` without its localisation-key wrapper: `C172` of
    /// `ATCCOM.AC_MODEL C172.0.text`, `B737-800` of a bare `B737-800`.
    pub fn model(&self) -> &str {
        let model = self.atc_model.trim();
        let model = model.strip_prefix("TT:").unwrap_or(model);
        match model.strip_prefix("ATCCOM.AC_MODEL") {
            Some(key) => key
                .trim_start_matches(['_', ' '])
                .split('.')
                .next()
                .unwrap_or_default(),
            None => model,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(atc_model: &str) -> Option<String> {
        AircraftId {
            atc_model: atc_model.into(),
            ..AircraftId::default()
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

    #[test]
    fn facts_out_of_their_documented_range_stay_unknown() {
        let mut f = SimFacts::default();
        f.from_values(&[5.0, 2.0, 1.0, 0.0, 0.0, 10_400.4]);
        assert_eq!(
            f,
            SimFacts {
                engine_type: Some(5),
                engines: Some(2),
                retractable: Some(true),
                floats: Some(false),
                skids: Some(false),
                max_gross_lb: Some(10_400),
                category: None,
            }
        );
        f.from_values(&[6.0, 2.5, 0.5, -1.0, f64::NAN, 9e9]);
        assert_eq!(f, SimFacts::default());
        f.from_values(&[1.0]);
        assert_eq!((f.engine_type, f.engines), (Some(1), None));
    }

    #[test]
    fn the_model_drops_the_localisation_key() {
        let id = |m: &str| AircraftId {
            atc_model: m.into(),
            ..AircraftId::default()
        };
        assert_eq!(id("ATCCOM.AC_MODEL C172.0.text").model(), "C172");
        assert_eq!(id(" B737-800 ").model(), "B737-800");
        assert_eq!(id("TT:ATCCOM.AC_MODEL_B38M.0.text").model(), "B38M");
    }
}
