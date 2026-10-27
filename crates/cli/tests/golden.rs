//! The golden attempt a site's scorer CI checks against
//! (docs/challenge-protocol.md A11): `fixtures/challenge/` holds a
//! definition, the synthetic flight it was flown in (`flight.fqz`), that
//! flight's slice (`slice.fqz`, what the app uploads) and the `fdm
//! challenge` output on it (`attempt.json`). `FOQA_BLESS=1 cargo test -p
//! cli --test golden` writes them again.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use fdm_core::Lfl;
use recorder::{
    AttemptMarker, AttemptPhase, JournalWriter, Record, Recording, SimEvent, read_records,
};
use serde_json::Value;
use testkit::*;

const REF: &str = "Qe3v1hJ0dEa9fP2kLm7xRw";
/// What the flight carries and the slice must not: the tail number, the
/// sim title, the flight file's folder, the pilot's note.
const PRIVATE: [&str; 4] = ["N123FQ", "C172 test", "Documents", "long flare"];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/challenge")
}

fn blessing() -> bool {
    env::var_os("FOQA_BLESS").is_some()
}

fn marker(phase: AttemptPhase, at_s: f64) -> (f64, SimEvent) {
    let m = AttemptMarker {
        phase,
        challenge: "test-20".into(),
        version: 1,
        reference: Some(REF.into()),
        reason: (phase == AttemptPhase::End).then(|| "stopped".into()),
    };
    (at_s, SimEvent::Attempt(m))
}

/// The synthetic C172 landing on runway 20 of `TEST`, as a compact flight,
/// with what a real flight carries besides. The attempt begins 20 s in and
/// ends 40 s after touchdown. Only written when blessing: another
/// platform's maths may differ in the last bit.
fn flight() -> Vec<u8> {
    let events = [
        (
            0.0,
            SimEvent::FlightLoaded(r"C:\Users\Ege\Documents\c172.FLT".into()),
        ),
        (0.0, identity("C172")),
        (0.0, SimEvent::Airport(runway_behind_touchdown(400.0))),
        marker(AttemptPhase::Begin, 20.0),
        (60.0, SimEvent::PilotReport("long flare".into())),
        marker(AttemptPhase::End, TD_S + 40.0),
    ];
    let journal = journal(&events, LIGHT_LB, TD_S + ROLLOUT_S, |t| {
        landing(t, TD_S, 1.2)
    });
    let records = read_records(&journal[..]).unwrap();
    let mut compact = Vec::new();
    Recording::from_records(records.lfl_version(), records)
        .unwrap()
        .write(&mut compact)
        .unwrap();
    compact
}

fn fdm(args: &[&Path]) -> Vec<u8> {
    let out = Command::new(env!("CARGO_BIN_EXE_fdm"))
        .args(args)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    out.stdout
}

/// `fdm slice` on the golden flight gives the golden slice, byte for byte,
/// without the flight's private details.
#[test]
fn the_golden_flight_slices_to_the_golden_slice() {
    let (flight_path, slice) = (dir().join("flight.fqz"), dir().join("slice.fqz"));
    if blessing() {
        fs::write(&flight_path, flight()).unwrap();
    }
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("slice.fqz");
    let args = [
        "slice".as_ref(),
        flight_path.as_path(),
        "--ref".as_ref(),
        REF.as_ref(),
        "-o".as_ref(),
        &out,
    ];
    let report: Value = serde_json::from_slice(&fdm(&args)).unwrap();
    assert_eq!(report["complete"], true, "{report}");
    let bytes = fs::read(&out).unwrap();
    if blessing() {
        fs::write(&slice, &bytes).unwrap();
    }
    assert_eq!(bytes, fs::read(&slice).unwrap(), "slice.fqz is stale");

    let (sliced, whole) = (plain(&bytes), plain(&fs::read(&flight_path).unwrap()));
    for secret in PRIVATE {
        assert!(contains(&whole, secret), "{secret} not in the flight");
        assert!(!contains(&sliced, secret), "{secret} in the slice");
        assert!(!contains(&bytes, secret), "{secret} in the slice file");
    }
    assert!(contains(&sliced, REF) && contains(&sliced, "C172"));
}

/// `fdm challenge` on the golden slice gives the golden attempt.
#[test]
fn the_golden_slice_grades_to_the_golden_attempt() {
    let (def, slice, expected) = (
        dir().join("challenge.json"),
        dir().join("slice.fqz"),
        dir().join("attempt.json"),
    );
    let out = fdm(&["challenge".as_ref(), &def, &slice]);
    if blessing() {
        fs::write(&expected, &out).unwrap();
    }
    let got: Value = serde_json::from_slice(&out).unwrap();
    let want: Value = serde_json::from_slice(&fs::read(&expected).unwrap()).unwrap();
    assert!(same(&got, &want), "attempt.json is stale:\n{got:#}");
    assert_eq!(got["valid"], true, "{got:#}");
    assert_eq!(got["marker"]["ref"], REF);
    assert_eq!(got["aircraft"]["icao"], "C172");
}

/// An end marker crafted at the end of time neither wraps the window nor
/// panics: the slice runs to the end of the data, not complete.
#[test]
fn an_end_marker_at_the_end_of_time_slices_to_the_end_of_the_data() {
    let whole = fs::read(dir().join("flight.fqz")).unwrap();
    let records = read_records(&whole[..]).unwrap();
    let mut journal = JournalWriter::new(Vec::new(), &Lfl::V1).unwrap();
    for record in records {
        match record.unwrap() {
            Record::Sample(s) => journal.sample(&s).unwrap(),
            Record::Event(_, SimEvent::Attempt(m)) if m.phase == AttemptPhase::End => {}
            Record::Event(t, e) => journal.event(t, &e).unwrap(),
        }
    }
    let (_, end) = marker(AttemptPhase::End, 0.0);
    let last = fdm_core::SimTime::from_micros(i64::MAX - 1);
    journal.event(last, &end).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let (flight, out) = (tmp.path().join("crafted.fqj"), tmp.path().join("out.fqz"));
    fs::write(&flight, journal.into_inner()).unwrap();
    let args = [
        "slice".as_ref(),
        flight.as_path(),
        "--ref".as_ref(),
        REF.as_ref(),
        "-o".as_ref(),
        &out,
    ];
    let report: Value = serde_json::from_slice(&fdm(&args)).unwrap();
    assert_eq!(report["complete"], false, "{report}");
    assert_eq!(report["to_s"], i64::MAX as f64 / 1e6, "{report}");
    let sliced = fs::read(&out).unwrap();
    let samples = read_records(&sliced[..])
        .unwrap()
        .filter(|r| matches!(r, Ok(Record::Sample(_))))
        .count();
    assert!(samples > 1000, "{samples} samples");
}

/// The records of a flight file as a journal: every event's text in plain.
fn plain(file: &[u8]) -> Vec<u8> {
    let records = read_records(file).unwrap();
    let mut journal = JournalWriter::new(Vec::new(), &Lfl::V1).unwrap();
    for record in records {
        match record.unwrap() {
            Record::Sample(s) => journal.sample(&s).unwrap(),
            Record::Event(t, e) => journal.event(t, &e).unwrap(),
        }
    }
    journal.into_inner()
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// Equal, numbers to 1e-9 relative: another platform's maths may differ
/// in the last bit.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0)
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}
