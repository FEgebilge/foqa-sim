//! Clean starts: the flight the sim saves right after a challenge's first
//! placement, one per challenge, so that Fly again reloads a pristine
//! aircraft (docs/challenges.md, "Fly again"). The sim reports neither
//! damage nor a crash, so every Fly again reloads it. A clean start is kept
//! with the ICAO type it was saved with and only reloaded into the same
//! type: a reload brings back the saved aircraft, not the loaded one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::library;

/// The sim's own extension for a saved flight.
const FLT: &str = "FLT";
/// No AI air traffic in a reloaded clean start, as the sim's own events and
/// mission templates turn it off (`asobo-liveevent` LiveEvent.FLT): a
/// challenge is flown alone, without another aircraft's wake on final.
/// Not their `[TrafficManager] disable=True` too: with it MSFS 2024 never
/// ends its move to the runway after a load (no `RTC_END`; bench
/// 2026-10-31), so the start was placed before the sim moved the aircraft
/// back onto the runway.
const NO_TRAFFIC: &[(&str, &str)] = &[("[AirTraffic]", "Status=0")];

/// `<key>.json` beside the flight: what it was saved with.
#[derive(Debug, Serialize, Deserialize)]
struct Saved {
    icao: String,
}

/// The clean starts folder.
pub struct CleanStarts {
    dir: PathBuf,
}

impl CleanStarts {
    pub fn new(dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// Where the sim saves library entry `key`'s clean start: no extension,
    /// the sim adds `.FLT`. A new one replaces it.
    pub fn path(&self, key: &str) -> Result<PathBuf, String> {
        library::checked_key(key)?;
        Ok(self.dir.join(key))
    }

    /// Forgets the clean start at `path` before another is saved there: a
    /// save the sim then rejects leaves none, not one of another type.
    pub fn forget(&self, path: &Path) {
        for file in [path.with_extension(FLT), path.with_extension("json")] {
            match fs::remove_file(&file) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => {
                    eprintln!("{}: could not delete: {e}", file.display())
                }
                _ => {}
            }
        }
    }

    /// Waits, up to `within`, until the sim has written the flight saved at
    /// `path`: it answers the save before the file is on disk (2026-10-30),
    /// so the file is taken once it exists and its size holds still.
    pub fn written(&self, path: &Path, within: Duration) -> Result<(), String> {
        let flt = path.with_extension(FLT);
        let until = Instant::now() + within;
        let mut last = None;
        while Instant::now() < until {
            let size = fs::metadata(&flt).ok().map(|m| m.len());
            if size.is_some_and(|s| s > 0) && size == last {
                return Ok(());
            }
            last = size;
            thread::sleep(Duration::from_millis(200));
        }
        Err("the sim did not write the saved flight".into())
    }

    /// The flight at `path` was saved for an `icao` aircraft: its traffic
    /// is turned off and its type kept beside it.
    pub fn saved(&self, path: &Path, icao: &str) -> Result<(), String> {
        let flt = path.with_extension(FLT);
        match fs::read(&flt) {
            Ok(bytes) => fs::write(&flt, without_traffic(&bytes)).map_err(|e| e.to_string())?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        let saved = Saved { icao: icao.into() };
        let json = serde_json::to_vec(&saved).map_err(|e| e.to_string())?;
        fs::write(path.with_extension("json"), json).map_err(|e| e.to_string())
    }

    /// The clean start of `key` to reload into the `icao` aircraft loaded
    /// now, or in plain words why there is none.
    pub fn find(&self, key: &str, icao: Option<&str>) -> Result<PathBuf, String> {
        let path = self.path(key)?;
        let saved = fs::read(path.with_extension("json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Saved>(&b).ok());
        let Some(saved) = saved.filter(|_| path.with_extension(FLT).is_file()) else {
            return Err("none saved yet".into());
        };
        match icao {
            Some(icao) if icao == saved.icao => Ok(path),
            Some(icao) => Err(format!(
                "the one saved is for a {}, a {icao} is loaded",
                saved.icao
            )),
            None => Err("the loaded aircraft's type is not known yet".into()),
        }
    }
}

/// `flt` with `NO_TRAFFIC`'s sections appended, those it lacks; bytes, not
/// text: the sim writes Latin-1 names.
fn without_traffic(flt: &[u8]) -> Vec<u8> {
    let has = |section: &str| flt.windows(section.len()).any(|w| w == section.as_bytes());
    let mut out = flt.to_vec();
    for (section, line) in NO_TRAFFIC.iter().filter(|(s, _)| !has(s)) {
        out.extend_from_slice(format!("\n{section}\n{line}\n").as_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_save_counts_once_the_file_is_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let starts = CleanStarts::new(tmp.path().to_path_buf()).unwrap();
        let path = starts.path("ltbj-34l-v1").unwrap();
        assert!(starts.written(&path, Duration::from_millis(300)).is_err());
        fs::write(path.with_extension(FLT), "[Main]").unwrap();
        assert!(starts.written(&path, Duration::from_secs(2)).is_ok());
    }

    #[test]
    fn a_clean_start_is_saved_without_traffic_once() {
        let once = without_traffic(b"[Main]\nTitle=x\xe9\n");
        let text = String::from_utf8_lossy(&once);
        assert!(text.ends_with("\n[AirTraffic]\nStatus=0\n"));
        assert!(!text.contains("[TrafficManager]"));
        assert_eq!(without_traffic(&once), once);
    }

    #[test]
    fn a_clean_start_is_reloaded_only_into_its_own_type() {
        let tmp = tempfile::tempdir().unwrap();
        let starts = CleanStarts::new(tmp.path().join("challenge-snapshots")).unwrap();
        let key = "ltbj-34l-v1";
        assert_eq!(starts.find(key, Some("C172")), Err("none saved yet".into()));
        let path = starts.path(key).unwrap();
        starts.saved(&path, "C172").unwrap();
        // The meta without the sim's file: the save never happened.
        assert!(starts.find(key, Some("C172")).is_err());
        fs::write(path.with_extension(FLT), "[Main]").unwrap();
        assert_eq!(starts.find(key, Some("C172")), Ok(path.clone()));
        let other = starts.find(key, Some("B738")).unwrap_err();
        assert_eq!(other, "the one saved is for a C172, a B738 is loaded");
        assert!(starts.find(key, None).is_err());
        // Forgotten before another save, whatever its type.
        starts.forget(&path);
        assert_eq!(starts.find(key, Some("C172")), Err("none saved yet".into()));
        assert!(starts.path("../x").is_err());
    }
}
