//! The flight list's cache: each stored flight's `FlightSummary`, valid
//! while the file's size and modification time and the `Station` that made
//! it are unchanged. Compact flights' summaries persist in `index.json` in
//! the flights folder (`docs/format.md`), so a start lists hundreds of
//! flights without analysing them; a journal (a flight being recorded, or
//! cut off) is kept in memory only. The file is a cache: a missing,
//! unreadable or stale one is rebuilt.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::UNIX_EPOCH;

use recorder::COMPACT_EXT;
use serde::{Deserialize, Serialize};

use crate::{FlightSummary, Station, file_name, flight_files, has_ext};

/// The index's file name in the flights folder.
pub const INDEX_FILE: &str = "index.json";
/// At most this many flights are analysed at once: a long one takes memory.
const WORKERS: usize = 4;

#[derive(Default, Serialize, Deserialize)]
struct Cache {
    /// `Station::version` of every entry.
    station: String,
    /// By file name.
    flights: BTreeMap<String, Entry>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    len: u64,
    modified_ns: u64,
    /// `None`: nothing to analyse in it. A file that could not be read
    /// (locked, cloud-only, gone) gets no entry, so it is tried again.
    summary: Option<FlightSummary>,
}

/// A stored flight file as it is now.
struct Stored {
    path: PathBuf,
    len: u64,
    modified_ns: u64,
}

impl Stored {
    fn of(path: PathBuf) -> Option<Self> {
        let meta = fs::metadata(&path).ok()?;
        let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        Some(Self {
            path,
            len: meta.len(),
            modified_ns: u64::try_from(modified.as_nanos()).ok()?,
        })
    }

    fn matches(&self, e: &Entry) -> bool {
        (self.len, self.modified_ns) == (e.len, e.modified_ns)
    }
}

/// The summaries of a flights folder; share one per folder.
pub struct Index {
    dir: PathBuf,
    cache: Mutex<Cache>,
}

impl Index {
    /// The index of the flights folder `dir`, as last saved.
    pub fn open(dir: &Path) -> Self {
        let saved = fs::read(dir.join(INDEX_FILE)).ok();
        Self {
            dir: dir.to_owned(),
            cache: Mutex::new(
                saved
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_default(),
            ),
        }
    }

    /// Every stored flight's summary, the most recently written first.
    /// Analyses only new and changed flights and saves the index when it
    /// changed; a concurrent call waits, then finds them analysed.
    pub fn summaries(&self, station: &Station) -> Vec<FlightSummary> {
        self.summaries_with(station, &|| {})
    }

    /// As `summaries`, each analysing thread first running `start`: a
    /// warm-up lowers the priority of its threads there.
    pub fn summaries_with(
        &self,
        station: &Station,
        start: &(dyn Fn() + Sync),
    ) -> Vec<FlightSummary> {
        self.list(station, |_| true, start)
    }

    /// The compact flights' summaries only, as `summaries`: no journal, so
    /// no flight being recorded, is analysed.
    pub fn saved(&self, station: &Station) -> Vec<FlightSummary> {
        self.list(station, |p| has_ext(p, COMPACT_EXT), &|| {})
    }

    /// The summaries of the stored flights `keep` takes.
    fn list(
        &self,
        station: &Station,
        keep: impl Fn(&Path) -> bool,
        start: &(dyn Fn() + Sync),
    ) -> Vec<FlightSummary> {
        let mut files: Vec<_> = flight_files(&self.dir)
            .into_iter()
            .filter(|p| keep(p))
            .filter_map(Stored::of)
            .collect();
        files.sort_by_key(|f| Reverse(f.modified_ns));
        let mut cache = self.lock(station);
        let cached = |f: &&Stored| {
            let entry = cache.flights.get(&file_name(&f.path));
            entry.is_some_and(|e| f.matches(e))
        };
        let stale: Vec<&Stored> = files.iter().filter(|f| !cached(f)).collect();
        let fresh = analyse(station, &stale, start);
        let mut dirty = false;
        for (f, summary) in stale.into_iter().zip(fresh) {
            let Some(summary) = summary else { continue };
            dirty |= has_ext(&f.path, COMPACT_EXT);
            let entry = Entry {
                len: f.len,
                modified_ns: f.modified_ns,
                summary,
            };
            cache.flights.insert(file_name(&f.path), entry);
        }
        let names: HashSet<_> = files.iter().map(|f| file_name(&f.path)).collect();
        let before = cache.flights.len();
        cache
            .flights
            .retain(|name, _| names.contains(name) || !keep(Path::new(name)));
        dirty |= cache.flights.len() != before;
        if dirty && let Err(e) = self.save(&cache) {
            eprintln!("{}: {e}", self.dir.join(INDEX_FILE).display());
        }
        let summary = |f: &Stored| cache.flights.get(&file_name(&f.path))?.summary.clone();
        files.iter().filter_map(summary).collect()
    }

    /// The cache, emptied if another station made it.
    fn lock(&self, station: &Station) -> MutexGuard<'_, Cache> {
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.station != station.version {
            *cache = Cache {
                station: station.version.clone(),
                flights: BTreeMap::new(),
            };
        }
        cache
    }

    /// Writes the compact flights' entries, whole or not at all.
    fn save(&self, cache: &Cache) -> io::Result<()> {
        let flights = cache
            .flights
            .iter()
            .filter(|(name, _)| has_ext(Path::new(name), COMPACT_EXT));
        let saved = Cache {
            station: cache.station.clone(),
            flights: flights.map(|(n, e)| (n.clone(), e.clone())).collect(),
        };
        let part = self.dir.join(format!("{INDEX_FILE}.part"));
        fs::write(&part, serde_json::to_vec(&saved)?)?;
        fs::rename(part, self.dir.join(INDEX_FILE))
    }
}

/// Each flight's `Entry::summary`, up to `WORKERS` at a time, each worker
/// first running `start`; `None` for one that could not be read. The workers
/// take the next flight from one counter, so a slow flight holds up only
/// its own worker.
fn analyse(
    station: &Station,
    files: &[&Stored],
    start: &(dyn Fn() + Sync),
) -> Vec<Option<Option<FlightSummary>>> {
    let cores = thread::available_parallelism().map_or(1, NonZero::get);
    let next = AtomicUsize::new(0);
    let work = || {
        start();
        let mut done = Vec::new();
        loop {
            let i = next.fetch_add(1, Ordering::Relaxed);
            let Some(f) = files.get(i) else { break };
            done.push((i, summary(station, &f.path)));
        }
        done
    };
    let mut done: Vec<(usize, _)> = thread::scope(|s| {
        let workers: Vec<_> = (0..cores.min(WORKERS).min(files.len()))
            .map(|_| s.spawn(work))
            .collect();
        let joined = workers.into_iter().map(|h| h.join());
        joined
            .flat_map(|r| r.unwrap_or_else(|p| std::panic::resume_unwind(p)))
            .collect()
    });
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, s)| s).collect()
}

/// The flight's `Entry::summary`; `None` on an OS error (locked,
/// cloud-only, gone), which may pass.
fn summary(station: &Station, path: &Path) -> Option<Option<FlightSummary>> {
    match station.open(path) {
        Ok(a) => Some(Some(a.summary())),
        Err(e)
            if e.downcast_ref::<io::Error>()
                .and_then(io::Error::raw_os_error)
                .is_some() =>
        {
            eprintln!("{}: {e}", path.display());
            None
        }
        Err(_) => Some(None),
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::time::SystemTime;

    use serde_json::Value;
    use tempfile::TempDir;

    use super::*;
    use crate::{core_rules, record, tests::C172};

    /// A folder with the C172 pattern as compact flight `flight-1` and as
    /// journal `flight-2`.
    fn folder() -> TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let journal = record(Path::new(C172)).unwrap();
        for name in ["flight-1.fqj", "flight-2.fqj"] {
            fs::write(tmp.path().join(name), &journal).unwrap();
        }
        recorder::compact_file(&tmp.path().join("flight-1.fqj")).unwrap();
        tmp
    }

    fn saved(dir: &Path) -> Value {
        serde_json::from_slice(&fs::read(dir.join(INDEX_FILE)).unwrap()).unwrap()
    }

    /// Sets `flight-1`'s saved landings, to tell a cached summary from a
    /// fresh one.
    fn tamper(dir: &Path) {
        let mut index = saved(dir);
        index["flights"]["flight-1.fqz"]["summary"]["landings"] = 99.into();
        fs::write(dir.join(INDEX_FILE), index.to_string()).unwrap();
    }

    fn landings(dir: &Path, station: &Station) -> Vec<usize> {
        let summaries = Index::open(dir).summaries(station);
        summaries.iter().map(|s| s.landings).collect()
    }

    #[test]
    fn a_summary_is_cached_until_its_flight_or_the_station_changes() {
        let (tmp, station) = (folder(), Station::core().unwrap());
        let dir = tmp.path();
        assert_eq!(landings(dir, &station), [1, 1]);
        // Only the compact flight is saved: a journal may be recording.
        let index = saved(dir);
        let flights = index["flights"].as_object().unwrap();
        assert_eq!(flights.keys().collect::<Vec<_>>(), ["flight-1.fqz"]);
        let saved = Index::open(dir).saved(&station);
        assert_eq!(
            saved.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["flight-1"]
        );

        tamper(dir);
        let mut cached = landings(dir, &station);
        cached.sort();
        assert_eq!(cached, [1, 99]);

        let fewer_rules = Station::new(core_rules().unwrap()[1..].to_vec()).unwrap();
        assert_eq!(landings(dir, &fewer_rules), [1, 1]);

        assert_eq!(landings(dir, &station), [1, 1]);
        tamper(dir);
        let touched = SystemTime::now() + std::time::Duration::from_secs(60);
        let flight = File::options().write(true).open(dir.join("flight-1.fqz"));
        flight.unwrap().set_modified(touched).unwrap();
        assert_eq!(landings(dir, &station), [1, 1]);
    }

    #[test]
    fn every_flight_is_analysed_once_in_order_by_workers_that_each_start() {
        let (tmp, station) = (folder(), Station::core().unwrap());
        let dir = tmp.path();
        for n in 3..=9 {
            fs::copy(
                dir.join("flight-2.fqj"),
                dir.join(format!("flight-{n}.fqj")),
            )
            .unwrap();
        }
        let started = AtomicUsize::new(0);
        let summaries = Index::open(dir).summaries_with(&station, &|| {
            started.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(summaries.len(), 9);
        let listed = Index::open(dir).summaries(&station);
        let ids = |v: &[FlightSummary]| v.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
        assert_eq!(
            ids(&summaries),
            ids(&listed),
            "newest first, whoever finished first"
        );
        assert!((1..=WORKERS).contains(&started.load(Ordering::Relaxed)));
    }

    #[test]
    fn a_corrupt_or_missing_index_is_rebuilt() {
        let (tmp, station) = (folder(), Station::core().unwrap());
        let dir = tmp.path();
        fs::write(dir.join(INDEX_FILE), "{\"station\": 1, \"fli").unwrap();
        assert_eq!(landings(dir, &station), [1, 1]);
        assert!(saved(dir)["flights"]["flight-1.fqz"]["summary"].is_object());

        fs::remove_file(dir.join(INDEX_FILE)).unwrap();
        assert_eq!(landings(dir, &station), [1, 1]);
        assert!(saved(dir)["flights"]["flight-1.fqz"]["summary"].is_object());

        // A deleted flight leaves the index.
        fs::remove_file(dir.join("flight-1.fqz")).unwrap();
        assert_eq!(landings(dir, &station), [1]);
        assert_eq!(saved(dir)["flights"], serde_json::json!({}));
    }

    #[test]
    fn an_unreadable_flight_is_not_cached_a_bad_one_is() {
        let (tmp, station) = (folder(), Station::core().unwrap());
        let dir = tmp.path();
        fs::write(dir.join("bad.fqz"), "not a flight").unwrap();
        // Opening a folder is an OS error, as a locked file's is.
        fs::create_dir(dir.join("locked.fqz")).unwrap();
        assert_eq!(landings(dir, &station), [1, 1]);
        let index = saved(dir);
        assert!(index["flights"]["bad.fqz"]["summary"].is_null());
        assert!(index["flights"].get("locked.fqz").is_none());
    }
}
