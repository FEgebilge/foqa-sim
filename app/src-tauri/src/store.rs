//! What the app keeps beside the recorder's flight files (docs/debrief-api.md):
//! each flight's title, notes and tags in `<id>.meta.json`, and the trash,
//! `.trash/<id>/` in the flights folder, where a deleted flight's files wait
//! to be restored until they are purged.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ::debrief::{FlightSummary, flight_file_names};
use serde::{Deserialize, Serialize};

pub const TRASH_DIR: &str = ".trash";
/// A deleted flight is purged this long after it was deleted.
pub const TRASH_KEEP: Duration = Duration::from_secs(30 * 24 * 3600);
/// In a trashed flight's folder: when it was deleted, in Unix seconds.
const DELETED_AT: &str = "deleted-at";
const META_EXT: &str = ".meta.json";
const TITLE_MAX: usize = 120;
const NOTES_MAX: usize = 4000;
const TAG_MAX: usize = 32;
const TAGS_MAX: usize = 20;

/// The pilot's words about a flight; empty when none were given.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FlightMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl FlightMeta {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Trimmed, blank fields dropped, tags without repeats (ignoring case);
    /// too long a value is refused rather than cut.
    fn normalized(self) -> Result<Self, String> {
        let text = |value: Option<String>, max: usize, what: &str| {
            let value = value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
            match value {
                Some(v) if v.chars().count() > max => {
                    Err(format!("The {what} is longer than {max} characters."))
                }
                v => Ok(v),
            }
        };
        let mut tags: Vec<String> = Vec::new();
        for tag in self.tags {
            let tag = tag.split_whitespace().collect::<Vec<_>>().join(" ");
            if tag.chars().count() > TAG_MAX {
                return Err(format!("A tag is longer than {TAG_MAX} characters."));
            }
            if !tag.is_empty() && !tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
                tags.push(tag);
            }
        }
        if tags.len() > TAGS_MAX {
            return Err(format!("A flight takes at most {TAGS_MAX} tags."));
        }
        Ok(Self {
            title: text(self.title, TITLE_MAX, "title")?,
            notes: text(self.notes, NOTES_MAX, "note")?,
            tags,
        })
    }
}

/// A flight list entry: the summary and what the pilot wrote about it.
#[derive(Debug, Clone, Serialize)]
pub struct Listed {
    #[serde(flatten)]
    pub summary: FlightSummary,
    #[serde(skip_serializing_if = "FlightMeta::is_empty")]
    pub meta: FlightMeta,
}

/// The trash's share of the flights folder.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TrashUsage {
    pub flights: usize,
    pub bytes: u64,
}

/// The flights folder's own files; ids are checked by the caller.
pub struct Store<'a> {
    dir: &'a Path,
}

impl<'a> Store<'a> {
    pub fn new(dir: &'a Path) -> Self {
        Self { dir }
    }

    fn meta_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}{META_EXT}"))
    }

    fn trashed(&self, id: &str) -> PathBuf {
        self.dir.join(TRASH_DIR).join(id)
    }

    /// The meta of flight `id`; a missing or unreadable sidecar is none.
    pub fn meta(&self, id: &str) -> FlightMeta {
        fs::read(self.meta_path(id))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Every flight's meta in one pass over the folder, by id.
    pub fn all_meta(&self) -> BTreeMap<String, FlightMeta> {
        let names = fs::read_dir(self.dir).into_iter().flatten().flatten();
        names
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let id = name.strip_suffix(META_EXT)?;
                Some((id.to_owned(), self.meta(id)))
            })
            .filter(|(_, meta)| !meta.is_empty())
            .collect()
    }

    /// Writes the meta as given, normalised; empty, the sidecar goes.
    pub fn set_meta(&self, id: &str, meta: FlightMeta) -> Result<FlightMeta, String> {
        let meta = meta.normalized()?;
        let path = self.meta_path(id);
        if meta.is_empty() {
            return match fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(meta),
            };
        }
        // Written aside and renamed, so a crash never leaves half a file.
        let part = path.with_extension("json.part");
        let json = serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?;
        fs::write(&part, json)
            .and_then(|()| fs::rename(&part, &path))
            .map_err(|e| e.to_string())?;
        Ok(meta)
    }

    /// The names of flight `id`'s files that exist in `dir`, meta included.
    fn files_in(dir: &Path, id: &str) -> Vec<String> {
        let mut names = flight_file_names(id).to_vec();
        names.push(format!("{id}{META_EXT}"));
        names.retain(|name| dir.join(name).is_file());
        names
    }

    /// Moves flight `id`'s files to the trash, `now` its deletion time.
    pub fn trash(&self, id: &str, now: SystemTime) -> io::Result<()> {
        let names = Self::files_in(self.dir, id);
        if names.is_empty() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no files"));
        }
        let to = self.trashed(id);
        // A flight deleted again (restored and deleted) replaces its older copy.
        if to.exists() {
            fs::remove_dir_all(&to)?;
        }
        fs::create_dir_all(&to)?;
        fs::write(to.join(DELETED_AT), unix(now).to_string())?;
        for name in names {
            fs::rename(self.dir.join(&name), to.join(&name))?;
        }
        Ok(())
    }

    /// Moves a trashed flight's files back; refused when the folder has
    /// files of the same flight again.
    pub fn restore(&self, id: &str) -> Result<(), String> {
        let from = self.trashed(id);
        let names = Self::files_in(&from, id);
        if names.is_empty() {
            return Err(format!("{id} is not in the trash"));
        }
        if !Self::files_in(self.dir, id).is_empty() {
            return Err(format!("{id} is in the flights folder already"));
        }
        for name in names {
            fs::rename(from.join(&name), self.dir.join(&name)).map_err(|e| e.to_string())?;
        }
        fs::remove_dir_all(&from).map_err(|e| e.to_string())
    }

    /// Trashed flights, each with when it was deleted (0: unknown).
    fn trashed_all(&self) -> Vec<(PathBuf, u64)> {
        let entries = fs::read_dir(self.dir.join(TRASH_DIR))
            .into_iter()
            .flatten()
            .flatten();
        entries
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .map(|p| {
                let at = fs::read_to_string(p.join(DELETED_AT)).ok();
                let at = at.and_then(|s| s.trim().parse().ok()).unwrap_or(0);
                (p, at)
            })
            .collect()
    }

    /// Deletes for good the trashed flights deleted before `now - keep`
    /// (all of them for a zero `keep`); how many went.
    pub fn purge(&self, keep: Duration, now: SystemTime) -> usize {
        let before = unix(now).saturating_sub(keep.as_secs());
        let old = self.trashed_all().into_iter();
        let old = old.filter(|(_, at)| keep.is_zero() || *at < before);
        old.filter(|(p, _)| match fs::remove_dir_all(p) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("{}: could not purge: {e}", p.display());
                false
            }
        })
        .count()
    }

    pub fn trash_usage(&self) -> TrashUsage {
        let trashed = self.trashed_all();
        let bytes = trashed
            .iter()
            .flat_map(|(p, _)| fs::read_dir(p).into_iter().flatten().flatten());
        TrashUsage {
            flights: trashed.len(),
            bytes: bytes
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum(),
        }
    }
}

fn unix(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: Duration = Duration::from_secs(24 * 3600);

    fn folder() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for name in [
            "flight-1.fqz",
            "flight-1.raw.jsonl.gz",
            "flight-2.fqz",
            "index.json",
        ] {
            fs::write(tmp.path().join(name), name).unwrap();
        }
        tmp
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_trashed_flight_leaves_the_folder_and_comes_back_intact() {
        let tmp = folder();
        let store = Store::new(tmp.path());
        let meta = FlightMeta {
            title: Some("Checkride".into()),
            ..Default::default()
        };
        store.set_meta("flight-1", meta.clone()).unwrap();
        store.trash("flight-1", SystemTime::now()).unwrap();
        assert_eq!(names(tmp.path()), [".trash", "flight-2.fqz", "index.json"]);
        assert_eq!(store.trash_usage().flights, 1);
        assert_eq!(store.meta("flight-1"), FlightMeta::default());
        store.restore("flight-1").unwrap();
        assert_eq!(
            names(tmp.path()),
            [
                ".trash",
                "flight-1.fqz",
                "flight-1.meta.json",
                "flight-1.raw.jsonl.gz",
                "flight-2.fqz",
                "index.json"
            ]
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("flight-1.fqz")).unwrap(),
            "flight-1.fqz"
        );
        assert_eq!(store.meta("flight-1"), meta);
        assert_eq!(store.trash_usage(), TrashUsage::default());
        assert!(
            store.restore("flight-1").is_err(),
            "not in the trash any more"
        );
        assert!(
            store.trash("flight-9", SystemTime::now()).is_err(),
            "no such flight"
        );
    }

    #[test]
    fn a_restore_never_overwrites_a_flight_in_the_folder() {
        let tmp = folder();
        let store = Store::new(tmp.path());
        store.trash("flight-2", SystemTime::now()).unwrap();
        fs::write(tmp.path().join("flight-2.fqz"), "new").unwrap();
        assert!(store.restore("flight-2").is_err());
        assert_eq!(
            fs::read_to_string(tmp.path().join("flight-2.fqz")).unwrap(),
            "new"
        );
    }

    #[test]
    fn the_trash_is_purged_after_its_keep_or_on_demand() {
        let tmp = folder();
        let store = Store::new(tmp.path());
        let now = SystemTime::now();
        store.trash("flight-1", now - 31 * DAY).unwrap();
        store.trash("flight-2", now - DAY).unwrap();
        assert_eq!(store.purge(TRASH_KEEP, now), 1);
        assert!(store.restore("flight-1").is_err());
        assert_eq!(store.trash_usage().flights, 1);
        assert_eq!(store.purge(Duration::ZERO, now), 1);
        assert_eq!(store.trash_usage(), TrashUsage::default());
    }

    #[test]
    fn meta_is_normalised_kept_and_removed_when_empty() {
        let tmp = folder();
        let store = Store::new(tmp.path());
        let given = FlightMeta {
            title: Some("  Gusty 28L ".into()),
            notes: Some(" ".into()),
            tags: vec![
                "VATSIM  event".into(),
                "vatsim event".into(),
                "".into(),
                "practice".into(),
            ],
        };
        let kept = store.set_meta("flight-1", given).unwrap();
        assert_eq!(kept.title.as_deref(), Some("Gusty 28L"));
        assert_eq!(
            (kept.notes.as_ref(), kept.tags.as_slice()),
            (
                None,
                ["VATSIM event", "practice"].map(String::from).as_slice()
            )
        );
        assert_eq!(store.meta("flight-1"), kept);
        assert_eq!(
            store.all_meta().into_keys().collect::<Vec<_>>(),
            ["flight-1"]
        );
        let long = FlightMeta {
            title: Some("x".repeat(121)),
            ..Default::default()
        };
        assert!(store.set_meta("flight-1", long).is_err());
        assert_eq!(
            store.meta("flight-1"),
            kept,
            "a refused write changes nothing"
        );
        store.set_meta("flight-1", FlightMeta::default()).unwrap();
        assert!(!tmp.path().join("flight-1.meta.json").exists());
        assert!(store.all_meta().is_empty());
    }
}
