//! Aircraft identity as JSON, the same encoding in the journal (event text)
//! and in the raw capture, so a replay reproduces the journal exactly.

use fdm_core::{AircraftId, SimFacts};
use serde_json::{Value, json};

/// The sim facts are written only where known, so an identity without them
/// encodes as before 0.11.
pub(crate) fn to_json(a: &AircraftId) -> Value {
    let mut v = json!({ "title": a.title, "atc_model": a.atc_model, "atc_type": a.atc_type, "atc_id": a.atc_id });
    let f = &a.facts;
    let facts = [
        ("category", f.category.as_ref().map(|c| json!(c))),
        ("engine_type", f.engine_type.map(|x| json!(x))),
        ("engines", f.engines.map(|x| json!(x))),
        ("retractable", f.retractable.map(|x| json!(x))),
        ("floats", f.floats.map(|x| json!(x))),
        ("skids", f.skids.map(|x| json!(x))),
        ("max_gross_lb", f.max_gross_lb.map(|x| json!(x))),
    ];
    for (key, value) in facts {
        if let Some(value) = value {
            v[key] = value;
        }
    }
    v
}

/// `None` if any of the four identity strings is missing or not a string;
/// a missing or malformed sim fact is unknown.
pub(crate) fn from_json(v: &Value) -> Option<AircraftId> {
    let text = |key| Some(v.get(key)?.as_str()?.to_owned());
    let small = |key| u8::try_from(v.get(key)?.as_u64()?).ok();
    let flag = |key| v.get(key)?.as_bool();
    Some(AircraftId {
        title: text("title")?,
        atc_model: text("atc_model")?,
        atc_type: text("atc_type")?,
        atc_id: text("atc_id")?,
        facts: SimFacts {
            category: text("category"),
            engine_type: small("engine_type"),
            engines: small("engines"),
            retractable: flag("retractable"),
            floats: flag("floats"),
            skids: flag("skids"),
            max_gross_lb: v
                .get("max_gross_lb")
                .and_then(Value::as_u64)
                .and_then(|x| u32::try_from(x).ok()),
        },
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
            facts: SimFacts::default(),
        }
    }

    #[test]
    fn round_trips_through_json_and_rejects_a_missing_field() {
        let mut v = to_json(&c172());
        assert_eq!(from_json(&v), Some(c172()));
        v.as_object_mut().unwrap().remove("atc_id");
        assert_eq!(from_json(&v), None);
    }

    #[test]
    fn sim_facts_round_trip_and_an_old_identity_has_none() {
        let id = AircraftId {
            facts: SimFacts {
                category: Some("Airplane".into()),
                engine_type: Some(5),
                engines: Some(1),
                retractable: Some(true),
                floats: Some(false),
                skids: None,
                max_gross_lb: Some(10_450),
            },
            ..c172()
        };
        let v = to_json(&id);
        assert_eq!(v["engine_type"], 5);
        assert!(v.get("skids").is_none());
        assert_eq!(from_json(&v), Some(id));
        // As written before 0.11.
        assert_eq!(to_json(&c172()).as_object().unwrap().len(), 4);
        let mut bad = to_json(&c172());
        bad["engines"] = json!("two");
        assert_eq!(from_json(&bad).unwrap().facts, SimFacts::default());
    }
}
