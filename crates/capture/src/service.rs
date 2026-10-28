//! The recorder as a background service: waits for the simulator, records
//! every flight while it runs, and comes back to waiting when it quits, the
//! way a QAR records whenever the aircraft is powered without anyone
//! switching it on. Status and analysed results go out on a channel; the
//! in-sim panel also gets the flight live and can end, discard or mark it.

use std::io;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use debrief::{Analysis, FlightSummary, Index, LandingNotice, RecorderStatus, Station, flight_id};
use fdm_core::{Airport, Lfl};
use recorder::{Acquisition, AttemptMarker, TOUCHDOWN_WINDOW_US, Telemetry};

use crate::history::History;
use crate::monitor::Monitor;
use crate::panel::{self, Ack, Msg, Snapshot, Tee, ViewCommand};
use crate::place::{Ask, Start};
use crate::views::{AppCommand, Views};
use crate::{Command, LiveSource, Progress, Reply, record};

/// First and longest wait between attempts to reach the simulator.
/// `SimConnect_Open` fails at once while MSFS is not running; doubling the
/// wait keeps an idle tray app from polling it many times a second.
const RETRY_MIN: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(15);
/// Sim seconds after the last touchdown of a landing before it is analysed:
/// the recorder's full-rate window after contact, plus time for the
/// touchdown airport snapshot to arrive. A bounce restarts the wait, so one
/// landing gives one notice.
const LANDING_AFTER_S: f64 = TOUCHDOWN_WINDOW_US as f64 / 1e6 + 2.0;
/// The panel's live values, 4 Hz.
const LIVE_EVERY: Duration = Duration::from_millis(250);
/// The open flight analysed for the panel, at most one analysis at a time,
/// and only while a view watches and sim time moves on.
const SNAPSHOT_EVERY: Duration = Duration::from_secs(5);
/// A snapshot analyses the whole journal so far, so its cost grows with the
/// flight: the next waits at least this many times the last one's, which
/// keeps it under 1% of a core (every 100 s once one takes 1 s).
const SNAPSHOT_COST_FACTOR: u32 = 100;

/// What the service reports.
#[derive(Debug, Clone, PartialEq)]
pub enum Happening {
    /// The state changed; while recording, also each second as the
    /// elapsed time grows.
    Status(RecorderStatus),
    /// A flight closed and was analysed.
    FlightSaved(FlightSummary),
    /// A landing, analysed from the journal as recorded so far.
    Landing(LandingNotice),
}

/// Asks a running `Service` to close any open flight and return.
#[derive(Debug, Clone)]
pub struct Stopper {
    stopped: Arc<AtomicBool>,
    /// Wakes a service waiting for the sim: a `Close`, which with no flight
    /// open does nothing.
    wake: Sender<Command>,
}

impl Stopper {
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        let _ = self.wake.send(Command::Close);
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
}

/// Asks a running `Service` to act on the simulator, from any thread. Each
/// call returns where its answer arrives: `Err` with the reason, or a
/// disconnection if the sim quit or the service stopped first.
#[derive(Debug, Clone)]
pub struct SimRequests(Sender<Command>);

impl SimRequests {
    /// Puts the user aircraft at `start` and configures it, at the next sim
    /// frame. `Ok` means the calls went out; a rejection by the sim is only
    /// logged. Into an open flight the jump is recorded like any other.
    pub fn place(&self, start: Start) -> Receiver<Result<(), String>> {
        let (ask, answer) = Ask::new(start);
        self.send(Command::Place(ask));
        answer
    }

    /// The airport `icao` with its runways from the sim's facility database
    /// (`analyzer::runway::thresholds` gives the threshold points). Also
    /// recorded, like the airports captured for landings.
    pub fn airport(&self, icao: &str) -> Receiver<Result<Airport, String>> {
        let (ask, answer) = Ask::new(icao.to_owned());
        self.send(Command::Airport(ask));
        answer
    }

    /// Records an attempt's begin or end marker into the open flight at the
    /// next sim frame (docs/challenge-protocol.md A7); with no flight open
    /// it is dropped.
    pub fn attempt_marker(&self, marker: AttemptMarker) {
        self.send(Command::Attempt(marker));
    }

    fn send(&self, command: Command) {
        // A stopped service drops the request, disconnecting its answer.
        let _ = self.0.send(command);
    }
}

pub struct Service {
    dir: PathBuf,
    station: Arc<Station>,
    index: Arc<Index>,
    stopper: Stopper,
    raw: bool,
    /// An analysis for the panel is running.
    analysing: Arc<AtomicBool>,
    /// The last one's cost, microseconds.
    analysis_us: Arc<AtomicU64>,
    /// A view showed lately that it is open (`panel::tee`).
    watched: Arc<AtomicBool>,
    requests: Sender<Command>,
    /// The app's requests (`SimRequests`): `Place`, `Airport` and `Attempt`.
    requested: Receiver<Command>,
    /// What the in-sim views show, from the recorder and the app (`Views`).
    feed: Sender<Msg>,
    feed_rx: Option<Receiver<Msg>>,
    /// The views' attempt commands, for the app.
    app: Sender<AppCommand>,
    app_rx: Option<Receiver<AppCommand>>,
}

impl Service {
    /// Records into `dir`, created if missing, grading with `station`.
    pub fn new(dir: PathBuf, station: Arc<Station>) -> io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        let (requests, requested) = mpsc::channel();
        let (feed, feed_rx) = mpsc::channel();
        let (app, app_rx) = mpsc::channel();
        Ok(Self {
            feed,
            feed_rx: Some(feed_rx),
            app,
            app_rx: Some(app_rx),
            index: Arc::new(Index::open(&dir)),
            dir,
            station,
            stopper: Stopper {
                stopped: Arc::default(),
                wake: requests.clone(),
            },
            raw: false,
            analysing: Arc::default(),
            analysis_us: Arc::default(),
            watched: Arc::default(),
            requests,
            requested,
        })
    }

    /// Also keeps every flight's raw capture (off by default: it is about
    /// twice the journal's size, which is itself compacted when closed).
    pub fn raw(mut self, on: bool) -> Self {
        self.raw = on;
        self
    }

    pub fn stopper(&self) -> Stopper {
        self.stopper.clone()
    }

    pub fn sim_requests(&self) -> SimRequests {
        SimRequests(self.requests.clone())
    }

    /// The app's way to the in-sim views (challenge attempts, B9).
    pub fn views(&self) -> Views {
        Views(self.feed.clone())
    }

    /// The views' attempt commands; taken once, by the app. Unless taken,
    /// a view's attempt command is refused.
    pub fn app_commands(&mut self) -> Option<Receiver<AppCommand>> {
        self.app_rx.take()
    }

    /// The flights folder's index; share it rather than open another.
    pub fn index(&self) -> Arc<Index> {
        self.index.clone()
    }

    /// Runs until stopped. Blocks: give it a thread of its own, which then
    /// owns the SimConnect connection. Analyses run on threads of their own
    /// so recording never waits for them.
    pub fn run(mut self, out: Sender<Happening>) {
        // Everything reported also goes to the in-sim panel, which sends
        // its views' commands back and answers their history queries.
        let (commands_tx, commands) = mpsc::channel();
        let history = History::new(self.dir.clone(), self.station.clone(), self.index.clone());
        // Not taken by the app: attempt commands are refused, not lost.
        drop(self.app_rx.take());
        let feed = self.feed.clone();
        let Some(rx) = self.feed_rx.take() else {
            return;
        };
        let tee = Tee {
            rx,
            out,
            commands: commands_tx,
            app: self.app.clone(),
            watched: self.watched.clone(),
        };
        panel::tee(tee, history);
        // Journals of earlier runs, listed before any new flight exists.
        let (dir, backlog) = (self.dir.clone(), crate::backlog(&self.dir));
        thread::spawn(move || crate::recover(&dir, backlog));
        let acquisition = Acquisition::new(&Lfl::V1);
        let mut status = StatusLine {
            feed: feed.clone(),
            last: None,
        };
        let mut retry = RETRY_MIN;
        while !self.stopper.stopped() {
            match LiveSource::open(&acquisition) {
                Ok(mut source) => {
                    retry = RETRY_MIN;
                    status.set(RecorderStatus::Connected);
                    let mut watch = Watch::default();
                    record(&mut source, &acquisition, &self.dir, self.raw, |p| {
                        let link = Link {
                            feed: &feed,
                            commands: &commands,
                        };
                        self.follow(p, &mut watch, &mut status, link)
                    });
                    status.set(RecorderStatus::WaitingForSim);
                }
                // How SimConnect_Open fails while MSFS is not running.
                Err(sim_link::Error::Call { .. }) => status.set(RecorderStatus::WaitingForSim),
                Err(e) => status.set(RecorderStatus::Error {
                    message: e.to_string(),
                }),
            }
            // Asleep until the retry, a request (refused at once) or a stop.
            let until = Instant::now() + retry;
            while !self.stopper.stopped() {
                let wait = until.saturating_duration_since(Instant::now());
                match self.requested.recv_timeout(wait) {
                    Ok(request) => request.refuse("not connected to the simulator"),
                    Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
                }
            }
            retry = (retry * 2).min(RETRY_MAX);
        }
    }

    fn follow(
        &self,
        progress: Progress,
        watch: &mut Watch,
        status: &mut StatusLine,
        link: Link,
    ) -> Reply {
        let mut command = None;
        match progress {
            Progress::Opened(journal) => {
                // Commands sent between flights are not for this one.
                for cmd in link.commands.try_iter() {
                    link.ack(&cmd, Err("no flight open".into()));
                }
                let now = Instant::now();
                *watch = Watch {
                    flight: Some(OpenFlight {
                        journal: journal.to_owned(),
                        id: flight_id(journal),
                        started: now,
                        monitor: Monitor::new(self.station.clone()),
                        next_live: now,
                        next_snapshot: now,
                        snapshot_s: None,
                        closing: None,
                    }),
                    touchdown_s: None,
                    fault: None,
                };
            }
            Progress::Recorded { item, touchdown } => {
                // Recorded only follows Opened; stop recording if it ever does not.
                let Some(flight) = watch.flight.as_mut() else {
                    return ControlFlow::Break(());
                };
                flight.monitor.record(item);
                if let Telemetry::Frame(frame) = item {
                    // Column 0 is always SIMULATION TIME (raw capture format).
                    let sim_time_s = frame[0];
                    if touchdown {
                        watch.touchdown_s = Some(sim_time_s);
                    }
                    // Due once the window has passed, or at once if sim time
                    // jumped back (a flight reload).
                    let due = watch
                        .touchdown_s
                        .is_some_and(|t| !(t..t + LANDING_AFTER_S).contains(&sim_time_s));
                    if due {
                        watch.touchdown_s = None;
                        // The landing's analysis also refreshes the panel.
                        self.report(flight.journal.clone(), link.feed, false, |a| {
                            let landing = a.landing().map(|n| Happening::Landing(n).into());
                            landing.into_iter().chain([snapshot(a)]).collect()
                        });
                    }
                    self.panel(flight, link.feed, sim_time_s);
                    command = link.command(flight);
                }
            }
            Progress::Saved(journal) => {
                // A flight closed before its landing was due reports it now.
                let landing = watch.touchdown_s.take().is_some();
                let closing = watch.flight.take().and_then(|f| f.closing);
                if closing == Some(ViewCommand::DiscardFlight) {
                    let deleted = debrief::delete_flight(&self.dir, &flight_id(journal));
                    let result = deleted.map(drop).map_err(|e| e.to_string());
                    link.ack(&ViewCommand::DiscardFlight, result);
                } else {
                    self.report(journal.to_owned(), link.feed, true, move |a| {
                        let notice = a.landing().filter(|_| landing);
                        notice
                            .map(Happening::Landing)
                            .into_iter()
                            .chain([Happening::FlightSaved(a.summary())])
                            .map(Msg::from)
                            .collect()
                    });
                    if let Some(cmd) = closing {
                        link.ack(&cmd, Ok(()));
                    }
                }
            }
            Progress::Discarded(_, why) => {
                // Nothing was saved, so nothing is analysed or listed.
                watch.touchdown_s = None;
                let closing = watch.flight.take().and_then(|f| f.closing);
                match closing {
                    // Deleting is what was asked for.
                    Some(ViewCommand::DiscardFlight) => {
                        link.ack(&ViewCommand::DiscardFlight, Ok(()))
                    }
                    Some(cmd) => link.ack(&cmd, Err(format!("nothing to save: {why}"))),
                    None => {}
                }
            }
            Progress::Idle => command = link.idle_command(),
            // Paused or loading: the panel's commands still count.
            Progress::Tick => {
                command = match watch.flight.as_mut() {
                    Some(flight) => link.command(flight),
                    None => link.idle_command(),
                }
            }
            Progress::Failed(e) => {
                let dir = self.dir.display();
                watch.fault = Some(format!("cannot write flights to {dir}: {e}"));
            }
        }
        // The app's requests wait for a frame without a panel command.
        let command = command.or_else(|| self.requested.try_recv().ok());
        match (&watch.flight, &watch.fault) {
            (Some(flight), _) => status.recording(&flight.id, flight.started.elapsed()),
            (None, Some(message)) => status.set(RecorderStatus::Error {
                message: message.clone(),
            }),
            (None, None) => status.set(RecorderStatus::Connected),
        }
        if self.stopper.stopped() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(command)
        }
    }

    /// While a view watches: the panel's live values at 4 Hz, and every
    /// few seconds of moving sim time the flight analysed as it stands, on
    /// a thread of its own and one at a time.
    fn panel(&self, flight: &mut OpenFlight, feed: &Sender<Msg>, sim_time_s: f64) {
        if !self.watched.load(Ordering::Relaxed) {
            return;
        }
        let now = Instant::now();
        if now >= flight.next_live {
            flight.next_live = now + LIVE_EVERY;
            if let Some(live) = flight.monitor.live() {
                let _ = feed.send(Msg::Live(live));
            }
        }
        if now < flight.next_snapshot
            || flight.snapshot_s == Some(sim_time_s)
            || self.analysing.swap(true, Ordering::Acquire)
        {
            return;
        }
        let cost = Duration::from_micros(self.analysis_us.load(Ordering::Relaxed));
        flight.next_snapshot = now + SNAPSHOT_EVERY.max(cost * SNAPSHOT_COST_FACTOR);
        flight.snapshot_s = Some(sim_time_s);
        let (station, feed, journal) = (self.station.clone(), feed.clone(), flight.journal.clone());
        let (busy, cost) = (self.analysing.clone(), self.analysis_us.clone());
        thread::spawn(move || {
            let _clear = ClearOnDrop(busy);
            let started = Instant::now();
            match station.open(&journal) {
                Ok(a) => {
                    let _ = feed.send(snapshot(&a));
                }
                Err(e) => eprintln!("{}: {e}", journal.display()),
            }
            let us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
            cost.store(us, Ordering::Relaxed);
        });
    }

    /// Sends what `pick` takes from the journal as it stands, analysed on a
    /// thread of its own; a closed flight is then compacted. Quitting mid-way
    /// is safe: compaction writes `.part` and renames, and a journal left
    /// behind is compacted on the next run.
    fn report(
        &self,
        journal: PathBuf,
        feed: &Sender<Msg>,
        closed: bool,
        pick: impl FnOnce(&Analysis) -> Vec<Msg> + Send + 'static,
    ) {
        let (station, feed) = (self.station.clone(), feed.clone());
        thread::spawn(move || {
            match station.open(&journal) {
                Ok(analysis) => pick(&analysis).into_iter().for_each(|msg| {
                    // The receiver is gone only when the app is closing.
                    let _ = feed.send(msg);
                }),
                Err(e) => eprintln!("{}: {e}", journal.display()),
            }
            if closed {
                debrief::compact_all([journal]);
            }
        });
    }
}

/// The open flight as the panel shows it.
fn snapshot(a: &Analysis) -> Msg {
    Msg::Snapshot(Snapshot {
        summary: a.summary(),
        events: a.events(),
        landings: a.landings(),
    })
}

/// Clears the flag when dropped, even by a panic.
pub(crate) struct ClearOnDrop(pub Arc<AtomicBool>);

impl Drop for ClearOnDrop {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// The way to the in-sim panel and back.
#[derive(Clone, Copy)]
struct Link<'a> {
    feed: &'a Sender<Msg>,
    commands: &'a Receiver<ViewCommand>,
}

impl Link<'_> {
    fn ack(&self, cmd: &ViewCommand, result: Result<(), String>) {
        let _ = self.feed.send(Msg::Ack(Ack::new(cmd.name(), result)));
    }

    /// The next view command while no flight is open: only a start, which
    /// is answered at once.
    fn idle_command(&self) -> Option<Command> {
        let cmd = self.commands.try_recv().ok()?;
        if cmd != ViewCommand::StartFlight {
            self.ack(&cmd, Err("no flight open".into()));
            return None;
        }
        self.ack(&cmd, Ok(()));
        Some(Command::Start)
    }

    /// The next view command for `record`, if any. Ending or discarding
    /// the flight is answered once it is saved, a mark at once.
    fn command(&self, flight: &mut OpenFlight) -> Option<Command> {
        let cmd = self.commands.try_recv().ok()?;
        match cmd {
            ViewCommand::StartFlight => {
                self.ack(&cmd, Err("a flight is already recording".into()));
                None
            }
            ViewCommand::Mark(note) => {
                self.ack(&ViewCommand::Mark(String::new()), Ok(()));
                Some(Command::Mark(note))
            }
            _ if flight.closing.is_some() => {
                self.ack(&cmd, Err("the flight is already closing".into()));
                None
            }
            _ => {
                flight.closing = Some(cmd);
                Some(Command::Close)
            }
        }
    }
}

/// The flight being recorded, the sim time of a touchdown whose landing is
/// not yet reported, and why no flight is, until one opens again.
#[derive(Default)]
struct Watch {
    flight: Option<OpenFlight>,
    touchdown_s: Option<f64>,
    fault: Option<String>,
}

struct OpenFlight {
    journal: PathBuf,
    id: String,
    /// Wall clock: the status shows the time the pilot has been flying.
    started: Instant,
    monitor: Monitor,
    next_live: Instant,
    next_snapshot: Instant,
    /// Sim time of the last snapshot: paused, the next waits.
    snapshot_s: Option<f64>,
    /// The view command closing it: end or discard.
    closing: Option<ViewCommand>,
}

/// Sends the status only when it changes.
struct StatusLine {
    feed: Sender<Msg>,
    last: Option<RecorderStatus>,
}

impl StatusLine {
    /// Recording flight `id` for `elapsed`, in whole seconds; only built
    /// when that changes, not at each frame.
    fn recording(&mut self, id: &str, elapsed: Duration) {
        let elapsed_s = elapsed.as_secs() as f64;
        if let Some(RecorderStatus::Recording {
            flight_id,
            elapsed_s: last,
        }) = &self.last
            && flight_id == id
            && *last == elapsed_s
        {
            return;
        }
        self.set(RecorderStatus::Recording {
            flight_id: id.to_owned(),
            elapsed_s,
        });
    }

    fn set(&mut self, status: RecorderStatus) {
        if self.last.as_ref() != Some(&status) {
            self.last = Some(status.clone());
            let _ = self.feed.send(Happening::Status(status).into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recording_status_goes_out_once_a_second() {
        let (feed, rx) = mpsc::channel();
        let mut status = StatusLine { feed, last: None };
        for ms in [0, 400, 999, 1000, 1500, 2000] {
            status.recording("flight-1", Duration::from_millis(ms));
        }
        status.recording("flight-2", Duration::from_secs(2));
        let sent: Vec<_> = rx
            .try_iter()
            .map(|m| match m {
                Msg::Happening(Happening::Status(RecorderStatus::Recording {
                    flight_id,
                    elapsed_s,
                })) => (flight_id, elapsed_s),
                other => panic!("{other:?}"),
            })
            .collect();
        let at = |id: &str, s| (id.to_owned(), s);
        let expected = [
            at("flight-1", 0.0),
            at("flight-1", 1.0),
            at("flight-1", 2.0),
            at("flight-2", 2.0),
        ];
        assert_eq!(sent, expected);
    }
}
