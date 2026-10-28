//! The mark an update leaves for the app the installer restarts. That app
//! starts like any other, hidden in the tray, so without the mark an update
//! looks as if the app never came back. The window asks for the mark before
//! it hands over to the installer; the next start takes it and shows itself.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// An installer run takes seconds; an older mark is a leftover (an update
/// that failed after the mark) and must not raise the window at a later start.
const FRESH: Duration = Duration::from_secs(10 * 60);

fn mark_path() -> PathBuf {
    std::env::temp_dir().join("foqa-sim-updated")
}

fn leave_at(path: &Path) -> std::io::Result<()> {
    std::fs::write(path, b"")
}

/// True once, when `path` holds a mark younger than `FRESH` at `now`.
fn take_at(path: &Path, now: SystemTime) -> bool {
    let fresh = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|at| now.duration_since(at).is_ok_and(|age| age < FRESH));
    let _ = std::fs::remove_file(path);
    fresh
}

/// The update is about to be installed: the next start shows its window.
#[tauri::command]
pub fn update_mark() {
    if let Err(e) = leave_at(&mark_path()) {
        eprintln!("update mark not written: {e}");
    }
}

/// Whether this start follows an update (and forgets that it does).
pub fn take() -> bool {
    take_at(&mark_path(), SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_mark_is_taken_once() {
        let path = std::env::temp_dir().join(format!("foqa-sim-mark-{}", std::process::id()));
        leave_at(&path).unwrap();
        assert!(take_at(&path, SystemTime::now()));
        assert!(!take_at(&path, SystemTime::now()));
    }

    #[test]
    fn a_stale_mark_is_dropped_without_showing() {
        let path = std::env::temp_dir().join(format!("foqa-sim-stale-{}", std::process::id()));
        leave_at(&path).unwrap();
        assert!(!take_at(&path, SystemTime::now() + FRESH * 2));
        assert!(!path.exists());
    }
}
