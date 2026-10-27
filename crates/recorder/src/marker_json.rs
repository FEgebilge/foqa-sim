//! Attempt marker as JSON, the same encoding in the journal (event text)
//! and in the raw capture (docs/challenge-protocol.md A7).

use serde_json::{Value, json};

use crate::{AttemptMarker, AttemptPhase, is_reference};

pub(crate) fn to_json(m: &AttemptMarker) -> Value {
    let mut v = json!({
        "phase": match m.phase {
            AttemptPhase::Begin => "begin",
            AttemptPhase::End => "end",
        },
        "challenge": m.challenge,
        "version": m.version,
        "ref": m.reference,
    });
    if let Some(reason) = &m.reason {
        v["reason"] = reason.as_str().into();
    }
    v
}

/// `None` for an unknown phase, a missing field or a malformed reference.
/// Unknown keys are ignored.
pub(crate) fn from_json(v: &Value) -> Option<AttemptMarker> {
    let phase = match v.get("phase")?.as_str()? {
        "begin" => AttemptPhase::Begin,
        "end" => AttemptPhase::End,
        _ => return None,
    };
    let reference = match v.get("ref")? {
        Value::Null => None,
        r => Some(r.as_str().filter(|r| is_reference(r))?.to_owned()),
    };
    let reason = match v.get("reason") {
        None | Some(Value::Null) => None,
        Some(r) => Some(r.as_str()?.to_owned()),
    };
    Some(AttemptMarker {
        phase,
        challenge: v.get("challenge")?.as_str()?.to_owned(),
        version: u32::try_from(v.get("version")?.as_u64()?).ok()?,
        reference,
        reason,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn begin(reference: Option<&str>) -> AttemptMarker {
        AttemptMarker {
            phase: AttemptPhase::Begin,
            challenge: "ltfj-06-gusty-2026w44".into(),
            version: 1,
            reference: reference.map(Into::into),
            reason: None,
        }
    }

    #[test]
    fn matches_the_protocol_text_and_round_trips() {
        let m = begin(Some("Qe3v1hJ0dEa9fP2kLm7xRw"));
        assert_eq!(
            to_json(&m).to_string(),
            r#"{"challenge":"ltfj-06-gusty-2026w44","phase":"begin","ref":"Qe3v1hJ0dEa9fP2kLm7xRw","version":1}"#
        );
        let end = AttemptMarker {
            phase: AttemptPhase::End,
            reason: Some("stopped".into()),
            ..begin(None)
        };
        for m in [m, end] {
            assert_eq!(from_json(&to_json(&m)), Some(m));
        }
    }

    #[test]
    fn rejects_a_bad_reference_or_phase() {
        for (key, value) in [
            ("ref", json!("short")),
            ("ref", json!("has space in it")),
            ("ref", json!(7)),
            ("phase", json!("middle")),
            ("version", json!(-1)),
        ] {
            let mut v = to_json(&begin(Some("Qe3v1hJ0dEa9fP2kLm7xRw")));
            v[key] = value;
            assert_eq!(from_json(&v), None, "{key}");
        }
        assert!(is_reference(&"a".repeat(64)) && !is_reference(&"a".repeat(65)));
    }
}
