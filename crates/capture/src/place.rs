//! Requests the desktop app makes of the simulator through the running
//! service: put the user aircraft at a start (a landing challenge on final,
//! airborne) and configure it, or fetch one airport's runways. Each request
//! carries the way its answer goes back, so the caller can wait on it.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use fdm_core::Airport;
pub use sim_link::InitPosition;

/// Key events a start sends; each is mapped once per connection as client
/// event `KEY_EVENT_BASE` + its index. Names from the MSFS key event list
/// (legacy events, still in MSFS 2024).
pub(crate) const KEY_EVENTS: [&str; 4] = ["AUTOPILOT_OFF", "GEAR_DOWN", "GEAR_UP", "FLAPS_SET"];
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
}

impl Start {
    /// The key events sent after the position, in order, with their data:
    /// autopilot off, the gear, then the flaps if set.
    pub fn events(&self) -> Vec<(&'static str, u32)> {
        let gear = if self.gear_down {
            "GEAR_DOWN"
        } else {
            "GEAR_UP"
        };
        let mut events = vec![("AUTOPILOT_OFF", 0), (gear, 0)];
        events.extend(self.flaps.map(|f| ("FLAPS_SET", flaps_set_data(f))));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn start(gear_down: bool, flaps: Option<f64>) -> Start {
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
    fn start_events_are_ap_off_gear_then_flaps() {
        let events = start(true, Some(1.0)).events();
        assert_eq!(
            events,
            [("AUTOPILOT_OFF", 0), ("GEAR_DOWN", 0), ("FLAPS_SET", 16383)]
        );
        assert_eq!(
            start(false, None).events(),
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
        let (ask, answer) = Ask::<_, ()>::new(start(true, None));
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
}
