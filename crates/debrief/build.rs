//! Bakes `SOURCE_HASH`, a hash of the code a flight summary comes from, so
//! every build of the same source (`fdm`, the app, debug or release) shares
//! the flights folder's index and any change to it rebuilds the index.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::{fs, io};

const CRATES: [&str; 5] = ["analyzer", "debrief", "fdm-core", "recorder", "rules"];

fn main() -> io::Result<()> {
    let mut h = DefaultHasher::new();
    for name in CRATES {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../{name}/src"));
        println!("cargo::rerun-if-changed={}", src.display());
        let mut files: Vec<_> = fs::read_dir(&src)?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<_, _>>()?;
        files.sort();
        for file in files {
            fs::read(file)?.hash(&mut h);
        }
    }
    println!("cargo::rustc-env=SOURCE_HASH={:016x}", h.finish());
    Ok(())
}
