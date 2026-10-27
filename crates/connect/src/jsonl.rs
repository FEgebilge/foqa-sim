//! Append-only JSON lines: `site.log.jsonl` per attempt and the connect
//! log, `{at, seq, event, …}` (B5). Lines are never rewritten; a line cut
//! off by a crash is skipped on reading.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use chrono::{SecondsFormat, Utc};
use serde_json::{Map, Value};

pub fn read(path: &Path) -> Vec<Value> {
    let bytes = fs::read(path).unwrap_or_default();
    bytes
        .split(|&b| b == b'\n')
        .filter_map(|l| serde_json::from_slice(l).ok())
        .collect()
}

/// Appends `fields` (an object with `event`) with `at` and the next `seq`.
pub fn append(path: &Path, fields: Value) -> std::io::Result<()> {
    let seq = read(path)
        .iter()
        .filter_map(|l| l["seq"].as_u64())
        .max()
        .unwrap_or(0)
        + 1;
    let cut = fs::read(path).is_ok_and(|b| b.last().is_some_and(|&c| c != b'\n'));
    let mut line = Map::new();
    line.insert("at".into(), now().into());
    line.insert("seq".into(), seq.into());
    if let Value::Object(fields) = fields {
        line.extend(fields);
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let start = if cut { "\n" } else { "" };
    writeln!(file, "{start}{}", Value::Object(line))
}

/// Moves `path` to `.1` (and `.1` to `.2`) once it passes `max` bytes.
pub fn rotate(path: &Path, max: u64, keep: u32) {
    if fs::metadata(path).map_or(true, |m| m.len() < max) {
        return;
    }
    let name = |n: u32| path.with_extension(format!("jsonl.{n}"));
    for n in (1..keep).rev() {
        let _ = fs::rename(name(n), name(n + 1));
    }
    let _ = fs::rename(path, name(1));
}

pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lines_are_numbered_a_cut_line_is_skipped_and_the_log_rotates() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("connect.log.jsonl");
        append(&path, json!({ "event": "site_added" })).unwrap();
        fs::write(
            &path,
            [fs::read(&path).unwrap(), b"{\"at\":".to_vec()].concat(),
        )
        .unwrap();
        append(&path, json!({ "event": "revoke" })).unwrap();
        let lines = read(&path);
        assert_eq!(lines.len(), 2);
        assert_eq!(
            (lines[1]["seq"].as_u64(), lines[1]["event"].as_str()),
            (Some(2), Some("revoke"))
        );
        rotate(&path, 10, 3);
        assert!(!path.exists() && path.with_extension("jsonl.1").exists());
    }
}
