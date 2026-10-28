//! The log file. A release build has no console (`windows_subsystem`), so
//! everything written to stderr, the app's `eprintln!`s, the crates' and a
//! thread's panic message, would be lost; stderr is pointed at a file instead.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Past this the file is kept as `.1` (replacing an older one) at the next start.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

static PATH: OnceLock<PathBuf> = OnceLock::new();

/// Where the log is, once `init` has run.
pub fn path() -> Option<&'static Path> {
    PATH.get().map(PathBuf::as_path)
}

/// Opens `<dir>/foqa-sim.log`, rotating it first, and sends stderr there.
/// Failing to log never stops the app.
pub fn init(dir: &Path) {
    let path = dir.join("foqa-sim.log");
    match open(&path).and_then(redirect_stderr) {
        Ok(()) => {
            let _ = PATH.set(path);
            eprintln!(
                "FOQA-Sim {} started {}",
                env!("CARGO_PKG_VERSION"),
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            );
        }
        Err(e) => eprintln!("no log file at {}: {e}", path.display()),
    }
}

fn open(path: &Path) -> std::io::Result<File> {
    fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    if fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = fs::rename(path, path.with_extension("log.1"));
    }
    OpenOptions::new().create(true).append(true).open(path)
}

/// Rust's stderr asks Windows for the handle at each write, so replacing the
/// process's handle captures every `eprintln!` in the process. The handle
/// stays open for the life of the process.
#[cfg(windows)]
fn redirect_stderr(file: File) -> std::io::Result<()> {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::System::Console::{STD_ERROR_HANDLE, SetStdHandle};
    let handle = file.into_raw_handle();
    // SAFETY: `handle` is a valid, open file handle that is never closed.
    if unsafe { SetStdHandle(STD_ERROR_HANDLE, handle) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// The app is Windows-only (SimConnect); elsewhere stderr stays.
#[cfg(not(windows))]
fn redirect_stderr(_: File) -> std::io::Result<()> {
    Ok(())
}

/// Tells the user why the app can't start, with the log's place.
pub fn start_failed(app: &tauri::AppHandle, why: &dyn std::fmt::Display) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    let log = path().map_or(String::new(), |p| format!("\n\nLog: {}", p.display()));
    app.dialog()
        .message(format!("FOQA-Sim could not start: {why}{log}"))
        .title("FOQA-Sim")
        .kind(MessageDialogKind::Error)
        .blocking_show();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn a_big_log_is_rotated_once_at_open() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("foqa-sim.log");
        fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();
        fs::write(path.with_extension("log.1"), b"older").unwrap();
        open(&path).unwrap().write_all(b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(
            fs::metadata(path.with_extension("log.1")).unwrap().len(),
            MAX_BYTES + 1
        );
        open(&path).unwrap().write_all(b"+").unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            b"new+",
            "a small log is appended to"
        );
    }
}
