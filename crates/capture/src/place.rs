//! Requests the desktop app makes of the simulator through the running
//! service: put the user aircraft at a start (a landing challenge on final,
//! airborne) and configure it, save the flight as a clean start and load it
//! back, or fetch one airport's runways or a runway end's ILS. Each request
//! carries the way its answer goes back, so the caller can wait on it.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use fdm_core::Airport;
use recorder::{FlowEvent, SimEvent, Telemetry};
pub use sim_link::InitPosition;
pub use sim_link::ils::{Ils, IlsQuery};

/// Key events a start sends; each is mapped once per connection as client
/// event `KEY_EVENT_BASE` + its index. Names from the MSFS key event list
/// (legacy events, still in MSFS 2024; `NAV1_RADIO_SET_HZ` takes Hz, not
/// BCD: Aircraft Radio Navigation Events).
pub(crate) const KEY_EVENTS: [&str; 5] = [
    "AUTOPILOT_OFF",
    "GEAR_DOWN",
    "GEAR_UP",
    "FLAPS_SET",
    "NAV1_RADIO_SET_HZ",
];
/// Above the system event ids (1..=`SYSTEM_EVENTS.len()`): client event ids
/// share one space.
pub(crate) const KEY_EVENT_BASE: u32 = 100;
/// `FLAPS_SET` data of the fully extended handle.
const FLAPS_FULL: f64 = 16383.0;

/// Where and how a start puts the user aircraft.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Start {
    pub pos: InitPosition,
    pub gear_down: bool,
    /// Flap handle as a fraction of its travel, `None` to leave it. Detent
    /// `i` of a handle with `n` steps past up is `i / n` (737 flaps 30 is
    /// 7 of 8: 0.875); the sim moves to the closest detent.
    pub flaps: Option<f64>,
    /// Active NAV1 frequency, Hz (the landing runway's ILS); `None` leaves it.
    pub nav1_hz: Option<u32>,
}

impl Start {
    /// The key events sent after the position, in order, with their data:
    /// autopilot off, the gear, then the flaps and NAV1 if set.
    pub fn events(&self) -> Vec<(&'static str, u32)> {
        let gear = if self.gear_down {
            "GEAR_DOWN"
        } else {
            "GEAR_UP"
        };
        let mut events = vec![("AUTOPILOT_OFF", 0), (gear, 0)];
        events.extend(self.flaps.map(|f| ("FLAPS_SET", flaps_set_data(f))));
        events.extend(self.nav1_hz.map(|hz| ("NAV1_RADIO_SET_HZ", hz)));
        events
    }
}

/// `FLAPS_SET` data (0 up to 16383 full) for a handle fraction; outside
/// 0..1 is clamped, NaN is up.
pub fn flaps_set_data(fraction: f64) -> u32 {
    // NaN survives the clamp and casts to 0.
    (fraction.clamp(0.0, 1.0) * FLAPS_FULL).round() as u32
}

/// The client event id of key event `name`, one of `KEY_EVENTS`.
pub(crate) fn key_event_id(name: &str) -> Option<u32> {
    let index = KEY_EVENTS.iter().position(|&e| e == name)?;
    Some(KEY_EVENT_BASE + index as u32)
}

/// How long the asker waits for an answer; an ask still queued after it
/// is refused, not acted on (a start minutes late would move the aircraft
/// when nobody expects it).
pub const ASK_TIMEOUT: Duration = Duration::from_secs(15);

/// A request and the way its answer goes back. Dropped unanswered (the
/// sim quit, the service stopped), its receiver reports disconnection.
#[derive(Debug, Clone)]
pub struct Ask<Q, A> {
    pub query: Q,
    reply: Sender<Result<A, String>>,
    /// Created + `ASK_TIMEOUT`: the asker has given up after it.
    deadline: Instant,
}

impl<Q, A> Ask<Q, A> {
    pub fn new(query: Q) -> (Self, Receiver<Result<A, String>>) {
        let (reply, answer) = mpsc::channel();
        let deadline = Instant::now() + ASK_TIMEOUT;
        (
            Self {
                query,
                reply,
                deadline,
            },
            answer,
        )
    }

    /// The ask if its asker still waits at `now`; past the deadline it is
    /// answered with an error and `None` returned, to be dropped.
    pub fn live(self, now: Instant) -> Option<Self> {
        if now <= self.deadline {
            return Some(self);
        }
        self.answer(Err(
            "the request expired before the sim could act on it".into()
        ));
        None
    }

    pub fn answer(self, result: Result<A, String>) {
        // The asker may have stopped waiting.
        let _ = self.reply.send(result);
    }
}

/// Airports asked for by ICAO on one connection, and those already
/// fetched, which answer at once: the facility fetch asks for an airport
/// only once per flight.
#[derive(Debug, Default)]
pub(crate) struct AirportAsks {
    fetched: HashMap<String, Airport>,
    waiting: HashMap<String, Vec<Ask<String, Airport>>>,
}

impl AirportAsks {
    /// Answers `ask` from what is fetched, or keeps it and returns the
    /// ICAO (trimmed, upper case) to fetch.
    pub fn ask(&mut self, ask: Ask<String, Airport>) -> Option<String> {
        let icao = ask.query.trim().to_ascii_uppercase();
        if let Some(airport) = self.fetched.get(&icao) {
            ask.answer(Ok(airport.clone()));
            return None;
        }
        self.waiting.entry(icao.clone()).or_default().push(ask);
        Some(icao)
    }

    /// Any airport fetched, asked for or not.
    pub fn fetched(&mut self, airport: &Airport) {
        self.answer(&airport.icao, Ok(airport.clone()));
        self.fetched.insert(airport.icao.clone(), airport.clone());
    }

    pub fn failed(&mut self, icao: &str, reason: String) {
        self.answer(icao, Err(reason));
    }

    /// ICAOs still awaited, to ask the fetch again after it was reset.
    pub fn waiting(&self) -> impl Iterator<Item = &String> {
        self.waiting.keys()
    }

    fn answer(&mut self, icao: &str, result: Result<Airport, String>) {
        for ask in self.waiting.remove(icao).into_iter().flatten() {
            ask.answer(result.clone());
        }
    }
}

/// ILS lookups asked for on one connection: one lookup answers every ask
/// for the same runway end.
#[derive(Debug, Default)]
pub(crate) struct IlsAsks(Vec<Ask<IlsQuery, Option<Ils>>>);

impl IlsAsks {
    /// Keeps `ask`; returns its query when no lookup for it is out yet.
    pub fn ask(&mut self, ask: Ask<IlsQuery, Option<Ils>>) -> Option<IlsQuery> {
        let new = !self.0.iter().any(|a| a.query == ask.query);
        let query = new.then(|| ask.query.clone());
        self.0.push(ask);
        query
    }

    pub fn found(&mut self, query: &IlsQuery, result: Result<Option<Ils>, String>) {
        for ask in self.0.extract_if(.., |a| a.query == *query) {
            ask.answer(result.clone());
        }
    }
}

/// A save waits this long after the last placement: the teleport, gear,
/// flaps and NAV1 it sent have then taken effect (the sim reports the
/// teleport done within a frame or two), so the flight saved is the start.
pub(crate) const SAVE_AFTER: Duration = Duration::from_secs(2);
/// The longest a reload waits for the sim to settle after the load.
pub const RELOAD_TIMEOUT: Duration = Duration::from_secs(60);
/// Without the sim's own move to the runway (MSFS 2020 sends no flow
/// events; another flow may not move the aircraft) a reload is settled this
/// long after the last `SimStart`. MSFS 2024 begins that move about 10 s
/// after the load's `SimStart`, so it is not cut short.
pub(crate) const QUIET_AFTER: Duration = Duration::from_secs(15);

/// How a reload settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    /// The sim's own move to the runway ended (`RTC_END`), as MSFS 2024
    /// does after a flight load.
    RunwayMove,
    /// No such move began: `QUIET_AFTER` past the last `SimStart`.
    Quiet,
}

/// A flight reloaded and settled, and the flight the recorder opened after
/// it: the one to place the start in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reloaded {
    pub flight_id: String,
    pub settled: Settled,
}

/// What a reload waits for once `SimConnect_FlightLoad` went out. MSFS 2024
/// (bench, 2026-10-30): `FLIGHT_END`, `FLT_LOAD`/`FLT_LOADED`, a teleport,
/// `SimStop`/`SimStart`; about 10 s later its own move to the runway:
/// `FLIGHT_START`, `taxi.flt` loaded, `RTC_START`, a teleport, `runway.flt`,
/// `RTC_END`. A start placed before that move ends is undone by it. Fed
/// every item the sim sends, in order.
#[derive(Debug, Clone)]
pub(crate) struct ReloadWait {
    sent: Instant,
    /// The flight file loaded (`FLT_LOADED`, or MSFS 2020's `FlightLoaded`).
    loaded: bool,
    /// The sim's move to the runway began after the load.
    moving: bool,
    settled: Option<Settled>,
    /// The last `SimStart` after the load.
    sim_start: Option<Instant>,
    failed: Option<&'static str>,
}

impl ReloadWait {
    pub fn new(sent: Instant) -> Self {
        Self {
            sent,
            loaded: false,
            moving: false,
            settled: None,
            sim_start: None,
            failed: None,
        }
    }

    /// Follows one item the sim sent, at `now`.
    pub fn see(&mut self, item: &Telemetry, now: Instant) {
        let Telemetry::Event(event) = item else {
            return;
        };
        match event {
            SimEvent::Flow(FlowEvent::FltLoaded) | SimEvent::FlightLoaded(_) => self.loaded = true,
            SimEvent::Flow(FlowEvent::RtcStart) if self.loaded => self.moving = true,
            SimEvent::Flow(FlowEvent::RtcEnd) if self.moving => {
                self.settled = Some(Settled::RunwayMove);
            }
            SimEvent::Flow(FlowEvent::BackToMainMenu) => {
                self.failed = Some("the sim went back to the main menu");
            }
            SimEvent::SimStart if self.loaded => self.sim_start = Some(now),
            _ => {}
        }
    }

    /// `RELOAD_TIMEOUT` has passed since the load was sent.
    pub fn expired(&self, now: Instant) -> bool {
        now >= self.sent + RELOAD_TIMEOUT
    }

    /// `None` while the sim is still loading, else how it settled or why
    /// the reload failed.
    pub fn state(&self, now: Instant) -> Option<Result<Settled, String>> {
        if let Some(why) = self.failed {
            return Some(Err(why.into()));
        }
        if let Some(settled) = self.settled {
            return Some(Ok(settled));
        }
        let quiet = self.sim_start.is_some_and(|t| now >= t + QUIET_AFTER);
        if quiet && !self.moving {
            return Some(Ok(Settled::Quiet));
        }
        self.expired(now).then(|| {
            Err(if self.loaded {
                "the sim did not finish loading the clean start".into()
            } else {
                "the sim did not load the clean start".into()
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(gear_down: bool, flaps: Option<f64>, nav1_hz: Option<u32>) -> Start {
        let pos = InitPosition {
            latitude_deg: 40.9,
            longitude_deg: 29.3,
            altitude_ft: 2900.0,
            pitch_deg: 0.0,
            bank_deg: 0.0,
            heading_deg: 62.0,
            on_ground: 0,
            airspeed_kt: 160,
        };
        Start {
            pos,
            gear_down,
            flaps,
            nav1_hz,
        }
    }

    #[test]
    fn flaps_fraction_maps_onto_the_handle_range() {
        assert_eq!(flaps_set_data(0.0), 0);
        assert_eq!(flaps_set_data(1.0), 16383);
        assert_eq!(flaps_set_data(0.875), 14335);
        assert_eq!(flaps_set_data(1.5), 16383);
        assert_eq!(flaps_set_data(-0.2), 0);
        assert_eq!(flaps_set_data(f64::NAN), 0);
    }

    #[test]
    fn start_events_are_ap_off_gear_flaps_then_nav1() {
        let events = start(true, Some(1.0), Some(110_300_000)).events();
        assert_eq!(
            events,
            [
                ("AUTOPILOT_OFF", 0),
                ("GEAR_DOWN", 0),
                ("FLAPS_SET", 16383),
                ("NAV1_RADIO_SET_HZ", 110_300_000)
            ]
        );
        assert_eq!(
            start(false, None, None).events(),
            [("AUTOPILOT_OFF", 0), ("GEAR_UP", 0)]
        );
        // Every event a start sends is mapped.
        assert!(events.iter().all(|(name, _)| key_event_id(name).is_some()));
        assert_eq!(key_event_id("FLAPS_SET"), Some(103));
        assert_eq!(key_event_id("AP_MASTER"), None);
    }

    fn airport(icao: &str) -> Airport {
        Airport {
            icao: icao.into(),
            region: "LT".into(),
            lat_deg: 40.9,
            lon_deg: 29.3,
            elevation_m: 95.0,
            magvar_deg: -6.0,
            runways: Vec::new(),
        }
    }

    #[test]
    fn asked_airports_are_answered_when_fetched_then_from_memory() {
        let mut asks = AirportAsks::default();
        let (ask, first) = Ask::new(" ltfj ".to_owned());
        assert_eq!(asks.ask(ask).as_deref(), Some("LTFJ"));
        let (ask, second) = Ask::new("LTFJ".to_owned());
        assert_eq!(asks.ask(ask).as_deref(), Some("LTFJ"));
        assert_eq!(asks.waiting().count(), 1);
        asks.fetched(&airport("LTFJ"));
        assert_eq!(first.recv().unwrap().unwrap().icao, "LTFJ");
        assert_eq!(second.recv().unwrap().unwrap().icao, "LTFJ");
        // Fetched once: a later ask is answered without a request.
        let (ask, third) = Ask::new("LTFJ".to_owned());
        assert_eq!(asks.ask(ask), None);
        assert!(third.recv().unwrap().is_ok());
        assert_eq!(asks.waiting().count(), 0);
    }

    #[test]
    fn an_ask_past_its_deadline_is_refused_not_acted_on() {
        let (ask, answer) = Ask::<_, ()>::new(start(true, None, None));
        let now = Instant::now();
        let ask = ask.live(now).expect("waited on at once");
        assert!(
            ask.live(now + ASK_TIMEOUT + Duration::from_secs(1))
                .is_none()
        );
        assert!(answer.recv().unwrap().unwrap_err().contains("expired"));
    }

    #[test]
    fn a_failed_airport_answers_its_askers_with_the_reason() {
        let mut asks = AirportAsks::default();
        let (ask, answer) = Ask::new("XXXX".to_owned());
        asks.ask(ask);
        asks.failed("XXXX", "timeout".into());
        assert_eq!(answer.recv().unwrap(), Err("timeout".into()));
        // A dropped ask disconnects its receiver.
        let (ask, answer) = Ask::<String, Airport>::new("LTBJ".into());
        drop(ask);
        assert!(answer.recv().is_err());
    }

    #[test]
    fn one_ils_lookup_answers_every_ask_for_its_runway_end() {
        let end = |runway: &str| IlsQuery {
            airport: "LTBJ".into(),
            runway: runway.into(),
        };
        let mut asks = IlsAsks::default();
        let (ask, first) = Ask::new(end("16L"));
        assert_eq!(asks.ask(ask), Some(end("16L")));
        let (ask, second) = Ask::new(end("16L"));
        assert_eq!(asks.ask(ask), None, "already looked up");
        let (ask, other) = Ask::new(end("34R"));
        assert_eq!(asks.ask(ask), Some(end("34R")));
        asks.found(&end("16L"), Ok(None));
        assert_eq!(first.recv().unwrap(), Ok(None));
        assert_eq!(second.recv().unwrap(), Ok(None));
        assert!(other.try_recv().is_err(), "another end waits on");
    }

    fn flow(f: FlowEvent) -> Telemetry {
        Telemetry::Event(SimEvent::Flow(f))
    }

    /// The wait's state `after` the last of `items`, fed one second apart
    /// from the load.
    fn reload(items: &[Telemetry], after: Duration) -> Option<Result<Settled, String>> {
        let sent = Instant::now();
        let mut wait = ReloadWait::new(sent);
        let mut now = sent;
        for item in items {
            now += Duration::from_secs(1);
            wait.see(item, now);
        }
        wait.state(now + after)
    }

    #[test]
    fn a_reload_settles_when_the_sims_move_to_the_runway_ends() {
        use FlowEvent::*;
        let items = [
            flow(FlightEnd),
            flow(FltLoad),
            flow(FltLoaded),
            flow(TeleportStart),
            flow(TeleportDone),
            Telemetry::Event(SimEvent::SimStop),
            Telemetry::Event(SimEvent::SimStart),
            flow(FlightStart),
            flow(FltLoad),
            flow(FltLoaded),
            flow(RtcStart),
            flow(TeleportStart),
            flow(TeleportDone),
        ];
        assert_eq!(reload(&items, Duration::ZERO), None, "still moving");
        // Moving: the quiet fallback does not cut it short.
        assert_eq!(reload(&items, QUIET_AFTER), None);
        let done: Vec<_> = items.iter().cloned().chain([flow(RtcEnd)]).collect();
        assert_eq!(reload(&done, Duration::ZERO), Some(Ok(Settled::RunwayMove)));
    }

    #[test]
    fn without_a_move_to_the_runway_a_reload_settles_after_a_quiet_spell() {
        // MSFS 2020: no flow events, the system events only.
        let items = [
            Telemetry::Event(SimEvent::FlightLoaded("clean.FLT".into())),
            Telemetry::Event(SimEvent::SimStart),
        ];
        let early = QUIET_AFTER - Duration::from_secs(1);
        assert_eq!(reload(&items, early), None);
        assert_eq!(reload(&items, QUIET_AFTER), Some(Ok(Settled::Quiet)));
    }

    #[test]
    fn a_reload_that_never_loads_times_out() {
        use FlowEvent::*;
        // A flight end alone, or a SimStart before any load, is no load.
        let items = [Telemetry::Event(SimEvent::SimStart), flow(FlightEnd)];
        assert_eq!(reload(&items, QUIET_AFTER), None);
        let failed = reload(&items, RELOAD_TIMEOUT).unwrap().unwrap_err();
        assert!(failed.contains("did not load"), "{failed}");
        let moving = [flow(FltLoaded), flow(RtcStart)];
        let failed = reload(&moving, RELOAD_TIMEOUT).unwrap().unwrap_err();
        assert!(failed.contains("did not finish"), "{failed}");
    }

    #[test]
    fn unrelated_flows_leave_a_reload_waiting() {
        use FlowEvent::*;
        // Before the load a move to the runway is not the reload's.
        let items = [
            flow(RtcStart),
            flow(RtcEnd),
            flow(ReplayStart),
            flow(ReplayEnd),
            flow(TeleportStart),
            flow(TeleportDone),
            flow(PlaneCrash),
        ];
        assert_eq!(reload(&items, Duration::ZERO), None);
        let menu = [flow(FltLoaded), flow(BackToMainMenu)];
        assert!(reload(&menu, Duration::ZERO).unwrap().is_err());
    }
}
