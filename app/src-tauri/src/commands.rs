//! The commands of `docs/debrief-api.md` and the state they share with the
//! recorder.

use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ::debrief::{
    Debrief, FlightSummary, Index, LandingNotice, RecorderStatus, Station, StorageUsage,
};
use capture::{ASK_TIMEOUT, InitPosition, SimRequests, Start, Stopper};
use challenge::{Attempt, Challenge, Mode, StartState};
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::attempts::{Attempts, Record, State as AttemptState, View};
use crate::library::{Briefing, Entry, Library};
use crate::setup::{Setup, SetupStatus};

/// What the commands, the tray and the recorder thread share.
pub struct Shared {
    pub station: Arc<Station>,
    /// Where the recorder writes the flights.
    pub dir: PathBuf,
    /// The latest status the recorder reported.
    pub status: Mutex<RecorderStatus>,
    pub stopper: Stopper,
    pub recorder: Mutex<Option<JoinHandle<()>>>,
    pub setup: Setup,
    /// The flight list, analysed once per flight file; the recorder's.
    index: Arc<Index>,
    /// Requests to the sim through the recorder's connection.
    sim: SimRequests,
    /// Challenge files the user opened.
    library: Library,
    attempts: Attempts,
}

impl Shared {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        station: Arc<Station>,
        dir: PathBuf,
        stopper: Stopper,
        index: Arc<Index>,
        recorder: JoinHandle<()>,
        setup: Setup,
        sim: SimRequests,
        library: Library,
        attempts: Attempts,
    ) -> Self {
        Self {
            library,
            attempts,
            sim,
            station,
            status: Mutex::new(RecorderStatus::WaitingForSim),
            stopper,
            recorder: Mutex::new(Some(recorder)),
            index,
            dir,
            setup,
        }
    }

    /// Shows the flights folder in Explorer.
    pub fn open_folder(&self) -> io::Result<()> {
        Command::new("explorer").arg(&self.dir).spawn().map(drop)
    }

    /// Every stored flight in the folder, the most recently written first.
    pub fn summaries(&self) -> Vec<FlightSummary> {
        self.index.summaries(&self.station)
    }

    /// Deletes a stored flight's files for good, unless it is the one being
    /// recorded.
    fn delete(&self, id: &str) -> Result<(), String> {
        self.flight(id)?;
        if matches!(&*lock(&self.status), RecorderStatus::Recording { flight_id, .. } if flight_id == id)
        {
            return Err(format!("{id} is being recorded"));
        }
        ::debrief::delete_flight(&self.dir, id).map_err(|e| format!("{id}: {e}"))?;
        Ok(())
    }

    fn setup_status(&self) -> SetupStatus {
        self.setup.status(&lock(&self.status), &self.dir)
    }

    /// The stored file of a flight id. Ids are file stems from `list_flights`;
    /// anything else could name a file outside the folder.
    fn flight(&self, id: &str) -> Result<PathBuf, String> {
        let stem = !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c));
        if !stem {
            return Err(format!("no flight {id:?}"));
        }
        ::debrief::flight_path(&self.dir, id).ok_or_else(|| format!("no flight {id:?}"))
    }
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs analysis off the IPC thread: a long flight takes a while.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_flights(shared: State<'_, Arc<Shared>>) -> Result<Vec<FlightSummary>, String> {
    let shared = Arc::clone(&shared);
    blocking(move || Ok(shared.summaries())).await
}

#[tauri::command]
pub async fn debrief(id: String, shared: State<'_, Arc<Shared>>) -> Result<Debrief, String> {
    let shared = Arc::clone(&shared);
    blocking(move || {
        let analysis = shared.station.open(&shared.flight(&id)?);
        Ok(analysis.map_err(|e| format!("{id}: {e}"))?.debrief())
    })
    .await
}

#[tauri::command]
pub async fn delete_flight(id: String, shared: State<'_, Arc<Shared>>) -> Result<(), String> {
    let shared = Arc::clone(&shared);
    blocking(move || shared.delete(&id)).await
}

/// Places the aircraft at a landing challenge's start (docs/challenges.md):
/// the airport from the sim, the start on its extended centreline.
#[tauri::command]
pub async fn place_challenge(
    definition: String,
    shared: State<'_, Arc<Shared>>,
) -> Result<StartState, String> {
    let shared = Arc::clone(&shared);
    blocking(move || shared.place(&Challenge::parse(&definition)?)).await
}

impl Shared {
    fn place(&self, challenge: &Challenge) -> Result<StartState, String> {
        if challenge.start.mode != Mode::Teleport {
            return Err("this challenge starts wherever you fly it from".into());
        }
        let airport = answer(self.sim.airport(&challenge.airport))?;
        let s = challenge::start_state(challenge, &airport)
            .ok_or_else(|| format!("{} has no runway {}", challenge.airport, challenge.runway))?;
        let pos = InitPosition {
            latitude_deg: s.lat_deg,
            longitude_deg: s.lon_deg,
            altitude_ft: s.alt_ft,
            pitch_deg: 0.0,
            bank_deg: 0.0,
            heading_deg: s.heading_deg,
            on_ground: 0,
            airspeed_kt: s.ias_kt.round() as u32,
        };
        let start = Start {
            pos,
            gear_down: challenge.start.gear_down,
            flaps: Some(challenge.start.flaps),
        };
        answer(self.sim.place(start))?;
        Ok(s)
    }

    /// The pre-flight card (protocol B8) and the flight being recorded,
    /// with the sim time it has reached.
    fn preflight(&self, c: &Challenge) -> (Vec<Line>, Option<(String, Option<f64>)>) {
        let status = lock(&self.status).clone();
        let load = if c.start.mode == Mode::Teleport {
            "Load a flight in MSFS 2024 at any airport; Place moves you to the start."
        } else {
            "Load a flight in MSFS 2024."
        };
        let (sim, flight) = match status {
            RecorderStatus::Recording { flight_id, .. } => (
                Line::ok("sim", format!("MSFS connected, recording {flight_id}.")),
                Some(flight_id),
            ),
            RecorderStatus::Connected => (
                Line::fail("sim", "MSFS is connected but no flight is loaded.", load),
                None,
            ),
            RecorderStatus::WaitingForSim => (
                Line::fail(
                    "sim",
                    "MSFS isn't connected.",
                    "Start MSFS 2024, then load a flight.",
                ),
                None,
            ),
            RecorderStatus::Error { message } => (
                Line::fail(
                    "sim",
                    format!("The recorder stopped: {message}."),
                    "Restart FOQA-Sim; if it happens again, open Installation.",
                ),
                None,
            ),
        };
        let analysis = flight.as_ref().map(|id| {
            self.flight(id)
                .and_then(|p| self.station.open(&p).map_err(|e| e.to_string()))
        });
        let rule = &c.aircraft;
        let aircraft = match &analysis {
            _ if rule.classes.is_empty() && rule.types.is_empty() => {
                Line::ok("aircraft", "Any aircraft.")
            }
            None => Line::unknown("aircraft", "Checked once a flight is loaded."),
            Some(Err(_)) => Line::unknown(
                "aircraft",
                "Checked once the flight has data; give it a few seconds.",
            ),
            Some(Ok(a)) => {
                let check = challenge::aircraft_check(c, a);
                let needs = if rule.types.is_empty() {
                    let classes: Vec<_> = rule.classes.iter().map(|k| k.to_string()).collect();
                    format!("a {} aircraft", classes.join(" or "))
                } else {
                    format!("a {}", rule.types.join(" or "))
                };
                if check.ok {
                    Line::ok("aircraft", format!("{} is loaded.", check.detail))
                } else {
                    Line::fail(
                        "aircraft",
                        format!(
                            "A {} is loaded; this challenge needs {needs}.",
                            check.detail
                        ),
                        "Load one; flown as it is, the attempt won't count.",
                    )
                }
            }
        };
        let conditions = &c.conditions;
        let weather = if conditions.wind.is_none()
            && conditions.crosswind_kt.is_none()
            && conditions.tailwind_kt.is_none()
        {
            Line::ok("weather", "Any weather.")
        } else {
            Line::unknown(
                "weather",
                "Set by the challenge (pending in-flight verification).",
            )
        };
        let ranking = Line::ok("ranking", "Local challenge file: graded here only.");
        let begin = analysis.and_then(|a| a.ok()?.track.t.last().map(|t| t.as_secs_f64()));
        (
            vec![sim, aircraft, weather, ranking],
            flight.map(|id| (id, begin)),
        )
    }

    /// Grades an attempt from its flight: only a landing after the start.
    fn grade(&self, r: &Record, definition: &str) -> Result<Attempt, String> {
        let c = Challenge::parse(definition)?;
        let id = r.flight_id.as_deref().ok_or("the attempt has no flight")?;
        let mut a = self
            .station
            .open(&self.flight(id)?)
            .map_err(|e| format!("{id}: {e}"))?;
        if let Some(begin) = r.begin_s {
            a.touchdowns.retain(|td| td.t.as_secs_f64() >= begin);
        }
        Ok(challenge::evaluate(&c, &a))
    }

    /// Creates an attempt at `definition` and places it.
    fn start_attempt(&self, key: &str, definition: &str) -> Result<Record, String> {
        let c = Challenge::parse(definition)?;
        let (lines, flight) = self.preflight(&c);
        let (flight_id, begin_s) = flight.ok_or_else(|| lines[0].text.clone())?;
        let record = self.attempts.create(key, definition)?;
        let checks: serde_json::Map<_, _> = lines
            .iter()
            .map(|l| (l.name.to_owned(), l.state.into()))
            .collect();
        self.attempts.place(&record.id, &flight_id, begin_s, |c| {
            let start = match c.start.mode {
                Mode::Teleport => {
                    serde_json::to_value(self.place(c)?).map_err(|e| e.to_string())?
                }
                Mode::SelfFlown => "self".into(),
            };
            Ok(json!({ "start": start, "preflight": checks }))
        })
    }

    /// A landing in the recorded flight: attempts flying in it go to the
    /// rollout and are finished `ROLLOUT` later unless finished sooner.
    pub fn landed(self: &Arc<Self>, notice: &LandingNotice) {
        let td = &notice.touchdown;
        for id in self
            .attempts
            .landed(&notice.flight_id, td.t_s, td.sink_fpm, td.peak_g)
        {
            let shared = Arc::clone(self);
            thread::spawn(move || {
                thread::sleep(ROLLOUT);
                if shared
                    .attempts
                    .get(&id)
                    .is_ok_and(|r| r.state == AttemptState::Rollout)
                {
                    shared.finish(&id, "stopped");
                }
            });
        }
    }

    /// A flight closed: its attempts in the air are abandoned, those
    /// rolling out finished.
    pub fn flight_closed(self: &Arc<Self>, flight_id: &str) {
        for id in self.attempts.flight_closed(flight_id) {
            let shared = Arc::clone(self);
            thread::spawn(move || shared.finish(&id, "flight_closed"));
        }
    }

    fn finish(&self, id: &str, reason: &str) {
        if let Err(e) = self.attempts.finish(id, reason, &|r, d| self.grade(r, d)) {
            eprintln!("attempt {id}: {e}");
        }
    }

    /// At start-up: attempts a crash left open, resolved from their flights.
    pub fn recover(&self) {
        self.attempts.recover(&|r, d| self.grade(r, d));
    }
}

/// After a landing, how long the rollout runs before the attempt is graded
/// on its own (protocol B5).
const ROLLOUT: Duration = Duration::from_secs(120);

/// A line of the pre-flight card.
#[derive(Debug, Clone, Serialize)]
pub struct Line {
    name: &'static str,
    state: &'static str,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<String>,
}

impl Line {
    fn new(
        name: &'static str,
        state: &'static str,
        text: impl Into<String>,
        fix: Option<&str>,
    ) -> Self {
        let (text, fix) = (text.into(), fix.map(str::to_owned));
        Self {
            name,
            state,
            text,
            fix,
        }
    }
    fn ok(name: &'static str, text: impl Into<String>) -> Self {
        Self::new(name, "ok", text, None)
    }
    fn unknown(name: &'static str, text: impl Into<String>) -> Self {
        Self::new(name, "unknown", text, None)
    }
    fn fail(name: &'static str, text: impl Into<String>, fix: &str) -> Self {
        Self::new(name, "fail", text, Some(fix))
    }
}

#[tauri::command]
pub fn challenges_list(shared: State<'_, Arc<Shared>>) -> Vec<Entry> {
    shared.library.list()
}

/// Asks for a challenge file and keeps it; `None` when cancelled.
#[tauri::command]
pub async fn challenge_import(
    app: AppHandle,
    shared: State<'_, Arc<Shared>>,
) -> Result<Option<Entry>, String> {
    let shared = Arc::clone(&shared);
    blocking(move || {
        let picked = app
            .dialog()
            .file()
            .set_title("Open a challenge file")
            .add_filter("Challenge", &["json"])
            .blocking_pick_file();
        let Some(file) = picked else { return Ok(None) };
        let path = file.into_path().map_err(|e| e.to_string())?;
        shared.library.import_file(&path).map(Some)
    })
    .await
}

#[tauri::command]
pub fn challenge_remove(key: String, shared: State<'_, Arc<Shared>>) -> Result<(), String> {
    shared.library.remove(&key)
}

#[tauri::command]
pub fn challenge_briefing(key: String, shared: State<'_, Arc<Shared>>) -> Result<Briefing, String> {
    shared.library.briefing(&key)
}

#[tauri::command]
pub async fn challenge_preflight(
    key: String,
    shared: State<'_, Arc<Shared>>,
) -> Result<Vec<Line>, String> {
    let shared = Arc::clone(&shared);
    blocking(move || Ok(shared.preflight(&shared.library.get(&key)?.0).0)).await
}

/// Creates an attempt at a library challenge and places the aircraft.
#[tauri::command]
pub async fn attempt_start(key: String, shared: State<'_, Arc<Shared>>) -> Result<Record, String> {
    let shared = Arc::clone(&shared);
    blocking(move || shared.start_attempt(&key, &shared.library.get(&key)?.1)).await
}

/// Abandons the attempt if it is still open (`replaced`) and starts the
/// same definition again.
#[tauri::command]
pub async fn attempt_fly_again(
    id: String,
    shared: State<'_, Arc<Shared>>,
) -> Result<Record, String> {
    let shared = Arc::clone(&shared);
    blocking(move || {
        let record = shared.attempts.get(&id)?;
        if record.state.can_become(AttemptState::Abandoned) {
            shared.attempts.abandon(&id, "replaced")?;
        }
        shared.start_attempt(&record.key, &shared.attempts.definition(&id)?)
    })
    .await
}

/// Ends the attempt now and grades it from its flight.
#[tauri::command]
pub async fn attempt_finish(id: String, shared: State<'_, Arc<Shared>>) -> Result<Record, String> {
    let shared = Arc::clone(&shared);
    blocking(move || {
        shared
            .attempts
            .finish(&id, "finish", &|r, d| shared.grade(r, d))
    })
    .await
}

#[tauri::command]
pub fn attempt_abandon(id: String, shared: State<'_, Arc<Shared>>) -> Result<Record, String> {
    shared.attempts.abandon(&id, "cancelled")
}

#[tauri::command]
pub fn attempts_list(shared: State<'_, Arc<Shared>>) -> Vec<View> {
    shared.attempts.list()
}

/// The sim's answer to a request; an error without one in `ASK_TIMEOUT`,
/// after which the service refuses the request too.
fn answer<T>(answer: Receiver<Result<T, String>>) -> Result<T, String> {
    let wait = answer.recv_timeout(ASK_TIMEOUT);
    wait.map_err(|_| "no answer from the sim: is it running?".to_owned())?
}

#[tauri::command]
pub fn recorder_status(shared: State<'_, Arc<Shared>>) -> RecorderStatus {
    lock(&shared.status).clone()
}

#[tauri::command]
pub fn open_flights_folder(shared: State<'_, Arc<Shared>>) -> Result<(), String> {
    shared.open_folder().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn storage_usage(shared: State<'_, Arc<Shared>>) -> StorageUsage {
    ::debrief::storage_usage(&shared.dir)
}

/// Runs a fix off the IPC thread, then checks again.
async fn fix(
    shared: State<'_, Arc<Shared>>,
    f: impl FnOnce(&Setup) -> Result<(), String> + Send + 'static,
) -> Result<SetupStatus, String> {
    let shared = Arc::clone(&shared);
    blocking(move || {
        f(&shared.setup)?;
        Ok(shared.setup_status())
    })
    .await
}

#[tauri::command]
pub async fn setup_status(shared: State<'_, Arc<Shared>>) -> Result<SetupStatus, String> {
    fix(shared, |_| Ok(())).await
}

#[tauri::command]
pub async fn setup_install(
    package: String,
    shared: State<'_, Arc<Shared>>,
) -> Result<SetupStatus, String> {
    fix(shared, move |setup| setup.install(&package)).await
}

#[tauri::command]
pub async fn setup_remove(
    package: String,
    shared: State<'_, Arc<Shared>>,
) -> Result<SetupStatus, String> {
    fix(shared, move |setup| setup.remove(&package)).await
}

#[tauri::command]
pub async fn setup_use_community(
    path: Option<PathBuf>,
    shared: State<'_, Arc<Shared>>,
) -> Result<SetupStatus, String> {
    fix(shared, move |setup| setup.use_community(path.as_deref())).await
}

/// Asks for the Community folder; a cancelled dialog changes nothing.
#[tauri::command]
pub async fn setup_pick_community(
    app: AppHandle,
    shared: State<'_, Arc<Shared>>,
) -> Result<SetupStatus, String> {
    fix(shared, move |setup| {
        let picked = app
            .dialog()
            .file()
            .set_title("MSFS 2024 Community folder")
            .blocking_pick_folder();
        match picked {
            Some(dir) => setup.use_community(Some(&dir.into_path().map_err(|e| e.to_string())?)),
            None => Ok(()),
        }
    })
    .await
}
