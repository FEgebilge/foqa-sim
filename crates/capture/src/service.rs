//! The recorder as a background service: waits for the simulator, records
//! every flight while it runs, and comes back to waiting when it quits, the
//! way a QAR records whenever the aircraft is powered without anyone
//! switching it on. Status and analysed results go out on a channel; the
//! in-sim panel also gets the flight live and can end, discard or mark it.

use std::io;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use debrief::{Analysis, FlightSummary, Index, LandingNotice, RecorderStatus, Station, flight_id};
use fdm_core::{Airport, Lfl};
use recorder::{Acquisition, TOUCHDOWN_WINDOW_US, Telemetry};

use crate::history::History;
use crate::monitor::Monitor;
use crate::panel::{self, Ack, Msg, Snapshot, ViewCommand};
use crate::place::{Ask, Start};
use crate::{Command, LiveSource, Progress, Reply, record};

/// First and longest wait between attempts to reach the simulator.
/// `SimConnect_Open` fails at once while MSFS is not running; doubling the
/// wait keeps an idle tray app from polling it many times a second.
const RETRY_MIN: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(15);
/// How often a wait checks for a stop request.
const STOP_POLL: Duration = Duration::from_millis(100);
/// Sim seconds after the last touchdown of a landing before it is analysed:
/// the recorder's full-rate window after contact, plus time for the
/// touchdown airport snapshot to arrive. A bounce restarts the wait, so one
/// landing gives one notice.
const LANDING_AFTER_S: f64 = TOUCHDOWN_WINDOW_US as f64 / 1e6 + 2.0;
/// The panel's live values, 4 Hz.
const LIVE_EVERY: Duration = Duration::from_millis(250);
/// The open flight analysed for the panel, at most one analysis at a time.
const SNAPSHOT_EVERY: Duration = Duration::from_secs(5);

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
#[derive(Debug, Clone, Default)]
pub struct Stopper(Arc<AtomicBool>);

impl Stopper {
    pub fn stop(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    fn stopped(&self) -> bool {
        self.0.load(Ordering::Relaxed)
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
    requests: Sender<Command>,
    /// The app's requests (`SimRequests`): only `Place` and `Airport`.
    requested: Receiver<Command>,
}

impl Service {
    /// Records into `dir`, created if missing, grading with `station`.
    pub fn new(dir: PathBuf, station: Arc<Station>) -> io::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        let (requests, requested) = mpsc::channel();
        Ok(Self {
            index: Arc::new(Index::open(&dir)),
            dir,
            station,
            stopper: Stopper::default(),
            raw: false,
            analysing: Arc::default(),
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

    /// The flights folder's index; share it rather than open another.
    pub fn index(&self) -> Arc<Index> {
        self.index.clone()
    }

    /// Runs until stopped. Blocks: give it a thread of its own, which then
    /// owns the SimConnect connection. Analyses run on threads of their own
    /// so recording never waits for them.
    pub fn run(self, out: Sender<Happening>) {
        // Everything reported also goes to the in-sim panel, which sends
        // its views' commands back and answers their history queries.
        let (commands_tx, commands) = mpsc::channel();
        let history = History::new(self.dir.clone(), self.station.clone(), self.index.clone());
        let feed = panel::tee(out, commands_tx, history);
        // Journals of earlier runs, listed before any new flight exists.
        let backlog = debrief::journals(&self.dir);
        thread::spawn(move || debrief::compact_all(backlog));
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
                    let result = record(&mut source, &acquisition, &self.dir, self.raw, |p| {
                        let link = Link {
                            feed: &feed,
                            commands: &commands,
                        };
                        self.follow(p, &mut watch, &mut status, link)
                    });
                    status.set(match result {
                        Ok(()) => RecorderStatus::WaitingForSim,
                        Err(e) => RecorderStatus::Error {
                            message: format!("recording failed: {e}"),
                        },
                    });
                }
                // How SimConnect_Open fails while MSFS is not running.
                Err(sim_link::Error::Call { .. }) => status.set(RecorderStatus::WaitingForSim),
                Err(e) => status.set(RecorderStatus::Error {
                    message: e.to_string(),
                }),
            }
            let until = Instant::now() + retry;
            while !self.stopper.stopped() && Instant::now() < until {
                for request in self.requested.try_iter() {
                    request.refuse("not connected to the simulator");
                }
                thread::sleep(STOP_POLL);
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
                        closing: None,
                    }),
                    touchdown_s: None,
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
                        self.report(flight.journal.clone(), link.feed, false, |a| {
                            a.landing().map(Happening::Landing).into_iter().collect()
                        });
                    }
                    self.panel(flight, link.feed);
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
                            .collect()
                    });
                    if let Some(cmd) = closing {
                        link.ack(&cmd, Ok(()));
                    }
                }
            }
            Progress::Idle => command = link.idle_command(),
        }
        // The app's requests wait for a frame without a panel command.
        let command = command.or_else(|| self.requested.try_recv().ok());
        status.set(match &watch.flight {
            Some(flight) => RecorderStatus::Recording {
                flight_id: flight.id.clone(),
                elapsed_s: flight.started.elapsed().as_secs() as f64,
            },
            None => RecorderStatus::Connected,
        });
        if self.stopper.stopped() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(command)
        }
    }

    /// The panel's live values at 4 Hz, and every few seconds the flight
    /// analysed as it stands, on a thread of its own and one at a time.
    fn panel(&self, flight: &mut OpenFlight, feed: &Sender<Msg>) {
        let now = Instant::now();
        if now >= flight.next_live {
            flight.next_live = now + LIVE_EVERY;
            if let Some(live) = flight.monitor.live() {
                let _ = feed.send(Msg::Live(live));
            }
        }
        if now < flight.next_snapshot || self.analysing.swap(true, Ordering::Acquire) {
            return;
        }
        flight.next_snapshot = now + SNAPSHOT_EVERY;
        let (station, feed, busy) = (self.station.clone(), feed.clone(), self.analysing.clone());
        let journal = flight.journal.clone();
        thread::spawn(move || {
            let _clear = ClearOnDrop(busy);
            match station.open(&journal) {
                Ok(a) => {
                    let _ = feed.send(Msg::Snapshot(Snapshot {
                        summary: a.summary(),
                        events: a.events(),
                        landings: a.landings(),
                    }));
                }
                Err(e) => eprintln!("{}: {e}", journal.display()),
            }
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
        pick: impl FnOnce(&Analysis) -> Vec<Happening> + Send + 'static,
    ) {
        let (station, feed) = (self.station.clone(), feed.clone());
        thread::spawn(move || {
            match station.open(&journal) {
                Ok(analysis) => pick(&analysis).into_iter().for_each(|h| {
                    // The receiver is gone only when the app is closing.
                    let _ = feed.send(h.into());
                }),
                Err(e) => eprintln!("{}: {e}", journal.display()),
            }
            if closed {
                debrief::compact_all([journal]);
            }
        });
    }
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

/// The flight being recorded, and the sim time of a touchdown whose
/// landing is not yet reported.
#[derive(Default)]
struct Watch {
    flight: Option<OpenFlight>,
    touchdown_s: Option<f64>,
}

struct OpenFlight {
    journal: PathBuf,
    id: String,
    /// Wall clock: the status shows the time the pilot has been flying.
    started: Instant,
    monitor: Monitor,
    next_live: Instant,
    next_snapshot: Instant,
    /// The view command closing it: end or discard.
    closing: Option<ViewCommand>,
}

/// Sends the status only when it changes.
struct StatusLine {
    feed: Sender<Msg>,
    last: Option<RecorderStatus>,
}

impl StatusLine {
    fn set(&mut self, status: RecorderStatus) {
        if self.last.as_ref() != Some(&status) {
            self.last = Some(status.clone());
            let _ = self.feed.send(Happening::Status(status).into());
        }
    }
}
