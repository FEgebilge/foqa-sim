//! Embeds every `profiles/<ICAO>.yaml` of the repository as (type, YAML), so
//! shipping a type profile takes nothing but adding its file.

use std::path::Path;
use std::{env, fs};

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../profiles");
    println!("cargo::rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = fs::read_dir(&dir)
        .expect("profiles/ directory")
        .map(|entry| entry.expect("profiles/ entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yaml"))
        .collect();
    files.sort();
    let list: String = files
        .iter()
        .map(|path| {
            let icao = path.file_stem().unwrap().to_string_lossy();
            format!(
                "({icao:?}, include_str!({:?})),\n",
                path.display().to_string()
            )
        })
        .collect();
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("profiles.rs");
    fs::write(out, format!("&[\n{list}]\n")).unwrap();
}
