//! Aircraft identity as JSON, the same encoding in the journal (event text)
//! and in the raw capture, so a replay reproduces the journal exactly.

use fdm_core::AircraftId;
use serde_json::{Value, json};

pub(crate) fn to_json(a: &AircraftId) -> Value {
    json!({ "title": a.title, "atc_model": a.atc_model, "atc_type": a.atc_type, "atc_id": a.atc_id })
}

/// `None` if any field is missing or not a string.
pub(crate) fn from_json(v: &Value) -> Option<AircraftId> {
    let text = |key| Some(v.get(key)?.as_str()?.to_owned());
    Some(AircraftId {
        title: text("title")?,
        atc_model: text("atc_model")?,
        atc_type: text("atc_type")?,
        atc_id: text("atc_id")?,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The Cessna 172 parked near Izmir, as MSFS 2024 reported it
    /// (observed live 2026-10-26).
    pub(crate) fn c172() -> AircraftId {
        AircraftId {
            title: "C172SP G1000 Passengers".into(),
            atc_model: "ATCCOM.AC_MODEL C172.0.text".into(),
            atc_type: "ATCCOM.ATC_NAME CESSNA.0.text".into(),
            atc_id: "BILGE".into(),
        }
    }

    #[test]
    fn round_trips_through_json_and_rejects_a_missing_field() {
        let mut v = to_json(&c172());
        assert_eq!(from_json(&v), Some(c172()));
        v.as_object_mut().unwrap().remove("atc_id");
        assert_eq!(from_json(&v), None);
    }
}
