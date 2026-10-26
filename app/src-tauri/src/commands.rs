//! The commands of `docs/debrief-api.md` and the state they share with the
//! recorder.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::SystemTime;

use ::debrief::{Debrief, FlightSummary, RecorderStatus, Station, StorageUsage};
use capture::Stopper;
use tauri::State;

/// What the commands, the tray and the recorder thread share.
pub struct Shared {
    pub station: Arc<Station>,
    /// Where the recorder writes the flights.
    pub dir: PathBuf,
    /// The latest status the recorder reported.
    pub status: Mutex<RecorderStatus>,
    pub stopper: Stopper,
    pub recorder: Mutex<Option<JoinHandle<()>>>,
    /// Analysed summaries by journal, valid while its modification time is
    /// unchanged, so listing re-analyses only new or growing flights.
    /// `None`: nothing to analyse in it (yet).
    summaries: Mutex<HashMap<PathBuf, (SystemTime, Option<FlightSummary>)>>,
}

impl Shared {
    pub fn new(
        station: Arc<Station>,
        dir: PathBuf,
        stopper: Stopper,
        recorder: JoinHandle<()>,
    ) -> Self {
        Self {
            station,
            dir,
            status: Mutex::new(RecorderStatus::WaitingForSim),
            stopper,
            recorder: Mutex::new(Some(recorder)),
            summaries: Mutex::default(),
        }
    }

    /// Shows the flights folder in Explorer.
    pub fn open_folder(&self) -> io::Result<()> {
        Command::new("explorer").arg(&self.dir).spawn().map(drop)
    }

    /// Every stored flight in the folder, the most recently written first.
    fn summaries(&self) -> Vec<FlightSummary> {
        let mut journals: Vec<(SystemTime, PathBuf)> = ::debrief::flight_files(&self.dir)
            .into_iter()
            .filter_map(|p| Some((fs::metadata(&p).ok()?.modified().ok()?, p)))
            .collect();
        journals.sort_by_key(|j| Reverse(j.0));
        let mut cache = lock(&self.summaries);
        journals
            .into_iter()
            .filter_map(|(modified, path)| match cache.get(&path) {
                Some((at, summary)) if *at == modified => summary.clone(),
                _ => {
                    let summary = self.station.open(&path).ok().map(|a| a.summary());
                    cache.insert(path, (modified, summary.clone()));
                    summary
                }
            })
            .collect()
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
