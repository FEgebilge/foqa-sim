use std::env;
use std::fs;
use std::path::{Path, PathBuf};
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

/// The VC++ runtime DLLs the app needs (msvcp140 for SimConnect.lib, and
/// what msvcp140 itself loads). Installed next to the exe (app-local
/// deployment, which the Visual C++ redistribution terms allow), so no
/// redistributable install is needed.
const VC_RUNTIME: [&str; 3] = ["msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"];

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
    copy_vc_runtime();
    tauri_build::build()
}

/// The newest `VC\Redist\MSVC\<version>\x64\Microsoft.VC14*.CRT` of the
/// latest Visual Studio (Build Tools) install, found with vswhere.
fn vc_redist() -> Option<PathBuf> {
    let programs = env::var_os("ProgramFiles(x86)")?;
    let vswhere = Path::new(&programs).join(r"Microsoft Visual Studio\Installer\vswhere.exe");
    let out = Command::new(vswhere)
        .args(["-latest", "-products", "*", "-property", "installationPath"])
        .output()
        .ok()?;
    let vs = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
    let mut crts: Vec<PathBuf> = fs::read_dir(vs.join(r"VC\Redist\MSVC"))
        .ok()?
        .flatten()
        .flat_map(|version| {
            fs::read_dir(version.path().join("x64"))
                .into_iter()
                .flatten()
                .flatten()
        })
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(".CRT"))
        })
        .filter(|p| p.join(VC_RUNTIME[0]).is_file())
        .collect();
    crts.sort();
    crts.pop()
}

/// Copies `VC_RUNTIME` to `vcrt` (not in git), where tauri.conf.json
/// bundles it from.
fn copy_vc_runtime() {
    let dest = Path::new("vcrt");
    if VC_RUNTIME.iter().all(|dll| dest.join(dll).is_file()) {
        return;
    }
    let crt = vc_redist().expect(
        "no VC++ redistributable files: install Visual Studio Build Tools with the C++ workload",
    );
    fs::create_dir_all(dest).expect("vcrt folder");
    for dll in VC_RUNTIME {
        fs::copy(crt.join(dll), dest.join(dll)).unwrap_or_else(|e| panic!("{dll}: {e}"));
    }
}
