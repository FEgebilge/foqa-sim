//! The local attempt lifecycle (docs/challenge-protocol.md B5): one folder
//! per attempt with `attempt.json`, the definition flown (`challenge.json`),
//! the local grade (`result.local.json`), the recording slice of an
//! attempt with a marker reference (`slice.fqz`) and an append-only
//! `log.jsonl`.
//! Knows no site, token or upload state; those live in the `connect`
//! sidecar.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use challenge::{Attempt, Challenge};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::commands::lock;
use crate::library::sha256_hex;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const RECORD: &str = "attempt.json";
const DEFINITION: &str = "challenge.json";
const RESULT: &str = "result.local.json";
const LOG: &str = "log.jsonl";
/// What a site receives (A5).
pub const SLICE: &str = "slice.fqz";

/// Core states (B5). The core ends at `Done`. An attempt with a marker
/// reference passes `Sliced` once its slice is cut; one without goes from
/// `GradedLocal` to `Done`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Created,
    Placing,
    FailedStart,
    Flying,
    Approach,
    Rollout,
    Finished,
    GradedLocal,
    Sliced,
    Done,
    Abandoned,
}

impl State {
    /// The moves the lifecycle allows.
    pub fn can_become(self, to: State) -> bool {
        use State::*;
        matches!(
            (self, to),
            (Created, Placing)
                | (Placing, Flying | FailedStart)
                | (Flying, Approach | Rollout | Finished)
                | (Approach, Rollout | Finished)
                | (Rollout, Finished)
                | (Finished, GradedLocal)
                | (GradedLocal, Sliced | Done)
                | (Sliced, Done)
                | (Created | Placing | Flying | Approach | Rollout, Abandoned)
        )
    }

    /// Between creation and the grade: what a crash leaves to recover
    /// (an attempt graded but not sliced is recovered apart).
    fn interrupted(self) -> bool {
        use State::*;
        matches!(
            self,
            Created | Placing | Flying | Approach | Rollout | Finished
        )
    }

    /// Started and without its end marker yet: one at a time, so a second
    /// begin marker never lands inside an attempt's flight.
    fn open(self) -> bool {
        use State::*;
        matches!(self, Created | Placing | Flying | Approach | Rollout)
    }

    /// Flown and not yet landed.
    fn airborne(self) -> bool {
        matches!(self, State::Flying | State::Approach)
    }
}

/// `attempt.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// UUID v4; also the folder name (and later `client_attempt_id`).
    pub id: String,
    /// The library entry it was started from, for Fly again.
    pub key: String,
    pub challenge: String,
    pub version: u32,
    pub title: String,
    pub definition_sha256: String,
    /// Attempt number on this challenge on this PC.
    pub n: u32,
    pub state: State,
    pub created_at: String,
    pub updated_at: String,
    pub flight_id: Option<String>,
    /// The opaque reference its attempt markers carry (A7), chosen by the
    /// module that issued it; `None` for a local attempt, which is never
    /// sliced.
    #[serde(rename = "ref", default)]
    pub marker_ref: Option<String>,
    /// Sim time the flight had reached when the aircraft was placed: only
    /// a landing after it is graded, until the recording carries the
    /// attempt marker (A7).
    pub begin_s: Option<f64>,
    pub touchdown_s: Option<f64>,
    /// Why it finished or was abandoned.
    pub reason: Option<String>,
    /// Why its slice can never be cut (`no_marker`, `no_flight`): it is
    /// done without one and never uploaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slice_failed: Option<String>,
}

/// An attempt as the Challenges screen lists it.
#[derive(Debug, Clone, Serialize)]
pub struct View {
    #[serde(flatten)]
    pub record: Record,
    /// `challenge::Attempt`, once graded.
    pub result: Option<Value>,
    pub log: Vec<Value>,
}

/// Grades an attempt from its record and the definition flown.
pub type Grade<'a> = &'a dyn Fn(&Record, &str) -> Result<Attempt, String>;

/// Why a slice was not cut.
#[derive(Debug, Clone, PartialEq)]
pub enum Uncut {
    /// It may be later (the flight file locked or unreadable).
    Later(String),
    /// It never can be: the code (`no_marker`, `no_flight`) and the error.
    Never(&'static str, String),
}

/// A recording slice cut for an attempt.
#[derive(Debug, Clone, PartialEq)]
pub struct Cut {
    pub bytes: Vec<u8>,
    /// The window it covers, sim s.
    pub from_s: f64,
    pub to_s: f64,
}

/// The attempt folders, written by one owner at a time.
pub struct Attempts {
    dir: PathBuf,
    guard: Mutex<()>,
    /// Attempts being finished and graded right now.
    grading: Mutex<HashSet<String>>,
    /// Attempts whose slice is being cut right now.
    slicing: Mutex<HashSet<String>>,
    changed: Box<dyn Fn(&Record) + Send + Sync>,
}

impl Attempts {
    pub fn new(dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            guard: Mutex::new(()),
            grading: Mutex::new(HashSet::new()),
            slicing: Mutex::new(HashSet::new()),
            changed: Box::new(|_| {}),
        })
    }

    /// Called after every state change, from the thread that made it.
    pub fn on_change(mut self, f: impl Fn(&Record) + Send + Sync + 'static) -> Self {
        self.changed = Box::new(f);
        self
    }

    /// A new attempt at the definition `json`, from library entry `key`;
    /// refused while another attempt is open.
    pub fn create(&self, key: &str, json: &str) -> Result<Record, String> {
        self.create_beside(key, json, true)
    }

    /// `create`; `alone` refuses it while another attempt is open.
    fn create_beside(&self, key: &str, json: &str, alone: bool) -> Result<Record, String> {
        let c = Challenge::parse(json)?;
        let _held = lock(&self.guard);
        if let Some(open) = self.records().find(|r| alone && r.state.open()) {
            return Err(format!(
                "Attempt {} at {} is still open: finish or abandon it first.",
                open.n, open.title
            ));
        }
        let n = self.records().filter(|r| r.key == key).count() as u32 + 1;
        let now = now();
        let record = Record {
            id: Uuid::new_v4().to_string(),
            key: key.to_owned(),
            challenge: c.id,
            version: c.version,
            title: c.title,
            definition_sha256: sha256_hex(json.as_bytes()),
            n,
            state: State::Created,
            created_at: now.clone(),
            updated_at: now,
            flight_id: None,
            marker_ref: None,
            begin_s: None,
            touchdown_s: None,
            reason: None,
            slice_failed: None,
        };
        let dir = self.dir.join(&record.id);
        fs::create_dir_all(&dir).map_err(text)?;
        fs::write(dir.join(DEFINITION), json).map_err(text)?;
        write_record(&dir, &record)?;
        let fields = json!({
            "challenge": record.challenge, "version": record.version,
            "definition_sha256": record.definition_sha256, "app": APP_VERSION,
        });
        append(&dir, "created", fields)?;
        (self.changed)(&record);
        Ok(record)
    }

    pub fn get(&self, id: &str) -> Result<Record, String> {
        read_record(&self.folder(id)?)
    }

    /// The exact definition bytes the attempt flies.
    pub fn definition(&self, id: &str) -> Result<String, String> {
        fs::read_to_string(self.folder(id)?.join(DEFINITION)).map_err(text)
    }

    /// Every attempt with its grade and log, the newest first.
    pub fn list(&self) -> Vec<View> {
        let mut views: Vec<View> = self
            .records()
            .map(|record| {
                let dir = self.dir.join(&record.id);
                let result = fs::read_to_string(dir.join(RESULT))
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok());
                View {
                    log: read_log(&dir),
                    result,
                    record,
                }
            })
            .collect();
        views.sort_by(|a, b| b.record.created_at.cmp(&a.record.created_at));
        views
    }

    /// Moves `id` to `to`, logging `event` with `fields`; `edit` changes the
    /// record on the way. Refused when the lifecycle does not allow it.
    fn step(
        &self,
        id: &str,
        to: State,
        event: &str,
        fields: Value,
        edit: impl FnOnce(&mut Record),
    ) -> Result<Record, String> {
        let dir = self.folder(id)?;
        let record = {
            let _held = lock(&self.guard);
            let mut record = read_record(&dir)?;
            if !record.state.can_become(to) {
                return Err(format!(
                    "attempt {} is {:?}; it can't become {to:?}",
                    record.n, record.state
                ));
            }
            edit(&mut record);
            record.state = to;
            record.updated_at = now();
            write_record(&dir, &record)?;
            append(&dir, event, fields)?;
            record
        };
        (self.changed)(&record);
        Ok(record)
    }

    /// Logs `event` without a state change.
    fn note(&self, id: &str, event: &str, fields: Value) -> Result<(), String> {
        let dir = self.folder(id)?;
        let _held = lock(&self.guard);
        append(&dir, event, fields)
    }

    /// Places the aircraft with `place`, which returns what to log with
    /// `placed`; on its error the attempt is `failed_start` and the error
    /// returned. `marker_ref` is the reference its markers carry.
    pub fn place(
        &self,
        id: &str,
        flight_id: &str,
        begin_s: Option<f64>,
        marker_ref: Option<&str>,
        place: impl FnOnce(&Challenge) -> Result<Value, String>,
    ) -> Result<Record, String> {
        let challenge = Challenge::parse(&self.definition(id)?)?;
        self.step(id, State::Placing, "placing", json!({}), |_| {})?;
        match place(&challenge) {
            Ok(mut fields) => {
                if let Value::Object(map) = &mut fields {
                    map.insert("flight_id".into(), flight_id.into());
                    map.insert("ref".into(), marker_ref.into());
                }
                self.step(id, State::Flying, "placed", fields, |r| {
                    r.flight_id = Some(flight_id.to_owned());
                    r.marker_ref = marker_ref.map(Into::into);
                    r.begin_s = begin_s;
                })
            }
            Err(e) => {
                self.step(
                    id,
                    State::FailedStart,
                    "failed_start",
                    json!({ "error": e }),
                    |_| {},
                )?;
                Err(e)
            }
        }
    }

    pub fn abandon(&self, id: &str, reason: &str) -> Result<Record, String> {
        self.step(
            id,
            State::Abandoned,
            "abandoned",
            json!({ "reason": reason }),
            |r| {
                r.reason = Some(reason.to_owned());
            },
        )
    }

    /// A landing on `flight_id` at sim time `t_s`: the attempts flying in
    /// it move to the rollout. Returns their ids.
    pub fn landed(&self, flight_id: &str, t_s: f64, sink_fpm: f64, peak_g: f64) -> Vec<String> {
        let landed = self.records().filter(|r| {
            r.state.airborne()
                && r.flight_id.as_deref() == Some(flight_id)
                && r.begin_s.is_none_or(|b| t_s >= b)
        });
        let fields = json!({ "sim_s": t_s, "sink_fpm": sink_fpm, "peak_g": peak_g });
        landed
            .filter_map(|r| {
                let moved = self.step(&r.id, State::Rollout, "touchdown", fields.clone(), |r| {
                    r.touchdown_s = Some(t_s);
                });
                moved.map_err(|e| eprintln!("attempt {}: {e}", r.id)).ok()
            })
            .map(|r| r.id)
            .collect()
    }

    /// The flight `flight_id` closed: its attempts still in the air are
    /// abandoned; those on the rollout are returned, to finish.
    pub fn flight_closed(&self, flight_id: &str) -> Vec<String> {
        let mut rolling = Vec::new();
        for r in self
            .records()
            .filter(|r| r.flight_id.as_deref() == Some(flight_id))
        {
            if r.state.airborne() {
                if let Err(e) = self.abandon(&r.id, "flight_closed") {
                    eprintln!("attempt {}: {e}", r.id);
                }
            } else if r.state == State::Rollout {
                rolling.push(r.id);
            }
        }
        rolling
    }

    /// Ends the attempt (`reason`: `stopped`, `finish`, …) and grades it;
    /// an attempt left `finished` by a failed grade is graded again.
    /// A finish already running for `id` is not repeated: the attempt as it
    /// stands is returned.
    pub fn finish(&self, id: &str, reason: &str, grade: Grade) -> Result<Record, String> {
        if !lock(&self.grading).insert(id.to_owned()) {
            return self.get(id);
        }
        let result = self.finish_claimed(id, reason, grade);
        lock(&self.grading).remove(id);
        result
    }

    fn finish_claimed(&self, id: &str, reason: &str, grade: Grade) -> Result<Record, String> {
        let mut record = self.get(id)?;
        if record.state != State::Finished {
            record = self.step(
                id,
                State::Finished,
                "finished",
                json!({ "reason": reason }),
                |r| {
                    r.reason = Some(reason.to_owned());
                },
            )?;
        }
        let result = grade(&record, &self.definition(id)?);
        self.graded(record, result)
    }

    /// Records a grade: the gate and touchdown it found, the result file,
    /// then `graded_local`, and `done` unless it waits for its slice. A
    /// failed grade is logged and the attempt stays `finished`.
    fn graded(&self, record: Record, result: Result<Attempt, String>) -> Result<Record, String> {
        let id = &record.id;
        let attempt = match result {
            Ok(a) => a,
            Err(e) => {
                self.note(id, "error", json!({ "during": "grading", "message": e }))?;
                return Err(e);
            }
        };
        if let Some(t) = attempt.gate_s {
            self.note(id, "gate", json!({ "sim_s": t }))?;
        }
        if let (None, Some(t)) = (record.touchdown_s, attempt.touchdown_s) {
            let part = |name| {
                let parts = attempt.score.as_ref().map(|s| s.parts.as_slice());
                parts
                    .into_iter()
                    .flatten()
                    .find(|p| p.name == name)
                    .and_then(|p| p.value)
            };
            let fields =
                json!({ "sim_s": t, "sink_fpm": part("sink_rate"), "peak_g": part("peak_g") });
            self.note(id, "touchdown", fields)?;
        }
        let json = serde_json::to_string_pretty(&attempt).map_err(text)?;
        fs::write(self.folder(id)?.join(RESULT), json).map_err(text)?;
        let failed: Vec<_> = attempt
            .checks
            .iter()
            .filter(|c| !c.ok)
            .map(|c| c.name)
            .collect();
        let fields = json!({
            "valid": attempt.valid,
            "score": attempt.score.as_ref().map(|s| s.total),
            "failed_checks": failed,
        });
        let touchdown_s = attempt.touchdown_s;
        let record = self.step(id, State::GradedLocal, "graded_local", fields, |r| {
            r.touchdown_s = r.touchdown_s.or(touchdown_s);
        })?;
        if record.marker_ref.is_some() {
            return Ok(record);
        }
        self.step(id, State::Done, "done", json!({}), |_| {})
    }

    /// Cuts the slice of a graded attempt with `cut`, writes it, then
    /// `sliced` and `done`. A failed cut is logged; the attempt stays
    /// `graded_local` to be cut again (`unsliced`), unless it never can be:
    /// then it is `done` without a slice (`slice_failed`). A cut already
    /// running for `id` is not repeated.
    pub fn slice(
        &self,
        id: &str,
        cut: impl FnOnce(&Record) -> Result<Cut, Uncut>,
    ) -> Result<Record, String> {
        if !lock(&self.slicing).insert(id.to_owned()) {
            return self.get(id);
        }
        let result = self.slice_claimed(id, cut);
        lock(&self.slicing).remove(id);
        result
    }

    fn slice_claimed(
        &self,
        id: &str,
        cut: impl FnOnce(&Record) -> Result<Cut, Uncut>,
    ) -> Result<Record, String> {
        let record = self.get(id)?;
        if record.state != State::GradedLocal || record.marker_ref.is_none() {
            return Err(format!("attempt {} has no slice to cut", record.n));
        }
        let dir = self.folder(id)?;
        let written = cut(&record).and_then(|c| {
            let part = dir.join(format!("{SLICE}.part"));
            fs::write(&part, &c.bytes).map_err(|e| Uncut::Later(e.to_string()))?;
            fs::rename(part, dir.join(SLICE)).map_err(|e| Uncut::Later(e.to_string()))?;
            Ok(c)
        });
        let c = match written {
            Ok(c) => c,
            Err(Uncut::Later(e)) => {
                self.note(id, "error", json!({ "during": "slicing", "message": e }))?;
                return Err(e);
            }
            Err(Uncut::Never(code, e)) => {
                let fields = json!({ "during": "slicing", "message": e, "final": true });
                self.note(id, "error", fields)?;
                self.step(
                    id,
                    State::Done,
                    "done",
                    json!({ "slice_failed": code }),
                    |r| {
                        r.slice_failed = Some(code.to_owned());
                    },
                )?;
                return Err(e);
            }
        };
        let fields = json!({
            "bytes": c.bytes.len(), "sha256": sha256_hex(&c.bytes),
            "from_s": c.from_s, "to_s": c.to_s,
        });
        self.step(id, State::Sliced, "sliced", fields, |_| {})?;
        self.step(id, State::Done, "done", json!({}), |_| {})
    }

    /// Attempts graded but waiting for their slice: a failed cut or a crash.
    pub fn unsliced(&self) -> Vec<String> {
        let waiting = self
            .records()
            .filter(|r| r.state == State::GradedLocal && r.marker_ref.is_some());
        waiting.map(|r| r.id).collect()
    }

    /// At start-up: each attempt a crash left between placing and its
    /// grade is resolved from its flight. A touchdown after the start
    /// finishes it (`recovered`) and grades it; otherwise it is abandoned
    /// (`interrupted`).
    pub fn recover(&self, grade: Grade) {
        let stuck: Vec<Record> = self.records().filter(|r| r.state.interrupted()).collect();
        for r in stuck {
            let id = r.id.clone();
            let resolved = match r.state {
                State::Created | State::Placing => self.abandon(&id, "interrupted"),
                State::Finished => self.finish(&id, "recovered", grade),
                _ => match grade(&r, &self.definition(&id).unwrap_or_default()) {
                    Ok(a) if a.touchdown_s.is_some() => self
                        .step(
                            &id,
                            State::Finished,
                            "finished",
                            json!({ "reason": "recovered" }),
                            |r| {
                                r.reason = Some("recovered".into());
                            },
                        )
                        .and_then(|r| self.graded(r, Ok(a))),
                    // Graded, but no landing: it never finished.
                    Ok(_) => self.abandon(&id, "interrupted"),
                    // Not graded (file locked, flight unreadable): kept for
                    // the next start-up.
                    Err(e) => {
                        let fields = json!({ "during": "recovery", "message": e });
                        let _ = self.note(&id, "error", fields);
                        Err(e)
                    }
                },
            };
            if let Err(e) = resolved {
                eprintln!("attempt {id}: not recovered: {e}");
            }
        }
    }

    /// The folder of attempt `id`; ids are UUIDs, so nothing else can name
    /// a path outside the attempts folder.
    fn folder(&self, id: &str) -> Result<PathBuf, String> {
        let uuid = Uuid::parse_str(id).map_err(|_| format!("no attempt {id:?}"))?;
        let dir = self.dir.join(uuid.hyphenated().to_string());
        dir.is_dir()
            .then_some(dir)
            .ok_or_else(|| format!("no attempt {id:?}"))
    }

    fn records(&self) -> impl Iterator<Item = Record> {
        let entries = fs::read_dir(&self.dir).into_iter().flatten().flatten();
        entries.filter_map(|e| read_record(&e.path()).ok())
    }
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn text(e: impl ToString) -> String {
    e.to_string()
}

fn read_record(dir: &Path) -> Result<Record, String> {
    let json = fs::read_to_string(dir.join(RECORD)).map_err(text)?;
    serde_json::from_str(&json).map_err(text)
}

/// Written aside and renamed, so a crash never leaves half a record.
fn write_record(dir: &Path, record: &Record) -> Result<(), String> {
    let part = dir.join(format!("{RECORD}.part"));
    fs::write(&part, serde_json::to_vec_pretty(record).map_err(text)?).map_err(text)?;
    fs::rename(part, dir.join(RECORD)).map_err(text)
}

/// One line `{at, seq, event, …fields}`, after the last; never rewritten.
fn append(dir: &Path, event: &str, fields: Value) -> Result<(), String> {
    let path = dir.join(LOG);
    let seq = read_log(dir)
        .iter()
        .filter_map(|l| l["seq"].as_u64())
        .max()
        .unwrap_or(0)
        + 1;
    // A line cut off by a crash ends there, so this one starts on its own.
    let cut = fs::read(&path).is_ok_and(|b| b.last().is_some_and(|&c| c != b'\n'));
    let mut line = Map::new();
    line.insert("at".into(), now().into());
    line.insert("seq".into(), seq.into());
    line.insert("event".into(), event.into());
    if let Value::Object(fields) = fields {
        line.extend(fields);
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(text)?;
    let start = if cut { "\n" } else { "" };
    writeln!(file, "{start}{}", Value::Object(line)).map_err(text)
}

/// The log's lines; a line cut off by a crash is skipped.
fn read_log(dir: &Path) -> Vec<Value> {
    let bytes = fs::read(dir.join(LOG)).unwrap_or_default();
    bytes
        .split(|&b| b == b'\n')
        .filter_map(|l| serde_json::from_slice(l).ok())
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use challenge::{Check, Part, Score};
    use std::thread;

    pub(crate) const JSON: &str = r#"{
      "id": "test-20", "version": 1, "title": "Runway 20",
      "airport": "TEST", "runway": "20",
      "start": { "distance_nm": 8, "height_ft": 2500, "ias_kt": 160, "mode": "teleport" },
      "gate": { "distance_nm": 3 },
      "scoring": { "version": "landing-1" }
    }"#;

    fn attempts() -> (tempfile::TempDir, Attempts) {
        let tmp = tempfile::tempdir().unwrap();
        let attempts = Attempts::new(tmp.path().join("attempts")).unwrap();
        (tmp, attempts)
    }

    fn events(a: &Attempts, id: &str) -> Vec<String> {
        let dir = a.folder(id).unwrap();
        read_log(&dir)
            .iter()
            .map(|l| l["event"].as_str().unwrap().to_owned())
            .collect()
    }

    fn graded(touchdown_s: Option<f64>) -> Attempt {
        Attempt {
            challenge: "test-20".into(),
            challenge_version: 1,
            aircraft: challenge::Flown {
                icao: Some("B738".into()),
                class: Some("transport".into()),
            },
            marker: None,
            valid: touchdown_s.is_some(),
            checks: vec![Check {
                name: "runway",
                ok: touchdown_s.is_some(),
                detail: String::new(),
            }],
            gate_s: touchdown_s.map(|t| t - 60.0),
            touchdown_s,
            score: touchdown_s.map(|_| Score {
                version: "landing-1",
                total: 87.4,
                parts: vec![Part {
                    name: "sink_rate",
                    weight: 25.0,
                    score: 1.0,
                    value: Some(142.0),
                    unit: "fpm",
                }],
            }),
            assessment: None,
        }
    }

    /// An attempt placed in flight-1 at sim time `begin_s`.
    fn flying(a: &Attempts, begin_s: f64) -> Record {
        flying_as(a, begin_s, None)
    }

    /// Beside any open attempt: what an older app could leave behind.
    fn created(a: &Attempts) -> Record {
        a.create_beside("test-20-v1", JSON, false).unwrap()
    }

    fn flying_as(a: &Attempts, begin_s: f64, marker_ref: Option<&str>) -> Record {
        let r = created(a);
        a.place(&r.id, "flight-1", Some(begin_s), marker_ref, |_| {
            Ok(json!({ "start": { "alt_ft": 2600 } }))
        })
        .unwrap()
    }

    #[test]
    fn the_lifecycle_allows_only_its_moves() {
        use State::*;
        assert!(Created.can_become(Placing) && Placing.can_become(FailedStart));
        assert!(Flying.can_become(Rollout) && Rollout.can_become(Finished));
        assert!(GradedLocal.can_become(Done) && Sliced.can_become(Done));
        assert!(!Created.can_become(Flying), "placing comes first");
        assert!(!Done.can_become(Abandoned) && !Abandoned.can_become(Flying));
        assert!(
            !Finished.can_become(Abandoned),
            "a finished attempt is graded"
        );
    }

    #[test]
    fn an_attempt_is_created_placed_landed_and_graded_with_every_step_logged() {
        let (_tmp, a) = attempts();
        let r = flying(&a, 100.0);
        assert_eq!(
            (r.state, r.n, r.flight_id.as_deref()),
            (State::Flying, 1, Some("flight-1"))
        );
        assert_eq!(r.definition_sha256, sha256_hex(JSON.as_bytes()));
        // A landing before the start or on another flight is not the attempt's.
        assert!(a.landed("flight-1", 50.0, 300.0, 1.4).is_empty());
        assert!(a.landed("flight-2", 400.0, 300.0, 1.4).is_empty());
        assert_eq!(a.landed("flight-1", 400.0, 142.0, 1.18), [r.id.as_str()]);
        let done = a
            .finish(&r.id, "stopped", &|_, _| Ok(graded(Some(400.0))))
            .unwrap();
        assert_eq!(
            (done.state, done.reason.as_deref()),
            (State::Done, Some("stopped"))
        );
        assert_eq!(
            events(&a, &r.id),
            [
                "created",
                "placing",
                "placed",
                "touchdown",
                "finished",
                "gate",
                "graded_local",
                "done"
            ]
        );
        let view = &a.list()[0];
        assert_eq!(view.result.as_ref().unwrap()["score"]["total"], 87.4);
        let created = &view.log[0];
        assert_eq!(created["seq"], 1);
        assert_eq!(created["app"], APP_VERSION);
        assert_eq!(created["definition_sha256"], r.definition_sha256.as_str());
        assert_eq!(view.log[2]["flight_id"], "flight-1");
        assert_eq!(view.log[6]["score"], 87.4);
        assert!(
            view.log
                .iter()
                .zip(1..)
                .all(|(l, seq)| l["seq"] == seq && l["at"].is_string())
        );
    }

    #[test]
    fn an_attempt_with_a_reference_is_sliced_before_it_is_done() {
        let (_tmp, a) = attempts();
        let r = flying_as(&a, 100.0, Some("Qe3v1hJ0dEa9fP2kLm7xRw"));
        assert_eq!(r.marker_ref.as_deref(), Some("Qe3v1hJ0dEa9fP2kLm7xRw"));
        let finished = a.finish(&r.id, "stopped", &|_, _| Ok(graded(Some(400.0))));
        assert_eq!(finished.unwrap().state, State::GradedLocal);
        assert_eq!(a.unsliced(), [r.id.as_str()]);

        // A failed cut is logged and cut again later.
        assert!(
            a.slice(&r.id, |_| Err(Uncut::Later("flight file locked".into())))
                .is_err()
        );
        assert_eq!(a.get(&r.id).unwrap().state, State::GradedLocal);
        let cut = Cut {
            bytes: b"FQJC slice".to_vec(),
            from_s: 90.0,
            to_s: 405.0,
        };
        let done = a.slice(&r.id, |_| Ok(cut)).unwrap();
        assert_eq!(done.state, State::Done);
        assert!(a.unsliced().is_empty());
        let dir = a.folder(&r.id).unwrap();
        assert_eq!(fs::read(dir.join(SLICE)).unwrap(), b"FQJC slice");
        let log = read_log(&dir);
        let sliced = log.iter().find(|l| l["event"] == "sliced").unwrap();
        assert_eq!(sliced["sha256"], sha256_hex(b"FQJC slice").as_str());
        assert_eq!(
            (sliced["bytes"].as_u64(), sliced["to_s"].as_f64()),
            (Some(10), Some(405.0))
        );
        assert_eq!(
            log[2]["ref"], "Qe3v1hJ0dEa9fP2kLm7xRw",
            "placed with its ref"
        );
        assert!(a.slice(&r.id, |_| unreachable!()).is_err(), "cut once");
        // A local attempt has nothing to slice.
        let local = flying(&a, 100.0);
        let done = a.finish(&local.id, "stopped", &|_, _| Ok(graded(Some(400.0))));
        assert_eq!(done.unwrap().state, State::Done);
    }

    #[test]
    fn a_slice_that_never_can_be_cut_ends_the_attempt_done_without_one() {
        let (_tmp, a) = attempts();
        let r = flying_as(&a, 100.0, Some("Qe3v1hJ0dEa9fP2kLm7xRw"));
        a.finish(&r.id, "stopped", &|_, _| Ok(graded(Some(400.0))))
            .unwrap();
        let never = Uncut::Never("no_marker", "flight-1 has no begin marker".into());
        assert!(a.slice(&r.id, |_| Err(never)).is_err());
        let done = a.get(&r.id).unwrap();
        assert_eq!(
            (done.state, done.slice_failed.as_deref()),
            (State::Done, Some("no_marker"))
        );
        assert!(a.unsliced().is_empty(), "never cut again");
        let log = read_log(&a.folder(&r.id).unwrap());
        assert_eq!(log.last().unwrap()["slice_failed"], "no_marker");
        assert!(!a.folder(&r.id).unwrap().join(SLICE).exists());
    }

    #[test]
    fn two_cuts_slice_an_attempt_once() {
        let (_tmp, a) = attempts();
        let r = flying_as(&a, 100.0, Some("Qe3v1hJ0dEa9fP2kLm7xRw"));
        a.finish(&r.id, "stopped", &|_, _| Ok(graded(Some(400.0))))
            .unwrap();
        let cut = || Cut {
            bytes: b"FQJC slice".to_vec(),
            from_s: 90.0,
            to_s: 405.0,
        };
        thread::scope(|s| {
            s.spawn(|| {
                a.slice(&r.id, |_| {
                    thread::sleep(std::time::Duration::from_millis(300));
                    Ok(cut())
                })
                .unwrap()
            });
            thread::sleep(std::time::Duration::from_millis(100));
            let busy = a.slice(&r.id, |_| unreachable!("already being cut"));
            assert_eq!(busy.unwrap().state, State::GradedLocal);
        });
        assert_eq!(a.get(&r.id).unwrap().state, State::Done);
    }

    #[test]
    fn a_failed_placement_or_grade_is_logged() {
        let (_tmp, a) = attempts();
        let r = a.create("test-20-v1", JSON).unwrap();
        let err = a.place(&r.id, "flight-1", None, None, |_| {
            Err("no answer from the sim".into())
        });
        assert_eq!(err.unwrap_err(), "no answer from the sim");
        assert_eq!(a.get(&r.id).unwrap().state, State::FailedStart);
        assert!(
            a.abandon(&r.id, "cancelled").is_err(),
            "a failed start is final"
        );

        let r = flying(&a, 100.0);
        assert_eq!(r.n, 2);
        assert!(
            a.finish(&r.id, "finish", &|_, _| Err("flight file locked".into()))
                .is_err()
        );
        assert_eq!(a.get(&r.id).unwrap().state, State::Finished);
        let log = &a.list()[0].log;
        assert_eq!(log.last().unwrap()["message"], "flight file locked");
        // Graded again, it completes; the touchdown found by the grade is logged.
        let done = a
            .finish(&r.id, "finish", &|_, _| Ok(graded(Some(400.0))))
            .unwrap();
        assert_eq!((done.state, done.touchdown_s), (State::Done, Some(400.0)));
        assert!(events(&a, &r.id).contains(&"touchdown".to_owned()));
    }

    #[test]
    fn one_attempt_is_open_at_a_time() {
        let (_tmp, a) = attempts();
        let r = flying(&a, 100.0);
        let refused = a.create("test-20-v1", JSON).unwrap_err();
        assert!(refused.contains("Attempt 1"), "{refused}");
        assert_eq!(a.list().len(), 1, "nothing created");
        a.landed("flight-1", 400.0, 142.0, 1.18);
        assert!(a.create("test-20-v1", JSON).is_err(), "rolling out");
        a.finish(&r.id, "stopped", &|_, _| Ok(graded(Some(400.0))))
            .unwrap();
        assert_eq!(a.create("test-20-v1", JSON).unwrap().n, 2);
    }

    #[test]
    fn closing_the_flight_abandons_an_attempt_in_the_air_and_finishes_one_rolling_out() {
        let (_tmp, a) = attempts();
        let (rolling, air) = (flying(&a, 100.0), flying(&a, 500.0));
        assert_eq!(
            a.landed("flight-1", 400.0, 142.0, 1.18),
            [rolling.id.as_str()]
        );
        assert_eq!(a.flight_closed("flight-1"), [rolling.id.as_str()]);
        let air = a.get(&air.id).unwrap();
        assert_eq!(
            (air.state, air.reason.as_deref()),
            (State::Abandoned, Some("flight_closed"))
        );
    }

    #[test]
    fn a_crash_is_recovered_from_the_flight() {
        let (_tmp, a) = attempts();
        let landed = flying(&a, 100.0);
        let lost = flying(&a, 100.0);
        let placing = created(&a);
        a.step(&placing.id, State::Placing, "placing", json!({}), |_| {})
            .unwrap();
        let unplaced = created(&a);
        let done = created(&a);
        a.abandon(&done.id, "cancelled").unwrap();

        let (landed_id, done_id) = (landed.id.clone(), done.id.clone());
        a.recover(&move |r, _| {
            assert_ne!(r.id, done_id, "a final attempt is left alone");
            Ok(graded((r.id == landed_id).then_some(400.0)))
        });
        let landed = a.get(&landed.id).unwrap();
        assert_eq!(
            (landed.state, landed.reason.as_deref()),
            (State::Done, Some("recovered"))
        );
        for id in [&lost.id, &placing.id, &unplaced.id] {
            let r = a.get(id).unwrap();
            assert_eq!(
                (r.state, r.reason.as_deref()),
                (State::Abandoned, Some("interrupted"))
            );
        }
        assert_eq!(
            a.get(&done.id).unwrap().reason.as_deref(),
            Some("cancelled")
        );
    }

    #[test]
    fn ids_never_leave_the_attempts_folder_and_a_cut_log_line_is_skipped() {
        let (_tmp, a) = attempts();
        assert!(a.get("../../Windows").is_err());
        assert!(a.get(&Uuid::new_v4().to_string()).is_err());
        let r = a.create("test-20-v1", JSON).unwrap();
        let log = a.folder(&r.id).unwrap().join(LOG);
        let mut file = OpenOptions::new().append(true).open(&log).unwrap();
        write!(file, "{{\"at\":\"2026-").unwrap();
        a.abandon(&r.id, "cancelled").unwrap();
        assert_eq!(events(&a, &r.id), ["created", "abandoned"]);
    }

    #[test]
    fn a_failed_grade_at_start_up_keeps_a_landed_attempt() {
        let (_tmp, a) = attempts();
        let r = flying(&a, 100.0);
        a.recover(&|_, _| Err("flight file locked".into()));
        assert_eq!(a.get(&r.id).unwrap().state, State::Flying);
        a.recover(&|_, _| Ok(graded(None)));
        assert_eq!(a.get(&r.id).unwrap().state, State::Abandoned);
    }

    #[test]
    fn two_finishes_grade_an_attempt_once() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let (_tmp, a) = attempts();
        let r = flying(&a, 100.0);
        let grades = AtomicU32::new(0);
        let slow = |_: &Record, _: &str| {
            grades.fetch_add(1, Ordering::SeqCst);
            thread::sleep(std::time::Duration::from_millis(300));
            Ok(graded(Some(400.0)))
        };
        thread::scope(|s| {
            s.spawn(|| a.finish(&r.id, "finish", &slow).unwrap());
            thread::sleep(std::time::Duration::from_millis(100));
            s.spawn(|| a.finish(&r.id, "stopped", &slow).unwrap());
        });
        assert_eq!(grades.load(Ordering::SeqCst), 1);
        assert_eq!(a.get(&r.id).unwrap().state, State::Done);
        assert_eq!(events(&a, &r.id).iter().filter(|e| *e == "done").count(), 1);
    }

    #[test]
    fn a_non_utf8_log_line_hides_nothing_and_seq_continues() {
        let (_tmp, a) = attempts();
        let r = a.create("test-20-v1", JSON).unwrap();
        let log = a.folder(&r.id).unwrap().join(LOG);
        let mut file = OpenOptions::new().append(true).open(&log).unwrap();
        file.write_all(b"{\"seq\":2,\"x\":\"\xC3\n").unwrap();
        file.write_all(b"{\"seq\":2,\"event\":\"noted\"}\n")
            .unwrap();
        a.abandon(&r.id, "cancelled").unwrap();
        let lines = read_log(&a.folder(&r.id).unwrap());
        assert_eq!(events(&a, &r.id), ["created", "noted", "abandoned"]);
        assert_eq!(lines[2]["seq"], 3);
    }
}
