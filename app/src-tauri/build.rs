use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// What `assemble.ps1` reads, from `msfs-panel`; the app version is the packages'.
const INPUTS: [&str; 6] = [
    "assemble.ps1",
    "layout.ps1",
    "prebuilt",
    "PackageSources",
    "efb",
    "../app/src-tauri/tauri.conf.json",
];

fn modified(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(UNIX_EPOCH)
}

/// The newest modification time under `path`.
fn newest(path: &Path) -> SystemTime {
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| newest(&e.path()))
        .fold(modified(path), SystemTime::max)
}

/// Assembles the in-sim packages tauri.conf.json bundles as resources
/// (`msfs-panel\out`, not in git), so a fresh clone builds too. Only when they
/// are out of date: tauri_build watches resources, so rewriting them on every
/// run would rerun this script on every build.
fn main() {
    let panel = Path::new("../../msfs-panel");
    let inputs = INPUTS.map(|input| panel.join(input));
    for input in &inputs {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    let built = ["foqa-sim-panel", "foqa-sim-efb"]
        .map(|name| modified(&panel.join("out/Packages").join(name).join("manifest.json")));
    if built.into_iter().min() <= inputs.iter().map(|input| newest(input)).max() {
        let assemble = panel.join("assemble.ps1");
        let status = Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&assemble)
            .status()
            .expect("powershell");
        assert!(status.success(), "{} failed", assemble.display());
    }
    tauri_build::build()
}
