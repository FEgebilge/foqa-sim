//! Which closed recordings are not flights. The sim streams frames in its
//! menus too (the aircraft parked at a placeholder position), so a recording
//! can open there; one that holds nothing worth analysing is deleted when it
//! closes instead of being compacted and listed. A journal an earlier run
//! never closed (the app or the PC stopped mid-flight) gets the same verdict
//! when the next run recovers it.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use fdm_core::Lfl;
use recorder::{Column, JournalReader, Record, Telemetry};

/// A recording shorter than this (sim seconds, first to last frame) is
/// dropped: a single menu sample, a reload or a mis-click is not a flight.
pub const MIN_FLIGHT_S: f64 = 10.0;
/// The position the sim reports in its menus: latitude 0, longitude 90.
pub const MENU_POSITION: (f64, f64) = (0.0, 90.0);
/// How close to `MENU_POSITION` (degrees) a frame counts as being there.
pub const MENU_TOLERANCE_DEG: f64 = 0.01;
/// What compaction writes before it renames (`recorder::compact_file`).
const COMPACT_PART: &str = ".fqz.part";

/// Why a closed recording is dropped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Discard {
    /// Less than `MIN_FLIGHT_S` of frames (zero for a single sample).
    TooShort { seconds: f64 },
    /// Every position recorded is the sim's menu placeholder, or none is.
    NeverPositioned,
}

impl fmt::Display for Discard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { seconds } => {
                write!(
                    f,
                    "only {seconds:.0} s recorded (under {MIN_FLIGHT_S:.0} s)"
                )
            }
            Self::NeverPositioned => f.write_str("never left the menu position"),
        }
    }
}

/// What a recording adds up to: its sim-time span and whether any position
/// is a real one.
#[derive(Debug, Clone, Default)]
struct Tally {
    span: Option<(f64, f64)>,
    positioned: bool,
}

impl Tally {
    fn time(&mut self, t: f64) {
        if t.is_finite() {
            let (lo, hi) = self.span.unwrap_or((t, t));
            self.span = Some((lo.min(t), hi.max(t)));
        }
    }

    fn position(&mut self, lat: f64, lon: f64) {
        if lat.is_finite() && lon.is_finite() {
            let at_menu = (lat - MENU_POSITION.0).abs() < MENU_TOLERANCE_DEG
                && (lon - MENU_POSITION.1).abs() < MENU_TOLERANCE_DEG;
            self.positioned |= !at_menu;
        }
    }

    /// Why this recording is not a flight, if it is not. Ground-only
    /// recordings at a real position are flights: taxi practice counts.
    fn verdict(&self) -> Option<Discard> {
        let seconds = self.span.map_or(0.0, |(lo, hi)| hi - lo);
        if seconds < MIN_FLIGHT_S {
            Some(Discard::TooShort { seconds })
        } else if !self.positioned {
            Some(Discard::NeverPositioned)
        } else {
            None
        }
    }
}

/// What a recording's frames add up to, kept as they are recorded.
#[derive(Debug, Clone)]
pub struct Usage {
    time: usize,
    lat: usize,
    lon: usize,
    tally: Tally,
}

impl Usage {
    pub fn new(columns: &[Column]) -> Self {
        let column = |simvar| {
            columns
                .iter()
                .position(|c| c.simvar == simvar)
                .unwrap_or_else(|| panic!("acquisition does not capture {simvar}"))
        };
        Self {
            time: column("SIMULATION TIME"),
            lat: column("PLANE LATITUDE"),
            lon: column("PLANE LONGITUDE"),
            tally: Tally::default(),
        }
    }

    pub fn see(&mut self, item: &Telemetry) {
        let Telemetry::Frame(frame) = item else {
            return;
        };
        let at = |i: usize| frame.get(i).copied().unwrap_or(f64::NAN);
        self.tally.time(at(self.time));
        self.tally.position(at(self.lat), at(self.lon));
    }

    /// Why this recording is not a flight, if it is not (`Discard`).
    pub fn verdict(&self) -> Option<Discard> {
        self.tally.verdict()
    }
}

/// `Usage::verdict` from a journal's records: the samples' sim time, and
/// each longitude with the latitude before it.
fn journal_verdict(journal: &Path) -> io::Result<Option<Discard>> {
    let reader = JournalReader::new(BufReader::new(File::open(journal)?))?;
    let id = |name| Lfl::V1.by_name(name).map(|s| s.id);
    let (lat_id, lon_id) = (id("pos.lat"), id("pos.lon"));
    let (mut tally, mut lat) = (Tally::default(), f64::NAN);
    for record in reader {
        let Record::Sample(s) = record? else {
            continue;
        };
        tally.time(s.t.as_secs_f64());
        if Some(s.signal) == lat_id {
            lat = s.value;
        } else if Some(s.signal) == lon_id {
            tally.position(lat, s.value);
        }
    }
    Ok(tally.verdict())
}

/// The journals an earlier run left in `dir`, listed before any new flight
/// exists, for `recover`. Compaction leftovers (`.fqz.part`, a quit while
/// compacting) are deleted now: no compaction runs yet.
pub fn backlog(dir: &Path) -> Vec<PathBuf> {
    let entries = fs::read_dir(dir).into_iter().flatten().flatten();
    for part in entries.map(|e| e.path()).filter(|p| {
        p.file_name()
            .is_some_and(|n| n.to_string_lossy().ends_with(COMPACT_PART))
    }) {
        match fs::remove_file(&part) {
            Ok(()) => eprintln!("deleted a stale {}", part.display()),
            Err(e) => eprintln!("{}: could not delete: {e}", part.display()),
        }
    }
    debrief::journals(dir)
}

/// Closes the journals of `backlog` as their run never did: one that is
/// not a flight (`Discard`) is deleted with its raw capture, the rest are
/// compacted, a cut-off tail dropped. Reads each journal twice: give it a
/// thread of its own.
pub fn recover(dir: &Path, backlog: Vec<PathBuf>) {
    let flights = backlog.into_iter().filter(|journal| {
        let Ok(Some(why)) = journal_verdict(journal) else {
            return true;
        };
        match debrief::delete_flight(dir, &debrief::flight_id(journal)) {
            Ok(_) => eprintln!("info: {} discarded, {why}", journal.display()),
            Err(e) => eprintln!("{}: could not delete: {e}", journal.display()),
        }
        false
    });
    debrief::compact_all(flights);
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLUMNS: [Column; 3] = [
        Column {
            simvar: "SIMULATION TIME",
            unit: "seconds",
        },
        Column {
            simvar: "PLANE LATITUDE",
            unit: "degrees",
        },
        Column {
            simvar: "PLANE LONGITUDE",
            unit: "degrees",
        },
    ];

    /// Frames once a second for `seconds` (inclusive) at `(lat, lon)`.
    fn verdict(seconds: u32, (lat, lon): (f64, f64)) -> Option<Discard> {
        let mut usage = Usage::new(&COLUMNS);
        for t in 0..=seconds {
            usage.see(&Telemetry::Frame(vec![100.0 + f64::from(t), lat, lon]));
        }
        usage.verdict()
    }

    const MENU: (f64, f64) = (0.0, 90.0);
    const IZMIR: (f64, f64) = (38.29, 27.16);

    #[test]
    fn a_single_sample_is_dropped() {
        assert_eq!(verdict(0, IZMIR), Some(Discard::TooShort { seconds: 0.0 }));
        assert_eq!(verdict(0, MENU), Some(Discard::TooShort { seconds: 0.0 }));
    }

    #[test]
    fn six_seconds_at_the_menu_position_are_dropped() {
        assert!(verdict(6, MENU).is_some());
    }

    #[test]
    fn nine_seconds_at_an_airport_are_dropped() {
        assert_eq!(verdict(9, IZMIR), Some(Discard::TooShort { seconds: 9.0 }));
    }

    #[test]
    fn a_long_recording_that_never_left_the_menu_is_dropped() {
        assert_eq!(verdict(120, MENU), Some(Discard::NeverPositioned));
        assert_eq!(
            verdict(120, (0.005, 90.005)),
            Some(Discard::NeverPositioned)
        );
    }

    #[test]
    fn thirty_seconds_on_the_ground_at_an_airport_are_kept() {
        assert_eq!(verdict(30, IZMIR), None);
        assert_eq!(verdict(10, IZMIR), None);
    }

    #[test]
    fn a_flight_that_starts_in_the_menu_position_is_kept() {
        let mut usage = Usage::new(&COLUMNS);
        usage.see(&Telemetry::Frame(vec![0.0, 0.0, 90.0]));
        for t in 1..=60 {
            usage.see(&Telemetry::Frame(vec![f64::from(t), 38.0, 27.0]));
        }
        assert_eq!(usage.verdict(), None);
    }

    #[test]
    fn no_position_at_all_is_dropped() {
        let mut usage = Usage::new(&COLUMNS);
        for t in 0..=30 {
            usage.see(&Telemetry::Frame(vec![f64::from(t), f64::NAN, f64::NAN]));
        }
        assert_eq!(usage.verdict(), Some(Discard::NeverPositioned));
    }
}
