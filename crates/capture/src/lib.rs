//! The aircraft segment on the live simulator: records every MSFS flight,
//! like a QAR that runs whenever the aircraft is powered. Each flight gets
//! a journal (`.fqj`), compacted to `.fqz` once the flight closes, and on
//! request a raw capture (`.raw.jsonl.gz`), including the
//! aircraft's identity and the sim's own runway data for the airports it
//! departs from and lands at. `record` covers one sim session (`foqa-rec`);
//! `Service` keeps doing it across sim restarts for the desktop app.

mod history;
mod live;
mod monitor;
mod panel;
mod place;
mod service;
mod session;
mod views;

use std::fs::File;
use std::io::{self, BufWriter};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use fdm_core::{Airport, Lfl};
use recorder::{
    Acquisition, AttemptMarker, Column, RawWriter, Recorder, SimEvent, Telemetry, TelemetrySource,
};

pub use live::LiveSource;
pub use place::{ASK_TIMEOUT, Ask, InitPosition, Start, flaps_set_data};
pub use service::{Happening, Service, SimRequests, Stopper};
use session::{Action, Session};
pub use views::{
    AppCommand, AttemptStrip, BriefingMsg, ChallengeCard, Landed, Local, PreflightLine, SiteStrip,
    Views,
};

/// What `record` reports as it goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Progress<'a> {
    /// A flight opened, recording to this journal.
    Opened(&'a Path),
    /// An item was recorded into the open flight; `touchdown` when it is a
    /// frame that put the aircraft on the ground from the air.
    Recorded {
        item: &'a Telemetry,
        touchdown: bool,
    },
    /// A flight closed; its journal (and raw capture) are complete.
    Saved(&'a Path),
    /// A frame passed with no flight open: after one was ended, or in the menu.
    Idle,
}

/// What the caller of `record` asks of it, in answer to a progress report.
#[derive(Debug, Clone)]
pub enum Command {
    /// Close the open flight now, as the return to the main menu does
    /// (reported as `Saved`); the next one opens at the next flight start.
    Close,
    /// Record a pilot report with this note into the open flight.
    Mark(String),
    /// Record an attempt marker into the open flight; dropped with none
    /// open, so it never lands in the next one.
    Attempt(AttemptMarker),
    /// Open a flight at the next frame, as a flight start does: to fly on
    /// after ending or discarding one without leaving to the menu.
    Start,
    /// Put the user aircraft at a start and configure it; answered once
    /// the calls are sent.
    Place(Ask<Start, ()>),
    /// Fetch an airport by ICAO; answered when it arrives or fails.
    Airport(Ask<String, Airport>),
}

impl Command {
    /// Answers a request that will not be carried out.
    fn refuse(self, reason: &str) {
        match self {
            Self::Place(ask) => ask.answer(Err(reason.into())),
            Self::Airport(ask) => ask.answer(Err(reason.into())),
            Self::Close | Self::Mark(_) | Self::Attempt(_) | Self::Start => {}
        }
    }
}

/// `Break` stops recording; a command is carried out before the next item.
/// One command per report: a second one before it is carried out is lost.
pub type Reply = ControlFlow<(), Option<Command>>;

/// Records the flights of one sim session into `dir` until the sim quits or
/// `on` breaks; a flight still open is then closed properly. `raw` also
/// keeps each flight's raw capture, about twice the journal's size.
pub fn record(
    source: &mut LiveSource,
    acquisition: &Acquisition,
    dir: &Path,
    raw: bool,
    mut on: impl FnMut(Progress) -> Reply,
) -> io::Result<()> {
    let mut session = Session::default();
    let mut held = Vec::new();
    let mut flight: Option<FlightFiles> = None;
    let (mut running, mut command) = (true, None);
    while running {
        // A command goes through the session like the sim's own items.
        let item = match command.take() {
            Some(Command::Close) => None,
            Some(Command::Start) => {
                session.start();
                continue;
            }
            Some(Command::Mark(note)) => Some(Telemetry::Event(SimEvent::PilotReport(note))),
            Some(Command::Attempt(m)) if flight.is_some() => {
                Some(Telemetry::Event(SimEvent::Attempt(m)))
            }
            Some(Command::Attempt(_)) => continue,
            Some(Command::Place(ask)) => {
                // Queued behind a long flight close: the app may have given up.
                if let Some(ask) = ask.live(Instant::now()) {
                    let placed = source.place(&ask.query).map_err(|e| e.to_string());
                    ask.answer(placed);
                }
                continue;
            }
            Some(Command::Airport(ask)) => {
                source.want_airport(ask);
                continue;
            }
            None => match source.recv()? {
                Some(item) => Some(item),
                None => break,
            },
        };
        let action = match &item {
            Some(item) => session.route(item),
            None => session.end(),
        };
        let touchdown = source.track(item.as_ref(), action);
        let mut reply = |r: Reply| match r {
            ControlFlow::Break(()) => running = false,
            ControlFlow::Continue(c) => command = command.take().or(c),
        };
        match (action, &item) {
            (Action::Hold, Some(item)) => held.push(item.clone()),
            (Action::Open, Some(item)) => {
                let mut f = FlightFiles::create(dir, raw.then(|| acquisition.columns()))?;
                for h in held.drain(..) {
                    f.record(&h)?;
                }
                f.record(item)?;
                reply(on(Progress::Opened(&f.journal)));
                flight = Some(f);
            }
            (Action::Record, Some(item)) => flight.as_mut().ok_or_else(no_flight)?.record(item)?,
            (Action::Close, _) => {
                let mut f = flight.take().ok_or_else(no_flight)?;
                if let Some(item) = &item {
                    f.record(item)?;
                }
                reply(on(Progress::Saved(&f.finish()?)));
            }
            _ => {}
        }
        match (action, &item) {
            (Action::Open | Action::Record, Some(item)) => {
                reply(on(Progress::Recorded { item, touchdown }))
            }
            (Action::Skip, Some(Telemetry::Frame(_))) if flight.is_none() => {
                reply(on(Progress::Idle))
            }
            _ => {}
        }
    }
    if let Some(f) = flight {
        // Recording ends here either way.
        let _ = on(Progress::Saved(&f.finish()?));
    }
    Ok(())
}

fn no_flight() -> io::Error {
    io::Error::other("session routed to a flight that is not open")
}

/// One flight's files: the journal, as the QAR records it, and optionally
/// the raw capture, every sim frame for replay through a changed on-board
/// chain.
struct FlightFiles {
    journal: PathBuf,
    recorder: Recorder<BufWriter<File>>,
    raw: Option<RawWriter>,
    frames: u64,
}

impl FlightFiles {
    /// Named by the wall-clock start, `flight-<unix seconds>`, which the
    /// debrief reads back as the start time.
    fn create(dir: &Path, raw_columns: Option<&[Column]>) -> io::Result<Self> {
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let name = format!("flight-{started}");
        let journal = dir.join(format!("{name}.fqj"));
        let flight = Self {
            recorder: Recorder::new(&Lfl::V1, BufWriter::new(File::create(&journal)?))?,
            raw: raw_columns
                .map(|columns| RawWriter::create(dir.join(format!("{name}.raw.jsonl.gz")), columns))
                .transpose()?,
            frames: 0,
            journal,
        };
        eprintln!("flight started: {name}");
        Ok(flight)
    }

    fn record(&mut self, item: &Telemetry) -> io::Result<()> {
        if let Some(raw) = &mut self.raw {
            raw.write(item)?;
        }
        self.recorder.record(item)?;
        if matches!(item, Telemetry::Frame(_)) {
            self.frames += 1;
        }
        Ok(())
    }

    /// Completes the raw capture (the journal is flushed per item) and
    /// returns the journal's path.
    fn finish(self) -> io::Result<PathBuf> {
        if let Some(raw) = self.raw {
            raw.finish()?;
        }
        eprintln!(
            "flight saved: {} ({} frames)",
            self.journal.display(),
            self.frames
        );
        Ok(self.journal)
    }
}
