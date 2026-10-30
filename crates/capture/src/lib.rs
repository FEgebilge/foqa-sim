//! The aircraft segment on the live simulator: records every MSFS flight,
//! like a QAR that runs whenever the aircraft is powered. Each flight gets
//! a journal (`.fqj`), compacted to `.fqz` once the flight closes, and on
//! request a raw capture (`.raw.jsonl.gz`), including the
//! aircraft's identity and the sim's own runway data for the airports it
//! departs from and lands at. `record` covers one sim session (`foqa-rec`);
//! `Service` keeps doing it across sim restarts for the desktop app.

mod discard;
mod history;
mod live;
mod monitor;
mod panel;
mod place;
mod service;
mod session;
mod views;

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufWriter};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fdm_core::{Airport, Lfl};
use recorder::{Acquisition, AttemptMarker, Column, RawWriter, Recorder, SimEvent, Telemetry};

use discard::Usage;
pub use discard::{Discard, MENU_POSITION, MENU_TOLERANCE_DEG, MIN_FLIGHT_S, backlog, recover};
pub use live::LiveSource;
pub use monitor::Impact;
pub use place::{
    ASK_TIMEOUT, Ask, Ils, IlsQuery, InitPosition, RELOAD_TIMEOUT, Reloaded, Settled, Start,
    flaps_set_data,
};
use place::{ReloadWait, SAVE_AFTER};
pub use rules::Strike;
pub use service::{Happening, Service, SimRequests, Stopper};
use session::{Action, Session};
pub use views::{
    AppCommand, AttemptStrip, BriefingMsg, ChallengeCard, Landed, Local, PreflightLine, SiteStrip,
    Views,
};

/// What `record` reports as it goes.
#[derive(Debug, Clone, Copy)]
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
    /// A recording closed that was not a flight (`Discard`): its files are
    /// deleted and the path is only its name.
    Discarded(&'a Path, Discard),
    /// A frame passed with no flight open: after one was ended, or in the menu.
    Idle,
    /// Nothing came from the sim for a while: paused, or loading.
    Tick,
    /// The open flight's files could not be written (disk full, folder
    /// locked): it was closed with what it has (`Saved` or `Discarded`
    /// came first), and none opens for a while.
    Failed(&'a io::Error),
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
    /// Put the user aircraft at a start and configure it. The flight open
    /// closes there and a new one opens once the placement took
    /// (`Session::place`); answered with that flight's id.
    Place(Ask<Start, String>),
    /// Fetch an airport by ICAO; answered when it arrives or fails.
    Airport(Ask<String, Airport>),
    /// Find a runway end's ILS; answered when found or failed.
    Ils(Ask<IlsQuery, Option<Ils>>),
    /// Save the flight to this path (no extension: the sim adds `.FLT`),
    /// `SAVE_AFTER` past the last placement; answered once the call is sent.
    Save(Ask<PathBuf, ()>),
    /// Load the flight saved at this path and wait for the sim to settle
    /// (`ReloadWait`); answered once it has and a flight is open.
    Reload(Ask<PathBuf, Reloaded>),
}

impl Command {
    /// Answers a request that will not be carried out.
    fn refuse(self, reason: &str) {
        match self {
            Self::Place(ask) => ask.answer(Err(reason.into())),
            Self::Airport(ask) => ask.answer(Err(reason.into())),
            Self::Ils(ask) => ask.answer(Err(reason.into())),
            Self::Save(ask) => ask.answer(Err(reason.into())),
            Self::Reload(ask) => ask.answer(Err(reason.into())),
            Self::Close | Self::Mark(_) | Self::Attempt(_) | Self::Start => {}
        }
    }
}

/// `Break` stops recording; a command is carried out before the next item,
/// in the order they came when several did.
pub type Reply = ControlFlow<(), Option<Command>>;

/// What one poll of the simulator gives.
#[derive(Debug)]
pub(crate) enum Polled {
    Item(Telemetry),
    /// The sim's state: flying (`true`) or in the menus.
    State(bool),
    /// Nothing arrived for a while.
    Tick,
    /// The sim quit, or its connection is gone.
    Ended,
}

/// The simulator as `record` uses it; `LiveSource` is the real one.
pub(crate) trait Sim {
    fn poll(&mut self) -> Polled;
    fn place(&mut self, start: &Start) -> Result<(), String>;
    fn want_airport(&mut self, ask: Ask<String, Airport>);
    fn want_ils(&mut self, ask: Ask<IlsQuery, Option<Ils>>);
    /// Saves the flight to `path`; `Ok` once the call is sent.
    fn save(&mut self, path: &Path) -> Result<(), String>;
    /// Loads the flight saved at `path`; `Ok` once the call is sent.
    fn load(&mut self, path: &Path) -> Result<(), String>;
    /// Follows the session's routing of `item`; true for a touchdown frame.
    fn track(&mut self, item: Option<&Telemetry>, action: Action) -> bool;
}

/// After a write failure no flight opens for this long, doubling with each
/// failure in a row: a full disk or a locked folder does not clear in
/// seconds, and each retry starts another file.
const WRITE_RETRY_MIN: Duration = Duration::from_secs(10);
const WRITE_RETRY_MAX: Duration = Duration::from_secs(5 * 60);
/// How long after the app places the aircraft a teleport is its own; the
/// sim reports one within a frame or two, if at all. A placement that has
/// opened no flight by then is answered with an error.
const OWN_TELEPORT: Duration = Duration::from_secs(10);
/// Without the sim's `TeleportDone`, the placement is taken to have taken
/// this long after it: `InitPosition` applies at the next sim frame, and a
/// second drains the frames of the old position already on their way.
/// It costs at most this much of the approach's start, never a frame of
/// where the aircraft was.
const PLACE_SETTLE: Duration = Duration::from_secs(1);
/// How often the open flight's journal is flushed to its file, about what
/// a killed process loses (the raw capture makes the same trade, `raw.rs`):
/// one write a second instead of one each frame. Events are flushed at
/// once, as is a quiet journal (paused) and a closing one.
const FLUSH_EVERY: Duration = Duration::from_secs(1);
/// Names tried for a new flight, one second apart, before giving up.
const NAME_TRIES: u64 = 60;
/// The raw capture's file name ending, as `debrief` lists it.
const RAW_SUFFIX: &str = ".raw.jsonl.gz";

/// Records the flights of one sim session into `dir` until the sim quits or
/// its connection is lost, or `on` breaks; a flight still open is then
/// closed properly. `raw` also keeps each flight's raw capture, about twice
/// the journal's size. A flight whose files cannot be written is closed with
/// what it has and reported as `Failed`; recording carries on after a wait.
pub fn record(
    source: &mut LiveSource,
    acquisition: &Acquisition,
    dir: &Path,
    raw: bool,
    on: impl FnMut(Progress) -> Reply,
) {
    run(source, acquisition.columns(), dir, raw, on);
}

fn run(
    source: &mut impl Sim,
    columns: &[Column],
    dir: &Path,
    raw: bool,
    on: impl FnMut(Progress) -> Reply,
) {
    let mut r = Run {
        columns,
        dir,
        raw,
        on,
        session: Session::default(),
        held: Vec::new(),
        flight: None,
        replies: Replies::default(),
        retry: Retry::default(),
        misframed: false,
    };
    let mut placed: Option<Instant> = None;
    let mut placing: Option<Ask<Start, String>> = None;
    let mut saving: Option<Ask<PathBuf, ()>> = None;
    let mut reloading: Option<(Ask<PathBuf, Reloaded>, ReloadWait)> = None;
    while !r.replies.stop {
        let now = Instant::now();
        if r.retry.over(now) {
            r.session.resume();
        }
        if placed.is_some_and(|t| now - t >= PLACE_SETTLE) {
            r.session.settled();
        }
        if placed.is_some_and(|t| now - t > OWN_TELEPORT) {
            placed = None;
            r.session.own_teleport_over();
        }
        // The flight opened after the placement is where the attempt goes.
        placing = placing.and_then(|ask| {
            match (&r.flight, placed) {
                (Some(f), _) if !r.session.is_placing() => {
                    ask.answer(Ok(debrief::flight_id(&f.journal)))
                }
                (_, None) => ask.answer(Err("no flight opened after the placement".into())),
                _ => return Some(ask),
            }
            None
        });
        if placed.is_none_or(|t| now - t >= SAVE_AFTER)
            && let Some(ask) = saving.take()
        {
            let result = source.save(&ask.query);
            ask.answer(result);
        }
        reloading = reloading.and_then(|(ask, wait)| {
            // Settled, it waits for a flight to open: the start goes there.
            let open = r.flight.as_ref().map(|f| debrief::flight_id(&f.journal));
            match (wait.state(now), open) {
                (Some(Ok(_)), None) if wait.expired(now) => {
                    ask.answer(Err("no flight opened after the reload".into()))
                }
                (None, _) | (Some(Ok(_)), None) => return Some((ask, wait)),
                (Some(Ok(settled)), Some(flight_id)) => {
                    ask.answer(Ok(Reloaded { flight_id, settled }))
                }
                (Some(Err(e)), _) => ask.answer(Err(e)),
            }
            None
        });
        // A command goes through the session like the sim's own items.
        let item = match r.replies.commands.pop_front() {
            Some(Command::Close) => None,
            Some(Command::Start) => {
                r.session.start();
                continue;
            }
            Some(Command::Mark(note)) => Some(Telemetry::Event(SimEvent::PilotReport(note))),
            Some(Command::Attempt(m)) if r.flight.is_some() => {
                Some(Telemetry::Event(SimEvent::Attempt(m)))
            }
            Some(Command::Attempt(_)) => continue,
            Some(Command::Place(ask)) => {
                // Queued behind a long flight close: the app may have given up.
                let Some(ask) = ask.live(now) else { continue };
                if let Err(e) = source.place(&ask.query) {
                    ask.answer(Err(e));
                    continue;
                }
                placed = Some(now);
                if let Some(older) = placing.replace(ask) {
                    older.answer(Err("another placement replaced it".into()));
                }
                // The flight so far ends where the aircraft was.
                let action = r.session.place();
                source.track(None, action);
                if let Err(e) = r.write(action, None, false) {
                    r.failed(&e, now);
                }
                continue;
            }
            Some(Command::Airport(ask)) => {
                source.want_airport(ask);
                continue;
            }
            Some(Command::Ils(ask)) => {
                source.want_ils(ask);
                continue;
            }
            Some(Command::Save(ask)) => {
                if let Some(ask) = ask.live(now)
                    && let Some(older) = saving.replace(ask)
                {
                    older.answer(Err("another save replaced it".into()));
                }
                continue;
            }
            Some(Command::Reload(ask)) => {
                let Some(ask) = ask.live(now) else { continue };
                if reloading.is_some() {
                    ask.answer(Err("a reload is already under way".into()));
                } else {
                    match source.load(&ask.query) {
                        Ok(()) => reloading = Some((ask, ReloadWait::new(now))),
                        Err(e) => ask.answer(Err(e)),
                    }
                }
                continue;
            }
            None => match source.poll() {
                Polled::Item(item) => match r.framed(item) {
                    Some(item) => Some(item),
                    None => continue,
                },
                Polled::State(flying) => {
                    r.session.in_control(flying);
                    continue;
                }
                Polled::Tick => {
                    // Nothing more may come for long: paused, or about to crash.
                    if let Some(Err(e)) = r.flight.as_mut().map(FlightFiles::flush) {
                        r.failed(&e, now);
                        source.track(None, Action::Close);
                    }
                    r.report(Progress::Tick);
                    continue;
                }
                Polled::Ended => break,
            },
        };
        if let (Some((_, wait)), Some(item)) = (&mut reloading, &item) {
            wait.see(item, now);
        }
        let action = match &item {
            Some(item) => r.session.route(item),
            None => r.session.end(),
        };
        let touchdown = source.track(item.as_ref(), action);
        if let Err(e) = r.write(action, item.as_ref(), touchdown) {
            r.failed(&e, now);
            source.track(None, Action::Close);
        }
    }
    // Recording ends here either way.
    r.close();
}

/// One `record` call's state.
struct Run<'a, F> {
    columns: &'a [Column],
    dir: &'a Path,
    raw: bool,
    on: F,
    session: Session,
    held: Vec<Telemetry>,
    flight: Option<FlightFiles>,
    replies: Replies,
    retry: Retry,
    /// A frame of the wrong length was reported.
    misframed: bool,
}

impl<F: FnMut(Progress) -> Reply> Run<'_, F> {
    fn report(&mut self, progress: Progress) {
        let reply = (self.on)(progress);
        self.replies.take(reply);
    }

    /// `item`, unless it is a frame whose length is not the columns': its
    /// values would land in the wrong signals (a SimVar dropped from the
    /// definition shifts every one after it).
    fn framed(&mut self, item: Telemetry) -> Option<Telemetry> {
        if let Telemetry::Frame(frame) = &item
            && frame.len() != self.columns.len()
        {
            if !std::mem::replace(&mut self.misframed, true) {
                eprintln!(
                    "error: sim frames of {} values for {} columns are dropped",
                    frame.len(),
                    self.columns.len()
                );
            }
            return None;
        }
        Some(item)
    }

    /// Files `item` as the session routed it, then reports it.
    fn write(
        &mut self,
        action: Action,
        item: Option<&Telemetry>,
        touchdown: bool,
    ) -> io::Result<()> {
        match (action, item) {
            (Action::Hold, Some(item)) => self.held.push(item.clone()),
            (Action::Open, Some(item)) => {
                let f = self
                    .flight
                    .insert(FlightFiles::create(self.dir, self.columns, self.raw)?);
                let reply = (self.on)(Progress::Opened(&f.journal));
                self.replies.take(reply);
                for h in self.held.drain(..) {
                    f.record(&h)?;
                }
                f.record(item)?;
            }
            (Action::Record, Some(item)) => {
                self.flight.as_mut().ok_or_else(no_flight)?.record(item)?
            }
            (Action::Close, _) => {
                if let Some(item) = item {
                    self.flight.as_mut().ok_or_else(no_flight)?.record(item)?;
                }
                self.close();
                self.retry = Retry::default();
            }
            (Action::Split, Some(item)) => {
                self.close();
                self.held.push(item.clone());
            }
            _ => {}
        }
        match (action, item) {
            (Action::Open | Action::Record, Some(item)) => {
                self.report(Progress::Recorded { item, touchdown })
            }
            (Action::Skip, Some(Telemetry::Frame(_))) if self.flight.is_none() => {
                self.report(Progress::Idle)
            }
            _ => {}
        }
        Ok(())
    }

    /// Closes the open flight, if any.
    fn close(&mut self) {
        if let Some(f) = self.flight.take() {
            let reply = f.finish_with(&mut self.on);
            self.replies.take(reply);
        }
    }

    /// The flight's files could not be written: it is closed with what it
    /// has, and no flight opens until the retry wait is over.
    fn failed(&mut self, e: &io::Error, now: Instant) {
        eprintln!("error: cannot write to {}: {e}", self.dir.display());
        self.close();
        self.session.fail();
        self.retry.fail(now);
        self.report(Progress::Failed(e));
    }
}

/// What the replies to `record`'s reports ask for. One item can take
/// several reports (a flight opening is `Opened` then `Recorded`), each
/// with a command.
#[derive(Default)]
struct Replies {
    stop: bool,
    commands: VecDeque<Command>,
}

impl Replies {
    fn take(&mut self, reply: Reply) {
        match reply {
            ControlFlow::Break(()) => self.stop = true,
            ControlFlow::Continue(c) => self.commands.extend(c),
        }
    }
}

/// When a flight may open again after write failures.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Retry {
    wait: Duration,
    until: Option<Instant>,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            wait: WRITE_RETRY_MIN,
            until: None,
        }
    }
}

impl Retry {
    fn fail(&mut self, now: Instant) {
        self.until = Some(now + self.wait);
        self.wait = (self.wait * 2).min(WRITE_RETRY_MAX);
    }

    /// True once, when the wait after a failure is over.
    fn over(&mut self, now: Instant) -> bool {
        self.until.take_if(|until| now >= *until).is_some()
    }
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
    raw_path: PathBuf,
    frames: u64,
    usage: Usage,
    next_flush: Instant,
}

impl FlightFiles {
    /// Named by the wall-clock start, `flight-<unix seconds>`, which the
    /// debrief reads back as the start time. `columns` are the frames'.
    fn create(dir: &Path, columns: &[Column], raw: bool) -> io::Result<Self> {
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let (name, journal, file) = new_journal(dir, started)?;
        let raw_path = dir.join(format!("{name}{RAW_SUFFIX}"));
        let flight = Self {
            recorder: Recorder::new(&Lfl::V1, BufWriter::new(file))?,
            raw: raw
                .then(|| RawWriter::create(raw_path.clone(), columns))
                .transpose()?,
            raw_path,
            frames: 0,
            usage: Usage::new(columns),
            next_flush: Instant::now() + FLUSH_EVERY,
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
        self.usage.see(item);
        if matches!(item, Telemetry::Event(_)) || Instant::now() >= self.next_flush {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.next_flush = Instant::now() + FLUSH_EVERY;
        self.recorder.flush()
    }

    /// Completes the journal and the raw capture, then reports the flight
    /// as `Saved`, or deletes its files and reports `Discarded` if it is not
    /// a flight (`Usage::verdict`). A raw capture that fails to complete
    /// does not hold back its journal, nor a journal that fails to flush
    /// what it has: up to its last intact record it reads back.
    fn finish_with(self, on: &mut impl FnMut(Progress) -> Reply) -> Reply {
        let Self {
            journal,
            mut recorder,
            raw,
            raw_path,
            frames,
            usage,
            next_flush: _,
        } = self;
        if let Err(e) = recorder.flush() {
            eprintln!("{}: {e}", journal.display());
        }
        drop(recorder);
        if let Some(Err(e)) = raw.map(RawWriter::finish) {
            eprintln!("{}: {e}", raw_path.display());
        }
        let Some(why) = usage.verdict() else {
            eprintln!("flight saved: {} ({frames} frames)", journal.display());
            return on(Progress::Saved(&journal));
        };
        for file in [&journal, &raw_path] {
            match std::fs::remove_file(file) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => {
                    eprintln!("{}: could not delete: {e}", file.display())
                }
                _ => {}
            }
        }
        eprintln!(
            "info: recording discarded, {why}: {} ({frames} frames)",
            journal.display()
        );
        on(Progress::Discarded(&journal, why))
    }
}

/// Creates a new flight's journal, `flight-<started>.fqj`, never over the
/// files of another: a taken name moves on a second, so it still reads back
/// as about the start time (two flights in one second, a clock set back).
fn new_journal(dir: &Path, started: u64) -> io::Result<(String, PathBuf, File)> {
    for secs in started..started + NAME_TRIES {
        let name = format!("flight-{secs}");
        let journal = dir.join(format!("{name}.fqj"));
        if debrief::flight_path(dir, &name).is_some()
            || dir.join(format!("{name}{RAW_SUFFIX}")).exists()
        {
            continue;
        }
        match File::create_new(&journal) {
            Ok(file) => return Ok((name, journal, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("no free flight name after flight-{started}"),
    ))
}

#[cfg(test)]
mod tests {
    use recorder::{JournalReader, Record};

    use super::*;

    const IZMIR: (f64, f64) = (38.29, 27.16);

    fn columns() -> Vec<Column> {
        Acquisition::new(&Lfl::V1).columns().to_vec()
    }

    /// A frame at sim time `t` and `(lat, lon)`, every other column `NaN`.
    fn frame(t: f64, (lat, lon): (f64, f64)) -> Telemetry {
        let columns = columns();
        let at = |name| columns.iter().position(|c| c.simvar == name).unwrap();
        let mut frame = vec![f64::NAN; columns.len()];
        frame[at("SIMULATION TIME")] = 100.0 + t;
        frame[at("PLANE LATITUDE")] = lat;
        frame[at("PLANE LONGITUDE")] = lon;
        Telemetry::Frame(frame)
    }

    /// 1 Hz frames at `pos` over sim seconds `from..=to`.
    fn frames(from: u32, to: u32, pos: (f64, f64)) -> impl Iterator<Item = Polled> {
        (from..=to).map(move |t| Polled::Item(frame(f64::from(t), pos)))
    }

    /// A sim that gives these polls, then ends as a lost connection does.
    struct Script(VecDeque<Polled>);

    impl Sim for Script {
        fn poll(&mut self) -> Polled {
            self.0.pop_front().unwrap_or(Polled::Ended)
        }
        fn place(&mut self, _: &Start) -> Result<(), String> {
            Ok(())
        }
        fn want_airport(&mut self, _: Ask<String, Airport>) {}
        fn want_ils(&mut self, _: Ask<IlsQuery, Option<Ils>>) {}
        fn save(&mut self, _: &Path) -> Result<(), String> {
            Ok(())
        }
        fn load(&mut self, _: &Path) -> Result<(), String> {
            Ok(())
        }
        fn track(&mut self, _: Option<&Telemetry>, _: Action) -> bool {
            false
        }
    }

    /// Records `polls` into `dir`, `on` answering each report; returns the
    /// reports, a run of one counted (`recorded x31`).
    fn script(
        dir: &Path,
        polls: impl IntoIterator<Item = Polled>,
        mut on: impl FnMut(&Progress) -> Reply,
    ) -> Vec<String> {
        let mut reports: Vec<(String, u32)> = Vec::new();
        let mut sim = Script(polls.into_iter().collect());
        run(&mut sim, &columns(), dir, false, |p| {
            let report = match &p {
                Progress::Opened(_) => "opened".into(),
                Progress::Recorded { .. } => "recorded".into(),
                Progress::Saved(journal) => format!("saved {}", debrief::flight_id(journal)),
                Progress::Discarded(..) => "discarded".into(),
                Progress::Idle => "idle".into(),
                Progress::Tick => "tick".into(),
                Progress::Failed(_) => "failed".into(),
            };
            match reports.last_mut() {
                Some((last, n)) if *last == report => *n += 1,
                _ => reports.push((report, 1)),
            }
            on(&p)
        });
        let counted = |(r, n): (String, u32)| if n > 1 { format!("{r} x{n}") } else { r };
        reports.into_iter().map(counted).collect()
    }

    fn carry_on(_: &Progress) -> Reply {
        ControlFlow::Continue(None)
    }

    /// Records `seconds` of 1 Hz frames at `pos` into a new journal in
    /// `dir`, closes it and returns what it was reported as.
    fn close(dir: &Path, seconds: u32, pos: (f64, f64)) -> (String, PathBuf) {
        let mut flight = FlightFiles::create(dir, &columns(), false).unwrap();
        let journal = flight.journal.clone();
        for t in 0..=seconds {
            flight.record(&frame(f64::from(t), pos)).unwrap();
        }
        let mut reported = String::new();
        let _ = flight.finish_with(&mut |p| {
            reported = match p {
                Progress::Saved(_) => "saved".into(),
                Progress::Discarded(_, why) => format!("discarded: {why}"),
                _ => unreachable!(),
            };
            ControlFlow::Continue(None)
        });
        (reported, journal)
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("foqa-capture-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_menu_recording_is_deleted_when_it_closes() {
        let dir = temp("menu");
        let (reported, journal) = close(&dir, 6, MENU_POSITION);
        assert!(reported.starts_with("discarded"), "{reported}");
        assert!(!journal.exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_ground_session_at_an_airport_is_kept() {
        let dir = temp("taxi");
        let (reported, journal) = close(&dir, 30, IZMIR);
        assert_eq!(reported, "saved");
        assert!(journal.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lost_sim_saves_the_open_flight() {
        let dir = temp("lost");
        let reports = script(&dir, frames(0, 30, IZMIR), carry_on);
        assert_eq!(reports[..2], ["opened", "recorded x31"]);
        assert!(reports[2].starts_with("saved flight-"), "{reports:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stop_while_nothing_arrives_closes_the_flight() {
        let dir = temp("pause");
        let polls = frames(0, 30, IZMIR).chain((0..100).map(|_| Polled::Tick));
        let reports = script(&dir, polls, |p| match p {
            Progress::Tick => ControlFlow::Break(()),
            _ => ControlFlow::Continue(None),
        });
        assert_eq!(reports[..3], ["opened", "recorded x31", "tick"]);
        assert!(reports[3].starts_with("saved"), "{reports:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_frame_of_the_wrong_length_is_dropped() {
        let dir = temp("short");
        let polls = frames(0, 5, IZMIR)
            .chain([Polled::Item(Telemetry::Frame(vec![106.0, 1.0]))])
            .chain(frames(7, 30, IZMIR));
        let reports = script(&dir, polls, carry_on);
        assert_eq!(reports[..2], ["opened", "recorded x30"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unwritable_folder_fails_once_then_waits() {
        let dir = temp("unwritable").join("gone");
        let reports = script(&dir, frames(0, 30, IZMIR), carry_on);
        assert_eq!(reports, ["failed", "idle x30"]);
    }

    #[test]
    fn an_aircraft_swap_saves_two_flights_of_one_aircraft_each() {
        let dir = temp("swap");
        let aircraft = |icao| Polled::Item(Telemetry::Event(testkit::identity(icao)));
        let polls = [aircraft("C172")]
            .into_iter()
            .chain(frames(0, 30, IZMIR))
            .chain([aircraft("A20N")])
            .chain(frames(31, 61, IZMIR));
        let reports = script(&dir, polls, carry_on);
        let saved: Vec<_> = reports.iter().filter(|r| r.starts_with("saved")).collect();
        assert_eq!(saved.len(), 2, "{reports:?}");
        assert_ne!(saved[0], saved[1]);
        for (report, icao) in saved.iter().zip(["C172", "A20N"]) {
            let id = report.trim_start_matches("saved ");
            let file = File::open(dir.join(format!("{id}.fqj"))).unwrap();
            let titles: Vec<_> = JournalReader::new(file)
                .unwrap()
                .filter_map(|r| match r.unwrap() {
                    Record::Event(_, SimEvent::Aircraft(a)) => Some(a.title),
                    _ => None,
                })
                .collect();
            assert_eq!(titles, [format!("{icao} test")]);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn commands_answering_two_reports_of_one_item_are_both_carried_out() {
        let dir = temp("two-commands");
        let mut opened = None;
        let mut marks = ["opened", "recorded"].into_iter();
        script(&dir, frames(0, 30, IZMIR), |p| {
            if let Progress::Opened(journal) = p {
                opened = Some(journal.to_path_buf());
            }
            ControlFlow::Continue(marks.next().map(|m| Command::Mark(m.into())))
        });
        let file = File::open(opened.unwrap()).unwrap();
        let notes: Vec<_> = JournalReader::new(file)
            .unwrap()
            .filter_map(|r| match r.unwrap() {
                Record::Event(_, SimEvent::PilotReport(note)) => Some(note),
                _ => None,
            })
            .collect();
        assert_eq!(notes, ["opened", "recorded"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reload_answers_with_the_flight_opened_after_the_sim_settled() {
        use recorder::FlowEvent::*;
        let dir = temp("reload");
        let flow = |f| Polled::Item(Telemetry::Event(SimEvent::Flow(f)));
        let sim_start = Polled::Item(Telemetry::Event(SimEvent::SimStart));
        // The bench's sequence: the load, ~10 s airborne, the sim's own
        // move to the runway, then the runway.
        let polls = frames(0, 30, IZMIR)
            .chain([flow(FlightEnd), flow(FltLoad), flow(FltLoaded)])
            .chain([flow(TeleportStart), flow(TeleportDone), sim_start])
            .chain(frames(31, 40, IZMIR))
            .chain([flow(FlightStart), flow(FltLoad), flow(FltLoaded)])
            .chain([flow(RtcStart), flow(TeleportStart), flow(TeleportDone)])
            .chain(frames(41, 42, IZMIR))
            .chain([flow(RtcEnd)])
            .chain(frames(43, 60, IZMIR));
        let (mut opened, mut answer, mut asked) = (Vec::new(), None, false);
        script(&dir, polls, |p| {
            if let Progress::Opened(journal) = p {
                opened.push(debrief::flight_id(journal));
            }
            if asked {
                return ControlFlow::Continue(None);
            }
            asked = true;
            let (ask, rx) = Ask::new(dir.join("clean"));
            answer = Some(rx);
            ControlFlow::Continue(Some(Command::Reload(ask)))
        });
        let reloaded = answer.unwrap().try_recv().unwrap().unwrap();
        assert_eq!(opened.len(), 3, "{opened:?}");
        // Not the flight it was asked in, nor the one before the move.
        assert_eq!(reloaded.flight_id, opened[2]);
        assert_eq!(reloaded.settled, Settled::RunwayMove);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_placement_answers_with_the_flight_opened_after_its_teleport() {
        use recorder::FlowEvent::*;
        let dir = temp("place");
        let flow = |f| Polled::Item(Telemetry::Event(SimEvent::Flow(f)));
        const PLACED: (f64, f64) = (38.4, 27.1);
        let start = Start {
            pos: InitPosition {
                latitude_deg: PLACED.0,
                longitude_deg: PLACED.1,
                altitude_ft: 1500.0,
                pitch_deg: 0.0,
                bank_deg: 0.0,
                heading_deg: 340.0,
                on_ground: 0,
                airspeed_kt: 70,
            },
            gear_down: true,
            flaps: None,
            nav1_hz: None,
        };
        // On the runway for 30 s, placed, a frame of the old position still
        // on its way, the sim's teleport, then the start.
        let polls = frames(0, 31, IZMIR)
            .chain([flow(TeleportStart), flow(TeleportDone)])
            .chain(frames(32, 60, PLACED));
        let (mut opened, mut saved, mut answer, mut recorded) = (vec![], vec![], None, 0);
        script(&dir, polls, |p| {
            match p {
                Progress::Opened(journal) => opened.push(debrief::flight_id(journal)),
                Progress::Saved(journal) => saved.push(debrief::flight_id(journal)),
                Progress::Recorded { .. } => recorded += 1,
                _ => {}
            }
            if recorded != 31 || answer.is_some() {
                return ControlFlow::Continue(None);
            }
            let (ask, rx) = Ask::new(start);
            answer = Some(rx);
            ControlFlow::Continue(Some(Command::Place(ask)))
        });
        let placed_in = answer.unwrap().try_recv().unwrap().unwrap();
        assert_eq!(opened.len(), 2, "{opened:?}");
        assert_eq!(placed_in, opened[1]);
        // The part before is a flight of its own; the new one holds only
        // the start, not a frame of where the aircraft was.
        assert_eq!(saved, opened);
        let file = File::open(dir.join(format!("{placed_in}.fqj"))).unwrap();
        let lat = Lfl::V1.by_name("pos.lat").unwrap().id;
        let lats: Vec<f64> = JournalReader::new(file)
            .unwrap()
            .filter_map(|r| match r.unwrap() {
                Record::Sample(s) if s.signal == lat => Some(s.value),
                _ => None,
            })
            .collect();
        assert!(
            !lats.is_empty() && lats.iter().all(|&l| l == PLACED.0),
            "{lats:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_journal_is_flushed_each_second_and_at_events() {
        let dir = temp("flush");
        let mut flight = FlightFiles::create(&dir, &columns(), false).unwrap();
        let journal = flight.journal.clone();
        let on_disk = || std::fs::metadata(&journal).unwrap().len();
        for t in 0..3 {
            flight.record(&frame(f64::from(t), IZMIR)).unwrap();
        }
        assert_eq!(on_disk(), 0);
        let mark = Telemetry::Event(SimEvent::PilotReport("bird".into()));
        flight.record(&mark).unwrap();
        let marked = on_disk();
        assert!(marked > 0);
        flight.record(&frame(3.0, IZMIR)).unwrap();
        assert_eq!(on_disk(), marked);
        // A second later.
        flight.next_flush = Instant::now();
        flight.record(&frame(4.0, IZMIR)).unwrap();
        assert!(on_disk() > marked);
        drop(flight);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_flight_never_takes_an_existing_name() {
        let dir = temp("names");
        std::fs::write(dir.join("flight-101.fqz"), b"").unwrap();
        let names: Vec<_> = (0..3).map(|_| new_journal(&dir, 100).unwrap().0).collect();
        assert_eq!(names, ["flight-100", "flight-102", "flight-103"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn journals_left_open_are_recovered_or_discarded() {
        let dir = temp("backlog");
        // Recorded, never closed: a crash.
        let left_open = |seconds, pos| {
            let mut flight = FlightFiles::create(&dir, &columns(), false).unwrap();
            for t in 0..=seconds {
                flight.record(&frame(f64::from(t), pos)).unwrap();
            }
            flight.journal.clone()
        };
        let flight = left_open(30, IZMIR);
        left_open(30, MENU_POSITION);
        left_open(3, IZMIR);
        let bytes = std::fs::read(&flight).unwrap();
        std::fs::write(&flight, &bytes[..bytes.len() - 3]).unwrap();
        std::fs::write(dir.join("flight-1.fqz.part"), b"quit while compacting").unwrap();
        recover(&dir, backlog(&dir));
        let files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(files, [format!("{}.fqz", debrief::flight_id(&flight))]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_retries_back_off() {
        let (mut retry, now) = (Retry::default(), Instant::now());
        retry.fail(now);
        assert!(!retry.over(now));
        assert!(retry.over(now + WRITE_RETRY_MIN));
        assert!(!retry.over(now + WRITE_RETRY_MIN));
        for _ in 0..10 {
            retry.fail(now);
        }
        assert_eq!(retry.wait, WRITE_RETRY_MAX);
    }
}
