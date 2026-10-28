use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

use fdm_core::{AircraftId, Airport, SimFacts};
use recorder::{Acquisition, Column, FlowEvent, SYSTEM_EVENTS, SimEvent, Telemetry};
use sim_link::facility::{AirportFetch, Fetched};
use sim_link::{Recv, SimConnect};

use crate::place::{AirportAsks, Ask, KEY_EVENT_BASE, KEY_EVENTS, Start, key_event_id};
use crate::session::Action;
use crate::{Polled, Sim};

const DEFINE: u32 = 1;
const FACILITY_DEFINE: u32 = 2;
const IDENTITY_DEFINE: u32 = 3;
const INIT_DEFINE: u32 = 4;
const FACTS_DEFINE: u32 = 5;
const REQUEST: u32 = 1;
const IDENTITY_REQUEST: u32 = 2;
const PROBE_REQUEST: u32 = 3;
const FACTS_REQUEST: u32 = 4;
/// The identification SimVars, in `AircraftId` field order, then the
/// category of `SimFacts` (the one text fact).
const IDENTITY: [&str; 5] = ["TITLE", "ATC MODEL", "ATC TYPE", "ATC ID", "CATEGORY"];
/// Idle wait between empty polls of the SimConnect queue.
const POLL: Duration = Duration::from_millis(1);
/// The longest `poll` waits for an item: a paused sim or a loading screen
/// sends none, and the recorder still acts on requests in the meantime.
const TICK: Duration = Duration::from_millis(250);
/// How often a quiet connection is checked. A crashed or killed sim sends
/// no `Quit`; the next call on its connection fails instead.
const PROBE_EVERY: Duration = Duration::from_secs(2);
/// Airports whose reference point lies this close to the aircraft at a
/// departure or touchdown; covers the gate-to-reference-point distance of
/// the largest airports without reaching far into neighbouring ones.
const AIRPORT_RADIUS_M: f64 = 10_000.0;

/// The live simulator as a telemetry source: one SimConnect data definition
/// holding every acquisition column, streamed each sim frame, plus the sim's
/// own runway data for the airports the flight uses and the aircraft's
/// identity. It also carries out the app's requests: place the aircraft
/// at a start, fetch an airport by ICAO.
pub struct LiveSource {
    sim: SimConnect,
    /// What each definition, mapping or placement call asked for, by send
    /// id, to report a rejected one.
    sent: HashMap<u32, String>,
    trigger: Trigger,
    airports: AirportFetch,
    asks: AirportAsks,
    identity: Identity,
    next_probe: Instant,
}

impl LiveSource {
    pub fn open(acquisition: &Acquisition) -> Result<Self, sim_link::Error> {
        let mut sim = SimConnect::open("FOQA-Sim recorder")?;
        let mut sent = HashMap::new();
        for column in acquisition.columns() {
            let send_id = sim.add_f64(DEFINE, column.simvar, column.unit)?;
            sent.insert(send_id, format!("{} [{}]", column.simvar, column.unit));
        }
        for simvar in IDENTITY {
            sent.insert(sim.add_text(IDENTITY_DEFINE, simvar)?, simvar.to_owned());
        }
        for (simvar, unit) in SimFacts::SIMVARS {
            sent.insert(sim.add_f64(FACTS_DEFINE, simvar, unit)?, simvar.to_owned());
        }
        for (id, name) in (KEY_EVENT_BASE..).zip(KEY_EVENTS) {
            sent.insert(sim.map_event(id, name)?, format!("key event {name}"));
        }
        for (id, name) in (1..).zip(SYSTEM_EVENTS) {
            sim.subscribe_system_event(id, name)?;
        }
        sim.subscribe_flow_events()?;
        sim.request_every_sim_frame(REQUEST, DEFINE)?;
        // The identity changes only when the aircraft is swapped: sent once
        // now and then on change, checked each second, instead of every
        // frame. `Identity` repeats it to each new flight.
        sim.request_on_change(IDENTITY_REQUEST, IDENTITY_DEFINE)?;
        sim.request_on_change(FACTS_REQUEST, FACTS_DEFINE)?;
        let airports = AirportFetch::new(&mut sim, FACILITY_DEFINE)?;
        Ok(Self {
            sim,
            sent,
            trigger: Trigger::new(acquisition.columns()),
            airports,
            asks: AirportAsks::default(),
            identity: Identity::default(),
            next_probe: Instant::now() + PROBE_EVERY,
        })
    }

    /// The next fetched airport as telemetry; progress and failures are logged.
    fn next_airport(&mut self) -> Result<Option<Telemetry>, sim_link::Error> {
        let fetched = self.airports.poll(&mut self.sim, Instant::now())?;
        Ok(match fetched {
            Some(Fetched::Airport(airport)) => {
                eprintln!("airport captured: {}", airport.icao);
                self.asks.fetched(&airport);
                Some(Telemetry::Event(SimEvent::Airport(airport)))
            }
            Some(Fetched::Near(icaos)) if !icaos.is_empty() => {
                eprintln!("fetching airports near the aircraft: {icaos:?}");
                None
            }
            Some(Fetched::Failed { icao, reason }) => {
                eprintln!("airport {icao}: {reason}");
                self.asks.failed(&icao, reason.to_string());
                None
            }
            Some(Fetched::Near(_)) | None => None,
        })
    }

    /// Puts the user aircraft at `start`'s position, then sends its key
    /// events (autopilot off, gear, flaps). Errors are calls that failed at
    /// once; a rejection arrives later as an exception, which is logged.
    fn send_start(&mut self, start: &Start) -> Result<(), sim_link::Error> {
        let send_id = self.sim.set_init_position(INIT_DEFINE, &start.pos)?;
        self.sent.insert(send_id, "the initial position".into());
        for (name, data) in start.events() {
            let id = key_event_id(name).expect("a start sends mapped key events only");
            let send_id = self.sim.transmit_event(id, data)?;
            self.sent.insert(send_id, format!("{name} {data}"));
        }
        Ok(())
    }

    /// One look at the sim: an item, `Ended` once it quit, `None` for
    /// nothing yet (after a short wait if the queue was empty). `Err` once
    /// the connection is gone.
    fn step(&mut self) -> Result<Option<Polled>, sim_link::Error> {
        if let Some(id) = self.identity.next() {
            eprintln!("aircraft: {} ({})", id.title, id.atc_model);
            return Ok(Some(Polled::Item(Telemetry::Event(SimEvent::Aircraft(id)))));
        }
        if let Some(airport) = self.next_airport()? {
            return Ok(Some(Polled::Item(airport)));
        }
        let Some(msg) = self.sim.try_dispatch()? else {
            let now = Instant::now();
            if now >= self.next_probe {
                self.next_probe = now + PROBE_EVERY;
                self.sim.probe(PROBE_REQUEST)?;
            }
            thread::sleep(POLL);
            return Ok(None);
        };
        let item = match msg {
            Recv::Data {
                request_id: FACTS_REQUEST,
                values,
            } => {
                self.identity.facts(&values);
                None
            }
            Recv::Data { values, .. } => Some(Telemetry::Frame(values)),
            Recv::Text { values, .. } => {
                self.identity.report(values);
                None
            }
            Recv::Event { id, data } => system_event(id, data, ""),
            Recv::EventFilename { id, file } => system_event(id, 0, &file),
            Recv::Flow { event, .. } => {
                FlowEvent::from_code(event).map(|f| Telemetry::Event(SimEvent::Flow(f)))
            }
            Recv::Facility(reply) => {
                self.airports.feed(reply);
                None
            }
            Recv::Exception {
                code,
                send_id,
                index,
            } => {
                let what = self
                    .sent
                    .get(&send_id)
                    .map(String::as_str)
                    .or_else(|| self.airports.describe(send_id));
                match what {
                    Some(what) => eprintln!("SimConnect rejected {what} (exception {code})"),
                    None => {
                        eprintln!("SimConnect exception {code} (send {send_id}, index {index})")
                    }
                }
                None
            }
            Recv::Open {
                simconnect_version, ..
            } => {
                eprintln!("connected to MSFS, SimConnect {simconnect_version:?}");
                None
            }
            Recv::Quit => return Ok(Some(Polled::Ended)),
            Recv::CommBus { .. } | Recv::Other { .. } => None,
        };
        Ok(item.map(Polled::Item))
    }
}

impl Sim for LiveSource {
    /// Frames keep flowing while airport requests are outstanding: the fetch
    /// is polled between messages and never waits.
    fn poll(&mut self) -> Polled {
        let tick = Instant::now() + TICK;
        loop {
            match self.step() {
                Ok(Some(polled)) => return polled,
                Ok(None) if Instant::now() >= tick => return Polled::Tick,
                Ok(None) => {}
                Err(e) => {
                    eprintln!("lost the connection to MSFS: {e}");
                    return Polled::Ended;
                }
            }
        }
    }

    fn place(&mut self, start: &Start) -> Result<(), String> {
        self.send_start(start).map_err(|e| e.to_string())
    }

    /// Fetches the airport `ask` names, or answers it from those fetched on
    /// this connection. Like any captured airport it is also recorded,
    /// into the open flight or the next one.
    fn want_airport(&mut self, ask: Ask<String, Airport>) {
        if let Some(icao) = self.asks.ask(ask) {
            self.airports.want(&icao);
        }
    }

    /// Follows the session's routing of an item. Flights are delimited in one
    /// place, `Session`; this only reacts. Closing a flight drops its airport
    /// requests and the set already captured, so the next flight fetches
    /// afresh and no late reply spills into it. Recorded frames drive the
    /// trigger. Opening a flight owes it the aircraft identity. True when
    /// the item is a touchdown frame.
    fn track(&mut self, item: Option<&Telemetry>, action: Action) -> bool {
        if matches!(action, Action::Close | Action::Split) {
            self.airports.reset();
            // Asked airports outlive the flight: ask again.
            for icao in self.asks.waiting() {
                self.airports.want(icao);
            }
        }
        self.identity.follow(action, item);
        let Some(contact) = item.and_then(|item| self.trigger.follow(item, action)) else {
            return false;
        };
        if let Some(pos) = contact.pos {
            self.airports.search(pos, AIRPORT_RADIUS_M);
        }
        contact.touchdown
    }
}

fn system_event(id: u32, data: u32, file: &str) -> Option<Telemetry> {
    let name = SYSTEM_EVENTS.get(usize::try_from(id).ok()?.checked_sub(1)?)?;
    Telemetry::from_system_event(name, data, file)
}

/// The aircraft identity each flight's journal must carry on its own: emitted
/// when it changes (an aircraft swap) and again into every new flight.
/// Emitted items go through `Session` like any event, so one reported
/// between flights is held for the next one and settles what it is owed.
#[derive(Debug, Default)]
struct Identity {
    current: Option<AircraftId>,
    /// The latest sim facts; they arrive apart from the strings.
    facts: SimFacts,
    /// `current` is to be emitted.
    due: bool,
    /// The next flight to open has not been given `current`.
    owed: bool,
}

impl Identity {
    /// The identity SimVars as the sim sent them, in `IDENTITY` order.
    fn report(&mut self, values: Vec<String>) {
        let Ok([title, atc_model, atc_type, atc_id, category]) = <[String; 5]>::try_from(values)
        else {
            return;
        };
        self.facts.category = Some(category).filter(|c| !c.trim().is_empty());
        self.update(AircraftId {
            title,
            atc_model,
            atc_type,
            atc_id,
            facts: self.facts.clone(),
        });
    }

    /// The `SimFacts::SIMVARS` values as the sim sent them. Before the
    /// strings there is no identity to attach them to yet.
    fn facts(&mut self, values: &[f64]) {
        self.facts.from_values(values);
        if let Some(id) = &self.current {
            let id = AircraftId {
                facts: self.facts.clone(),
                ..id.clone()
            };
            self.update(id);
        }
    }

    /// Emits `id` when it differs from the current identity.
    fn update(&mut self, id: AircraftId) {
        if self.current.as_ref() != Some(&id) {
            self.current = Some(id);
            self.due = true;
        }
    }

    /// `item` is the one `action` routed: a split on a new identity holds
    /// it into the next flight, which is then owed nothing.
    fn follow(&mut self, action: Action, item: Option<&Telemetry>) {
        match action {
            Action::Split if matches!(item, Some(Telemetry::Event(SimEvent::Aircraft(_)))) => {}
            Action::Close | Action::Split => self.owed = true,
            Action::Open => self.due |= std::mem::take(&mut self.owed),
            _ => {}
        }
    }

    /// The identity to emit now, if any.
    fn next(&mut self) -> Option<AircraftId> {
        if !std::mem::take(&mut self.due) {
            return None;
        }
        self.owed = false;
        self.current.clone()
    }
}

/// When to capture the airports in use, from the recorded frames: the first
/// frame of a flight if it starts on the ground (departure) and each
/// air-to-ground transition (touchdown). A flight that starts airborne has
/// no departure airport to capture.
#[derive(Debug)]
struct Trigger {
    on_ground: usize,
    lat: usize,
    lon: usize,
    /// Air/ground state of the previous recorded frame; `None` at flight start.
    was_on_ground: Option<bool>,
}

/// A recorded frame that puts the aircraft on the ground.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Contact {
    /// From the air (a touchdown), not the flight's first frame.
    touchdown: bool,
    /// Where to search for airports; `None` without a valid position.
    pos: Option<(f64, f64)>,
}

impl Trigger {
    fn new(columns: &[Column]) -> Self {
        let column = |simvar| {
            columns
                .iter()
                .position(|c| c.simvar == simvar)
                .unwrap_or_else(|| panic!("acquisition does not capture {simvar}"))
        };
        Self {
            on_ground: column("SIM ON GROUND"),
            lat: column("PLANE LATITUDE"),
            lon: column("PLANE LONGITUDE"),
            was_on_ground: None,
        }
    }

    /// The contact this item makes, if it fires the trigger. Only frames
    /// the session records count, never menu or replay frames.
    fn follow(&mut self, item: &Telemetry, action: Action) -> Option<Contact> {
        let Telemetry::Frame(frame) = item else {
            return None;
        };
        match action {
            Action::Open => self.was_on_ground = None,
            Action::Record => {}
            _ => return None,
        }
        let on_ground = *frame.get(self.on_ground)? > 0.5;
        let was_on_ground = self.was_on_ground.replace(on_ground);
        if !on_ground || was_on_ground == Some(true) {
            return None;
        }
        let pos = (*frame.get(self.lat)?, *frame.get(self.lon)?);
        Some(Contact {
            touchdown: was_on_ground == Some(false),
            pos: (pos.0.is_finite() && pos.1.is_finite()).then_some(pos),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLUMNS: [Column; 3] = [
        Column {
            simvar: "PLANE LATITUDE",
            unit: "degrees",
        },
        Column {
            simvar: "PLANE LONGITUDE",
            unit: "degrees",
        },
        Column {
            simvar: "SIM ON GROUND",
            unit: "bool",
        },
    ];

    fn frame(on_ground: bool) -> Telemetry {
        Telemetry::Frame(vec![38.29, 27.16, f64::from(u8::from(on_ground))])
    }

    /// Per item: `None` if it does not fire, else whether it is a touchdown.
    fn fires(items: &[(Telemetry, Action)]) -> Vec<Option<bool>> {
        let mut trigger = Trigger::new(&COLUMNS);
        items
            .iter()
            .map(|(item, action)| Some(trigger.follow(item, *action)?.touchdown))
            .collect()
    }

    #[test]
    fn departure_on_the_first_ground_frame_then_each_touchdown() {
        use Action::*;
        let items = [
            (frame(true), Open),
            (frame(true), Record),
            (frame(false), Record),
            (frame(true), Record),
            (frame(false), Record),
            (frame(true), Record),
        ];
        let (dep, td) = (Some(false), Some(true));
        assert_eq!(fires(&items), [dep, None, None, td, None, td]);
        let mut trigger = Trigger::new(&COLUMNS);
        let contact = trigger.follow(&frame(true), Open).unwrap();
        assert_eq!(contact.pos, Some((38.29, 27.16)));
    }

    #[test]
    fn airborne_start_waits_for_touchdown_and_a_new_flight_rearms() {
        use Action::*;
        let items = [
            (frame(false), Open),
            (frame(true), Record),
            (frame(true), Skip),
            (frame(true), Open),
        ];
        assert_eq!(fires(&items), [None, Some(true), None, Some(false)]);
    }

    #[test]
    fn unrecorded_frames_and_events_do_not_fire() {
        use Action::*;
        let items = [
            (frame(false), Open),
            (frame(true), Skip),
            (frame(true), Hold),
            (Telemetry::Pause(true), Record),
            (frame(false), Record),
        ];
        assert_eq!(fires(&items), [None; 5]);
    }

    fn reported(title: &str) -> Vec<String> {
        [
            title,
            "ATCCOM.AC_MODEL C172.0.text",
            "ATCCOM.ATC_NAME CESSNA.0.text",
            "BILGE",
            "Airplane",
        ]
        .map(String::from)
        .into()
    }

    #[test]
    fn identity_is_emitted_on_change_only() {
        let mut identity = Identity::default();
        assert_eq!(identity.next(), None);
        identity.report(reported("C172SP G1000 Passengers"));
        assert_eq!(identity.next().unwrap().atc_id, "BILGE");
        identity.report(reported("C172SP G1000 Passengers"));
        assert_eq!(identity.next(), None);
        identity.report(reported("C172SP Classic"));
        assert_eq!(identity.next().unwrap().title, "C172SP Classic");
        identity.report(vec!["too short".into()]);
        assert_eq!(identity.next(), None);
    }

    #[test]
    fn sim_facts_join_the_identity_as_they_arrive() {
        let mut identity = Identity::default();
        // Facts before the strings wait for them.
        identity.facts(&[0.0, 1.0, 0.0, 0.0, 0.0, 2550.0]);
        assert_eq!(identity.next(), None);
        identity.report(reported("C172SP G1000 Passengers"));
        let facts = identity.next().unwrap().facts;
        assert_eq!(facts.category.as_deref(), Some("Airplane"));
        assert_eq!(
            (facts.engine_type, facts.max_gross_lb),
            (Some(0), Some(2550))
        );
        // A change of a fact alone is emitted; the same facts are not.
        identity.facts(&[0.0, 1.0, 0.0, 0.0, 0.0, 2550.0]);
        assert_eq!(identity.next(), None);
        identity.facts(&[0.0, 1.0, 0.0, 0.0, 0.0, 2600.0]);
        assert_eq!(identity.next().unwrap().facts.max_gross_lb, Some(2600));
    }

    #[test]
    fn every_new_flight_gets_the_identity_once() {
        use Action::*;
        let mut identity = Identity::default();
        identity.report(reported("C172SP G1000 Passengers"));
        // Emitted before the first flight: held into it, nothing owed.
        assert!(identity.next().is_some());
        identity.follow(Open, None);
        assert_eq!(identity.next(), None);
        // The next flight with the same aircraft gets it repeated.
        identity.follow(Close, None);
        identity.follow(Open, None);
        assert!(identity.next().is_some());
        identity.follow(Record, None);
        assert_eq!(identity.next(), None);
        // A swap between flights is held into the next one: no repeat.
        identity.follow(Close, None);
        identity.report(reported("C172SP Classic"));
        assert!(identity.next().is_some());
        identity.follow(Open, None);
        assert_eq!(identity.next(), None);
    }

    #[test]
    fn missing_position_leaves_nowhere_to_search() {
        let mut trigger = Trigger::new(&COLUMNS);
        let frame = Telemetry::Frame(vec![f64::NAN, 27.16, 1.0]);
        let contact = trigger.follow(&frame, Action::Open).unwrap();
        assert_eq!(contact.pos, None);
    }
}
