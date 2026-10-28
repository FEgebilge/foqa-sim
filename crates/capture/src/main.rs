//! foqa-rec: waits on MSFS and records every flight of this sim session.
//!
//! `foqa-rec [--raw] [output-dir]` (default `flights`). Stop with Ctrl+C.
//! Each flight is compacted when it closes, and journals left by earlier
//! runs when this starts. `--raw` also keeps every flight's raw capture.

use std::error::Error;
use std::fs;
use std::ops::ControlFlow;
use std::path::PathBuf;

use capture::{LiveSource, Progress};
use fdm_core::Lfl;
use recorder::Acquisition;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let raw = args.first().is_some_and(|a| a == "--raw");
    if raw {
        args.remove(0);
    }
    let dir = PathBuf::from(args.pop().unwrap_or_else(|| "flights".into()));
    fs::create_dir_all(&dir)?;
    capture::recover(&dir, capture::backlog(&dir));
    let acquisition = Acquisition::new(&Lfl::V1);
    let mut source = LiveSource::open(&acquisition)?;
    eprintln!("recording flights to {}", dir.display());
    capture::record(&mut source, &acquisition, &dir, raw, |p| {
        if let Progress::Saved(journal) = p {
            debrief::compact_all([journal.to_owned()]);
        }
        ControlFlow::Continue(None)
    });
    eprintln!("simulator closed");
    Ok(())
}
