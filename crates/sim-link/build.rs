//! Links SimConnect statically from the MSFS 2024 SDK, so binaries need no
//! SimConnect.dll. The SDK is looked up in `MSFS2024_SDK`, else its default
//! folder. The library uses the dynamic CRT (/MD).

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-env-changed=MSFS2024_SDK");
    let sdk = env::var_os("MSFS2024_SDK").unwrap_or_else(|| r"C:\MSFS 2024 SDK".into());
    let dir = PathBuf::from(sdk).join(r"SimConnect SDK\lib\static");
    if !dir.join("SimConnect.lib").is_file() {
        println!(
            "cargo::error={} has no SimConnect.lib: install the MSFS 2024 SDK (MSFS Developer \
             Mode, Help > SDK Installer) or set MSFS2024_SDK to its folder",
            dir.display()
        );
        return;
    }
    println!("cargo::rustc-link-search=native={}", dir.display());
    println!("cargo::rustc-link-lib=static=SimConnect");
    // System libraries SimConnect.lib calls into.
    for sys in ["ws2_32", "shlwapi", "user32", "advapi32", "shell32"] {
        println!("cargo::rustc-link-lib={sys}");
    }
}
