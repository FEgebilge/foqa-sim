//! Embeds every `profiles/<ICAO>.yaml` of the repository as (type, YAML), so
//! shipping a type profile takes nothing but adding its file.

use std::path::Path;
use std::{env, fs, io};

fn main() -> io::Result<()> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../profiles");
    println!("cargo::rerun-if-changed={}", dir.display());
    let mut files = Vec::new();
    for entry in fs::read_dir(&dir)? {
        files.push(entry?.path());
    }
    files.sort();
    let mut list = String::new();
    for path in &files {
        let icao = match (path.extension(), path.file_stem()) {
            (Some(ext), Some(stem)) if ext == "yaml" => stem.to_string_lossy(),
            _ => {
                let msg = format!("unexpected file in profiles/: {}", path.display());
                return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
            }
        };
        list += &format!(
            "({icao:?}, include_str!({:?})),\n",
            path.display().to_string()
        );
    }
    let out_dir = env::var_os("OUT_DIR").ok_or(io::ErrorKind::NotFound)?;
    fs::write(
        Path::new(&out_dir).join("profiles.rs"),
        format!("&[\n{list}]\n"),
    )
}
