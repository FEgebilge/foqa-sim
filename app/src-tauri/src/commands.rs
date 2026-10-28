//! The commands of `docs/debrief-api.md` and the state they share with the
//! recorder.

use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use ::debrief::{
    Analysis, Debrief, FlightSummary, Index, LandingNotice, RecorderStatus, Station, StorageUsage,
    TrendFilter, Trends,
};
use capture::{
    ASK_TIMEOUT, InitPosition, PreflightLine, SimRequests, SiteStrip, Start, Stopper, Views,
};
use challenge::{Attempt, Begin, Challenge, Mode, StartState};
use recorder::{AttemptMarker, AttemptPhase};
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::attempts::{Attempts, Cut, Record, State as AttemptState, Uncut, View};
use crate::diag;
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
    pub(crate) library: Library,
    pub(crate) attempts: Attempts,
    /// What an optional module adds to an attempt (the site client).
    pub(crate) hooks: Option<Arc<dyn AttemptHooks>>,
    /// The in-sim views' challenge part (B9).
    pub(crate) views: Views,
    /// A pass over the slices still to cut is running.
    reslicing: AtomicBool,
    /// What the last analysis of the live journal found, so the gate watch
    /// and the pre-flight card don't re-read a journal that did not grow.
    gate_memo: Memo<Option<f64>>,
    preflight_memo: Memo<(Line, Option<f64>)>,
}

/// The size and modification time of a file: it changed if they did.
type Stamp = (u64, SystemTime);

/// A result computed from one file, kept while the file is unchanged.
struct Memo<T>(Mutex<Option<(String, Stamp, T)>>);

impl<T: Clone> Memo<T> {
    fn new() -> Self {
        Self(Mutex::new(None))
    }

    /// The result for `key` and the file `path`, from `compute` unless it
    /// ran for this key on this very file.
    fn get(&self, key: &str, path: &Path, compute: impl FnOnce() -> T) -> T {
        let stamp = fs::metadata(path)
            .ok()
            .and_then(|m| Some((m.len(), m.modified().ok()?)));
        let mut slot = lock(&self.0);
        if let (Some(now), Some((k, then, v))) = (stamp, &*slot)
            && k == key
            && *then == now
        {
            return v.clone();
        }
        let value = compute();
        // The stamp is from before `compute`: a file that grew meanwhile misses.
        *slot = stamp.map(|s| (key.to_owned(), s, value.clone()));
        value
    }
}

/// Where an optional module joins the attempt lifecycle; the core knows
/// no site, token or upload (docs/challenge-protocol.md B1).
pub trait AttemptHooks: Send + Sync {
    /// After the attempt is created, before the aircraft is placed: the
    /// opaque reference its markers carry, if the module has one for it.
    fn before_place(&self, record: &Record) -> Option<String>;
    /// The pre-flight card's ranking line for library entry `key`, when it
    /// is not a local file: `(state, text, fix)`.
    fn ranking(&self, key: &str) -> Option<(&'static str, String, Option<String>)>;
    /// Where library entry `key` came from, when not a local file:
    /// `<name> (<ascii host>)`.
    fn source(&self, key: &str) -> Option<String>;
    /// The site part of attempt `id` for the in-sim strip, and whether
    /// nothing more will happen to it.
    fn site_strip(&self, id: &str) -> Option<(SiteStrip, bool)>;
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
        views: Views,
    ) -> Self {
        Self {
            views,
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
            hooks: None,
            reslicing: AtomicBool::new(false),
            gate_memo: Memo::new(),
            preflight_memo: Memo::new(),
        }
    }

    /// Shows the flights folder in Explorer.
    pub fn open_folder(&self) -> io::Result<()> {
        Command::new("explorer").arg(&self.dir).spawn().map(drop)
    }

    /// Every stored flight in the folder, the most recently written first,
    /// the one being recorded too.
    pub fn summaries(&self) -> Vec<FlightSummary> {
        self.index.summaries(&self.station)
    }

    /// `summaries`, each analysing thread first running `start`.
    pub fn summaries_with(&self, start: &(dyn Fn() + Sync)) -> Vec<FlightSummary> {
        self.index.summaries_with(&self.station, start)
    }

    /// The compact flights only: what trends and exports are about, and
    /// without analysing the journal that changes on every flush.
    pub fn saved(&self) -> Vec<FlightSummary> {
        self.index.saved(&self.station)
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
    pub(crate) fn flight(&self, id: &str) -> Result<PathBuf, String> {
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

/// What optional modules use.
#[cfg_attr(not(feature = "connect"), allow(dead_code))]
impl Shared {
    pub fn with_hooks(mut self, hooks: Arc<dyn AttemptHooks>) -> Self {
        self.hooks = Some(hooks);
        self
    }

    /// Imports a challenge's exact bytes into the library.
    pub fn import_challenge(&self, json: &str) -> Result<Entry, String> {
        self.library.import(json)
    }

    /// The kept challenge that importing `json` would replace.
    pub fn challenge_conflict(&self, json: &str) -> Option<Entry> {
        self.library.conflict(json)
    }
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs analysis off the IPC thread: a long flight takes a while.
pub(crate) async fn blocking<T: Send + 'static>(
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

/// The saved flights' trends (docs/trends.md) from the cached summaries.
#[tauri::command]
pub async fn trends(filter: TrendFilter, shared: State<'_, Arc<Shared>>) -> Result<Trends, String> {
    let shared = Arc::clone(&shared);
    blocking(move || Ok(::debrief::trends(&shared.saved(), &filter, &shared.station))).await
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
    fn preflight(&self, key: &str, c: &Challenge) -> (Vec<Line>, Option<(String, Option<f64>)>) {
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
        let (aircraft, begin) = match flight.as_ref().map(|id| self.flight(id)) {
            Some(Ok(path)) => self.preflight_memo.get(key, &path, || {
                let a = self.station.open(&path).map_err(|e| e.to_string());
                let begin = a.as_ref().ok().and_then(|a| a.track.t.last());
                (aircraft_line(c, Some(&a)), begin.map(|t| t.as_secs_f64()))
            }),
            Some(Err(e)) => (aircraft_line(c, Some(&Err(e))), None),
            None => (aircraft_line(c, None), None),
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
        let hooked = self.hooks.as_ref().and_then(|h| h.ranking(key));
        let ranking = match hooked {
            Some((state, text, fix)) => Line::new("ranking", state, text, fix.as_deref()),
            None => Line::ok("ranking", "Local challenge file: graded here only."),
        };
        (
            vec![sim, aircraft, weather, ranking],
            flight.map(|id| (id, begin)),
        )
    }

    /// Grades an attempt from its whole flight: only a landing after its
    /// own begin marker (or its start), not another attempt's.
    fn grade(&self, r: &Record, definition: &str) -> Result<Attempt, String> {
        let c = Challenge::parse(definition)?;
        let id = r.flight_id.as_deref().ok_or("the attempt has no flight")?;
        let a = self
            .station
            .open(&self.flight(id)?)
            .map_err(|e| format!("{id}: {e}"))?;
        let begin = Begin::Of {
            reference: r.marker_ref.as_deref(),
            begin_s: r.begin_s,
        };
        Ok(challenge::evaluate_after(&c, &a, begin))
    }

    /// Creates an attempt at `definition` and places it; as `practice` no
    /// site is asked for an attempt, so it is never uploaded.
    pub(crate) fn start_attempt(
        &self,
        key: &str,
        definition: &str,
        practice: bool,
    ) -> Result<Record, String> {
        let c = Challenge::parse(definition)?;
        let (lines, flight) = self.preflight(key, &c);
        let (flight_id, begin_s) = flight.ok_or_else(|| lines[0].text.clone())?;
        let record = self.attempts.create(key, definition)?;
        let marker_ref = self
            .hooks
            .as_ref()
            .filter(|_| !practice)
            .and_then(|hooks| hooks.before_place(&record))
            .filter(|r| recorder::is_reference(r));
        let checks: serde_json::Map<_, _> = lines
            .iter()
            .map(|l| (l.name.to_owned(), l.state.into()))
            .collect();
        let reference = marker_ref.as_deref();
        self.attempts
            .place(&record.id, &flight_id, begin_s, reference, |c| {
                let start = match c.start.mode {
                    Mode::Teleport => {
                        serde_json::to_value(self.place(c)?).map_err(|e| e.to_string())?
                    }
                    Mode::SelfFlown => "self".into(),
                };
                let begin = marker(&c.id, c.version, AttemptPhase::Begin, reference, None);
                self.sim.attempt_marker(begin);
                Ok(json!({ "start": start, "preflight": checks }))
            })
    }

    /// Writes the end marker of an attempt still open, into its flight if
    /// that is the one recording.
    fn end_marker(&self, r: &Record, reason: &str) {
        let open = matches!(
            r.state,
            AttemptState::Flying | AttemptState::Approach | AttemptState::Rollout
        );
        if !open || !r.flight_id.as_deref().is_some_and(|f| self.recording(f)) {
            return;
        }
        let reason = match reason {
            "finish" | "stopped" => "stopped",
            "cancelled" => "abandoned",
            other => other,
        };
        let reference = r.marker_ref.as_deref();
        let end = marker(
            &r.challenge,
            r.version,
            AttemptPhase::End,
            reference,
            Some(reason),
        );
        self.sim.attempt_marker(end);
    }

    fn recording(&self, flight: &str) -> bool {
        matches!(&*lock(&self.status), RecorderStatus::Recording { flight_id, .. } if flight_id == flight)
    }

    /// Abandons attempt `id` (`cancelled`, `replaced`), ending its marker.
    pub(crate) fn abandon(&self, id: &str, reason: &str) -> Result<Record, String> {
        self.end_marker(&self.attempts.get(id)?, reason);
        self.attempts.abandon(id, reason)
    }

    /// Abandons attempt `id` if it is still open (`replaced`) and starts
    /// the same definition again, so one attempt is open at a time.
    pub(crate) fn fly_again(&self, id: &str) -> Result<Record, String> {
        let record = self.attempts.get(id)?;
        if record.state.can_become(AttemptState::Abandoned) {
            self.abandon(id, "replaced")?;
        }
        self.start_attempt(&record.key, &self.attempts.definition(id)?, false)
    }

    /// The pre-flight card of library entry `key`, as the views show it.
    pub(crate) fn preflight_lines(&self, key: &str) -> Result<Vec<PreflightLine>, String> {
        let (c, _) = self.library.get(key)?;
        let lines = self.preflight(key, &c).0.into_iter();
        Ok(lines
            .map(|l| PreflightLine {
                name: l.name.into(),
                state: l.state.into(),
                text: l.text,
                fix: l.fix,
            })
            .collect())
    }

    /// An attempt placed in the flight being recorded moves to the
    /// approach once the recording shows its gate passed (B5).
    pub(crate) fn watch_gate(&self, r: &Record) {
        let recording = r.flight_id.as_deref().is_some_and(|f| self.recording(f));
        if r.state != AttemptState::Flying || !recording {
            return;
        }
        let Some(path) = r.flight_id.as_deref().and_then(|f| self.flight(f).ok()) else {
            return;
        };
        let gate_s = self.gate_memo.get(&r.id, &path, || {
            let graded = self.attempts.definition(&r.id);
            graded.and_then(|d| self.grade(r, &d)).ok()?.gate_s
        });
        if let Some(gate_s) = gate_s
            && let Err(e) = self.attempts.gate(&r.id, gate_s)
        {
            eprintln!("attempt {}: {e}", r.id);
        }
    }

    /// The slice of an attempt from its flight (A5). While the flight
    /// records, waits up to `SLICE_WAIT` for the data past the end marker
    /// (and for a begin marker not on disk yet); a failed read is tried
    /// again after `READ_BACKOFF`, doubling, `READ_TRIES` times.
    fn cut(&self, r: &Record) -> Result<Cut, Uncut> {
        let never = |code, e: &str| Uncut::Never(code, e.to_owned());
        let reference = r.marker_ref.as_deref();
        let reference = reference.ok_or_else(|| never("no_marker", "the attempt has no marker"))?;
        let id = r.flight_id.as_deref();
        let id = id.ok_or_else(|| never("no_flight", "the attempt has no flight"))?;
        let until = Instant::now() + SLICE_WAIT;
        loop {
            let path = self.flight(id).map_err(|e| never("no_flight", &e))?;
            let read = || {
                let file = File::open(&path)?;
                recorder::slice(BufReader::new(file), recorder::Cut::Attempt(reference))
            };
            let read = retried(read, READ_BACKOFF, READ_TRIES);
            let read = read.map_err(|e| Uncut::Later(format!("{id}: {e}")))?;
            let waiting = self.recording(id) && Instant::now() < until;
            match read {
                None if !waiting => {
                    let e = format!("{id} has no begin marker for this attempt");
                    return Err(never("no_marker", &e));
                }
                Some(slice) if slice.complete || !waiting => {
                    let bytes = slice.to_bytes().map_err(|e| Uncut::Later(e.to_string()))?;
                    return Ok(Cut {
                        bytes,
                        from_s: slice.from.as_secs_f64(),
                        to_s: slice.to.as_secs_f64(),
                    });
                }
                _ => thread::sleep(Duration::from_secs(1)),
            }
        }
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
                    let _ = shared.finish(&id, "stopped");
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

    /// Ends attempt `id` (its end marker first) and grades it; its slice is
    /// then cut on a thread of its own.
    pub(crate) fn finish(self: &Arc<Self>, id: &str, reason: &str) -> Result<Record, String> {
        if let Ok(r) = self.attempts.get(id) {
            self.end_marker(&r, reason);
        }
        let record = self
            .attempts
            .finish(id, reason, &|r, d| self.grade(r, d))
            .inspect_err(|e| eprintln!("attempt {id}: {e}"))?;
        if record.state == AttemptState::GradedLocal {
            let (shared, id) = (Arc::clone(self), id.to_owned());
            thread::spawn(move || shared.slice(&id));
        }
        Ok(record)
    }

    fn slice(&self, id: &str) {
        if let Err(e) = self.attempts.slice(id, |r| self.cut(r)) {
            eprintln!("attempt {id}: not sliced: {e}");
        }
    }

    /// Cuts again, on a thread of its own, the slices a failed cut or a
    /// crash left: at start-up, every `RESLICE` and when the Challenges
    /// screen lists the attempts. One pass at a time.
    fn reslice(self: &Arc<Self>) {
        if self.reslicing.swap(true, Ordering::SeqCst) {
            return;
        }
        let shared = Arc::clone(self);
        thread::spawn(move || {
            for id in shared.attempts.unsliced() {
                shared.slice(&id);
            }
            shared.reslicing.store(false, Ordering::SeqCst);
        });
    }

    /// At start-up: attempts a crash left open, resolved from their
    /// flights; then the graded ones still to slice, every `RESLICE`.
    pub fn recover(self: &Arc<Self>) {
        self.attempts.recover(&|r, d| self.grade(r, d));
        let shared = Arc::clone(self);
        thread::spawn(move || {
            loop {
                shared.reslice();
                thread::sleep(RESLICE);
            }
        });
    }
}

/// After a landing, how long the rollout runs before the attempt is graded
/// on its own (protocol B5).
const ROLLOUT: Duration = Duration::from_secs(120);
/// How long a slice waits for the recording to pass its end marker.
const SLICE_WAIT: Duration = Duration::from_secs(15);
/// A failed read of the flight is tried again this much later, doubling.
const READ_BACKOFF: Duration = Duration::from_secs(1);
const READ_TRIES: u32 = 3;
/// How often the slices still to cut are tried again.
const RESLICE: Duration = Duration::from_secs(300);

/// `read`, tried again while it fails up to `tries` more times, after
/// `pause`, doubling: a flight file briefly locked is read all the same.
fn retried<T>(
    mut read: impl FnMut() -> io::Result<T>,
    pause: Duration,
    tries: u32,
) -> io::Result<T> {
    let mut pause = pause;
    for _ in 0..tries {
        if let Ok(v) = read() {
            return Ok(v);
        }
        thread::sleep(pause);
        pause *= 2;
    }
    read()
}

fn marker(
    challenge: &str,
    version: u32,
    phase: AttemptPhase,
    reference: Option<&str>,
    reason: Option<&str>,
) -> AttemptMarker {
    AttemptMarker {
        phase,
        challenge: challenge.to_owned(),
        version,
        reference: reference.map(Into::into),
        reason: reason.map(Into::into),
    }
}

/// The pre-flight card's aircraft line for the flight's `analysis`, if
/// there is a flight.
fn aircraft_line(c: &Challenge, analysis: Option<&Result<Analysis<'_>, String>>) -> Line {
    let rule = &c.aircraft;
    match analysis {
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
    }
}

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
pub async fn challenges_list(shared: State<'_, Arc<Shared>>) -> Result<Vec<Entry>, String> {
    let shared = Arc::clone(&shared);
    blocking(move || Ok(shared.library.list())).await
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
pub async fn challenge_briefing(
    key: String,
    shared: State<'_, Arc<Shared>>,
) -> Result<Briefing, String> {
    let shared = Arc::clone(&shared);
    blocking(move || shared.library.briefing(&key)).await
}

#[tauri::command]
pub async fn challenge_preflight(
    key: String,
    shared: State<'_, Arc<Shared>>,
) -> Result<Vec<Line>, String> {
    let shared = Arc::clone(&shared);
    blocking(move || Ok(shared.preflight(&key, &shared.library.get(&key)?.0).0)).await
}

/// Creates an attempt at a library challenge and places the aircraft.
#[tauri::command]
pub async fn attempt_start(key: String, shared: State<'_, Arc<Shared>>) -> Result<Record, String> {
    let shared = Arc::clone(&shared);
    blocking(move || shared.start_attempt(&key, &shared.library.get(&key)?.1, false)).await
}

/// Abandons the attempt if it is still open (`replaced`) and starts the
/// same definition again.
#[tauri::command]
pub async fn attempt_fly_again(
    id: String,
    shared: State<'_, Arc<Shared>>,
) -> Result<Record, String> {
    let shared = Arc::clone(&shared);
    blocking(move || shared.fly_again(&id)).await
}

/// Ends the attempt now and grades it from its flight.
#[tauri::command]
pub async fn attempt_finish(id: String, shared: State<'_, Arc<Shared>>) -> Result<Record, String> {
    let shared = Arc::clone(&shared);
    blocking(move || shared.finish(&id, "finish")).await
}

#[tauri::command]
pub fn attempt_abandon(id: String, shared: State<'_, Arc<Shared>>) -> Result<Record, String> {
    shared.abandon(&id, "cancelled")
}

/// Every attempt; the Challenges screen listing them also cuts again the
/// slices still to cut.
#[tauri::command]
pub async fn attempts_list(shared: State<'_, Arc<Shared>>) -> Result<Vec<View>, String> {
    let shared = Arc::clone(&shared);
    blocking(move || {
        shared.reslice();
        Ok(shared.attempts.list())
    })
    .await
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

/// Opens the folder holding the log file (docs/debrief-api.md).
#[tauri::command]
pub fn open_log_folder() -> Result<(), String> {
    let dir = diag::path()
        .and_then(|p| p.parent())
        .ok_or("There is no log file.")?;
    Command::new("explorer")
        .arg(dir)
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn storage_usage(shared: State<'_, Arc<Shared>>) -> Result<StorageUsage, String> {
    let shared = Arc::clone(&shared);
    blocking(move || Ok(::debrief::storage_usage(&shared.dir))).await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_memo_recomputes_only_for_another_key_or_a_changed_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("flight.fqj");
        fs::write(&path, "a").unwrap();
        let (memo, mut runs) = (Memo::new(), 0);
        let mut get = |key: &str| {
            memo.get(key, &path, || {
                runs += 1;
                runs
            })
        };
        assert_eq!((get("x"), get("x")), (1, 1));
        assert_eq!(get("y"), 2);
        fs::write(&path, "ab").unwrap();
        assert_eq!(get("y"), 3);
    }

    #[test]
    fn a_failed_read_is_tried_again_with_backoff() {
        let pause = Duration::from_millis(5);
        let mut calls = 0;
        let flaky = retried(
            || {
                calls += 1;
                if calls < 3 {
                    Err(io::Error::other("locked"))
                } else {
                    Ok(calls)
                }
            },
            pause,
            READ_TRIES,
        );
        assert_eq!(flaky.unwrap(), 3);
        let mut calls = 0;
        let started = Instant::now();
        let locked: io::Result<()> = retried(
            || {
                calls += 1;
                Err(io::Error::other("locked"))
            },
            pause,
            READ_TRIES,
        );
        assert!(locked.is_err());
        assert_eq!(calls, READ_TRIES + 1);
        assert!(started.elapsed() >= pause * 7, "5, 10 and 20 ms apart");
    }
}
