//! The local challenge library: challenge files the user opened, kept as
//! the exact bytes imported (docs/challenges.md), one file per id and
//! version.

use std::fs;
use std::io;
use std::path::PathBuf;

use challenge::Challenge;
use serde::Serialize;
use serde::de::IgnoredAny;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Larger than any definition; a bigger file is not a challenge.
const MAX_BYTES: u64 = 64 * 1024;

/// A challenge in the library, as the list shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    /// `<id>-v<version>`, also the file name.
    pub key: String,
    pub id: String,
    pub version: u32,
    pub title: String,
    pub airport: String,
    pub runway: String,
    /// Of the stored bytes.
    pub sha256: String,
}

/// The briefing: the entry and its definition as written.
#[derive(Debug, Clone, Serialize)]
pub struct Briefing {
    #[serde(flatten)]
    pub entry: Entry,
    pub definition: Value,
}

pub struct Library {
    dir: PathBuf,
}

impl Library {
    pub fn new(dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    /// Checks a challenge file and keeps it, replacing the same id and
    /// version. Errors are in plain words.
    pub fn import(&self, json: &str) -> Result<Entry, String> {
        if let Err(e) = serde_json::from_str::<IgnoredAny>(json) {
            return Err(format!(
                "{NOT_A_CHALLENGE}it isn't valid JSON (line {}).",
                e.line()
            ));
        }
        let c = Challenge::parse(json).map_err(|e| plain(&e))?;
        let entry = entry(&c, json);
        if let Ok((kept, _)) = self.get(&entry.key)
            && kept.id != c.id
        {
            return Err(format!(
                "A different challenge, `{}`, is already kept under the same name; change this one's id.",
                kept.id
            ));
        }
        // Always written: a conflicting copy is replaced here, the callers
        // having asked first (`conflict`).
        fs::write(self.dir.join(format!("{}.json", entry.key)), json).map_err(|e| e.to_string())?;
        Ok(entry)
    }

    /// The entry kept under the same id and version as `json` with other
    /// bytes, which importing `json` would replace. `None` when there is
    /// none, the same bytes are kept, or `json` isn't a challenge.
    pub fn conflict(&self, json: &str) -> Option<Entry> {
        let c = Challenge::parse(json).ok()?;
        let (kept, bytes) = self.get(&key(&c)).ok()?;
        (bytes != json).then(|| entry(&kept, &bytes))
    }

    /// Reads a file the user picked, then imports it.
    pub fn import_file(&self, path: &std::path::Path) -> Result<Entry, String> {
        let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
        if size > MAX_BYTES {
            return Err(format!(
                "This file is {} KB; a challenge file is a few KB of JSON.",
                size / 1024
            ));
        }
        let json = fs::read_to_string(path)
            .map_err(|_| "This file isn't text, so it can't be a challenge.".to_owned())?;
        self.import(json.trim_start_matches('\u{feff}'))
    }

    /// Every challenge kept, by title.
    pub fn list(&self) -> Vec<Entry> {
        let files = fs::read_dir(&self.dir).into_iter().flatten().flatten();
        let mut entries: Vec<Entry> = files
            .filter_map(|f| {
                let key = f.path().file_stem()?.to_str()?.to_owned();
                self.get(&key).ok().map(|(c, json)| entry(&c, &json))
            })
            .collect();
        entries.sort_by(|a, b| a.title.cmp(&b.title).then(a.key.cmp(&b.key)));
        entries
    }

    /// The challenge and its stored bytes.
    pub fn get(&self, key: &str) -> Result<(Challenge, String), String> {
        let json =
            fs::read_to_string(self.path(key)?).map_err(|_| format!("no challenge {key:?}"))?;
        let c = Challenge::parse(&json).map_err(|e| plain(&e))?;
        Ok((c, json))
    }

    pub fn briefing(&self, key: &str) -> Result<Briefing, String> {
        let (c, json) = self.get(key)?;
        Ok(Briefing {
            entry: entry(&c, &json),
            definition: serde_json::from_str(&json).map_err(|e| e.to_string())?,
        })
    }

    /// Forgets a challenge; its attempts stay.
    pub fn remove(&self, key: &str) -> Result<(), String> {
        fs::remove_file(self.path(key)?).map_err(|_| format!("no challenge {key:?}"))
    }

    fn path(&self, key: &str) -> Result<PathBuf, String> {
        checked_key(key).map(|key| self.dir.join(format!("{key}.json")))
    }
}

/// `key` if it is one `key` makes: nothing else can name a path outside.
pub(crate) fn checked_key(key: &str) -> Result<&str, String> {
    let ok = !key.is_empty()
        && key.len() <= 80
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    ok.then_some(key)
        .ok_or_else(|| format!("no challenge {key:?}"))
}

fn entry(c: &Challenge, json: &str) -> Entry {
    Entry {
        key: key(c),
        id: c.id.clone(),
        version: c.version,
        title: c.title.clone(),
        airport: c.airport.clone(),
        runway: c.runway.clone(),
        sha256: sha256_hex(json.as_bytes()),
    }
}

/// `<id>-v<version>`, lower case, anything but letters and digits as `-`.
fn key(c: &Challenge) -> String {
    let id: String =
        c.id.chars()
            .take(64)
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
    format!("{id}-v{}", c.version)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// `Challenge::parse`'s message as a sentence for the pilot.
fn plain(error: &str) -> String {
    // serde_json ends with " at line L column C".
    let (what, line) = match error.rsplit_once(" at line ") {
        Some((what, at)) => (what, at.split(' ').next().map(|l| format!(" (line {l})"))),
        None => (error, None),
    };
    let line = line.unwrap_or_default();
    let quoted = |s: &str| s.split('`').nth(1).unwrap_or("?").to_owned();
    let why = if what.starts_with("unknown field") {
        format!(
            "it has a field FOQA-Sim doesn't know, `{}`{line}. It may be made for a newer FOQA-Sim.",
            quoted(what)
        )
    } else if what.starts_with("missing field") {
        format!("it has no `{}`{line}.", quoted(what))
    } else if what.starts_with("unknown variant") {
        format!("`{}` isn't an allowed value{line}.", quoted(what))
    } else if what.starts_with("invalid type") {
        format!("a value has the wrong type{line}: {what}.")
    } else if what.starts_with("unknown scoring") {
        format!("its scoring, `{}`, needs a newer FOQA-Sim.", quoted(what))
    } else {
        format!("{what}{line}.")
    };
    format!("{NOT_A_CHALLENGE}{why}")
}

const NOT_A_CHALLENGE: &str = "This file isn't a challenge FOQA-Sim can fly: ";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attempts::tests::JSON;

    fn library() -> (tempfile::TempDir, Library) {
        let tmp = tempfile::tempdir().unwrap();
        let library = Library::new(tmp.path().join("challenges")).unwrap();
        (tmp, library)
    }

    #[test]
    fn a_challenge_is_imported_listed_briefed_and_removed() {
        let (_tmp, lib) = library();
        let entry = lib.import(JSON).unwrap();
        assert_eq!(
            (entry.key.as_str(), entry.title.as_str()),
            ("test-20-v1", "Runway 20")
        );
        assert_eq!(entry.sha256, sha256_hex(JSON.as_bytes()));
        // The same id and version replaces it; the next version is another entry.
        lib.import(JSON).unwrap();
        lib.import(&JSON.replace(r#""version": 1"#, r#""version": 2"#))
            .unwrap();
        let keys: Vec<_> = lib.list().into_iter().map(|e| e.key).collect();
        assert_eq!(keys, ["test-20-v1", "test-20-v2"]);
        let briefing = lib.briefing("test-20-v1").unwrap();
        assert_eq!(briefing.definition["gate"]["distance_nm"], 3);
        assert_eq!(
            lib.get("test-20-v1").unwrap().1,
            JSON,
            "the exact bytes are kept"
        );
        lib.remove("test-20-v1").unwrap();
        assert_eq!(lib.list().len(), 1);
        assert!(lib.remove("test-20-v1").is_err());
    }

    #[test]
    fn another_copy_of_a_kept_challenge_is_a_conflict() {
        let (_tmp, lib) = library();
        assert_eq!(lib.conflict(JSON), None, "nothing kept yet");
        lib.import(JSON).unwrap();
        assert_eq!(lib.conflict(JSON), None, "the same bytes");
        let other = JSON.replace("Runway 20", "Runway 20, edited");
        let kept = lib.conflict(&other).unwrap();
        assert_eq!(
            (kept.key.as_str(), kept.title.as_str()),
            ("test-20-v1", "Runway 20")
        );
        assert_eq!(lib.conflict("not json"), None);
        let next = JSON.replace(r#""version": 1"#, r#""version": 2"#);
        assert_eq!(
            lib.conflict(&next),
            None,
            "another version is another entry"
        );
    }

    #[test]
    fn importing_a_conflicting_copy_replaces_the_kept_bytes() {
        let (_tmp, lib) = library();
        lib.import(JSON).unwrap();
        let other = JSON.replace("Runway 20", "Runway 20, edited");
        let entry = lib.import(&other).unwrap();
        assert_eq!(lib.get(&entry.key).unwrap().1, other);
        assert_eq!(lib.list()[0].sha256, entry.sha256);
        assert_eq!(lib.conflict(&other), None);
    }

    #[test]
    fn keys_cannot_name_a_path_outside_the_library() {
        let (_tmp, lib) = library();
        let odd = JSON.replace(r#""id": "test-20""#, r#""id": "../Ünï/x""#);
        assert_eq!(lib.import(&odd).unwrap().key, "----n--x-v1");
        for key in ["../x", "a/b", "A", "", "c:\\x"] {
            assert!(lib.get(key).is_err() && lib.remove(key).is_err(), "{key}");
        }
    }

    #[test]
    fn ids_that_share_a_key_do_not_overwrite_each_other() {
        let (_tmp, lib) = library();
        let with = |id: &str| JSON.replace(r#""id": "test-20""#, &format!(r#""id": "{id}""#));
        lib.import(&with("a.b")).unwrap();
        assert!(lib.import(&with("a_b")).is_err() && lib.import(&with("A-B")).is_err());
        assert_eq!(lib.get("a-b-v1").unwrap().0.id, "a.b");
        lib.import(&with("a.b")).unwrap();
    }

    #[test]
    fn errors_are_in_plain_words() {
        let (_tmp, lib) = library();
        let err = |json: &str| lib.import(json).unwrap_err();
        let prefix = NOT_A_CHALLENGE;
        assert_eq!(
            err("{ nope"),
            format!("{prefix}it isn't valid JSON (line 1).")
        );
        assert_eq!(
            err(&JSON.replace(r#""title""#, r#""name""#)),
            format!(
                "{prefix}it has a field FOQA-Sim doesn't know, `name` (line 2). It may be made for a newer FOQA-Sim."
            )
        );
        assert_eq!(
            err(&JSON.replace(r#""distance_nm": 3"#, r#""distance_nm": 9"#)),
            format!("{prefix}the gate must be closer to the runway than the start.")
        );
        assert_eq!(
            err(&JSON.replace("landing-1", "landing-9")),
            format!("{prefix}its scoring, `landing-9`, needs a newer FOQA-Sim.")
        );
        assert!(err(r#"{"id": "x"}"#).contains("it has no `"));
        let big = tempfile::NamedTempFile::new().unwrap();
        fs::write(big.path(), vec![b' '; 70 * 1024]).unwrap();
        assert!(lib.import_file(big.path()).unwrap_err().contains("70 KB"));
    }
}
