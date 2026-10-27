//! The commands of `docs/debrief-api.md` and the state they share with the
//! recorder.

use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use ::debrief::{Debrief, FlightSummary, Index, RecorderStatus, Station, StorageUsage};
use capture::{ASK_TIMEOUT, InitPosition, SimRequests, Start, Stopper};
use challenge::{Challenge, Mode, StartState};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

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
}

impl Shared {
    pub fn new(
        station: Arc<Station>,
        dir: PathBuf,
        stopper: Stopper,
        index: Arc<Index>,
        recorder: JoinHandle<()>,
        setup: Setup,
        sim: SimRequests,
    ) -> Self {
        Self {
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
    blocking(move || {
        let challenge = Challenge::parse(&definition)?;
        if challenge.start.mode != Mode::Teleport {
            return Err("this challenge starts wherever you fly it from".into());
        }
        let airport = answer(shared.sim.airport(&challenge.airport))?;
        let s = challenge::start_state(&challenge, &airport)
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
        answer(shared.sim.place(start))?;
        Ok(s)
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
