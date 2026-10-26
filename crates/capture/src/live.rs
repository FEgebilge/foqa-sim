use std::collections::HashMap;
use std::io;
use std::thread;
use std::time::{Duration, Instant};

use fdm_core::AircraftId;
use recorder::{
    Acquisition, Column, FlowEvent, SYSTEM_EVENTS, SimEvent, Telemetry, TelemetrySource,
};
use sim_link::facility::{AirportFetch, Fetched};
use sim_link::{Recv, SimConnect};

use crate::session::Action;

const DEFINE: u32 = 1;
const FACILITY_DEFINE: u32 = 2;
const IDENTITY_DEFINE: u32 = 3;
const REQUEST: u32 = 1;
const IDENTITY_REQUEST: u32 = 2;
/// The identification SimVars, in `AircraftId` field order.
const IDENTITY: [&str; 4] = ["TITLE", "ATC MODEL", "ATC TYPE", "ATC ID"];
/// Idle wait between empty polls of the SimConnect queue.
const POLL: Duration = Duration::from_millis(1);
/// Airports whose reference point lies this close to the aircraft at a
/// departure or touchdown; covers the gate-to-reference-point distance of
/// the largest airports without reaching far into neighbouring ones.
const AIRPORT_RADIUS_M: f64 = 10_000.0;

/// The live simulator as a telemetry source: one SimConnect data definition
/// holding every acquisition column, streamed each sim frame, plus the sim's
/// own runway data for the airports the flight uses and the aircraft's
/// identity.
pub struct LiveSource {
    sim: SimConnect,
    /// SimVar named by the send id of its definition call, to report a
    /// rejected one.
    simvars_by_send_id: HashMap<u32, String>,
    trigger: Trigger,
    airports: AirportFetch,
    identity: Identity,
}

impl LiveSource {
    pub fn open(acquisition: &Acquisition) -> Result<Self, sim_link::Error> {
        let mut sim = SimConnect::open("FOQA-Sim recorder")?;
        let mut simvars_by_send_id = HashMap::new();
        for column in acquisition.columns() {
            let send_id = sim.add_f64(DEFINE, column.simvar, column.unit)?;
            simvars_by_send_id.insert(send_id, format!("{} [{}]", column.simvar, column.unit));
        }
        for simvar in IDENTITY {
            simvars_by_send_id.insert(sim.add_text(IDENTITY_DEFINE, simvar)?, simvar.to_owned());
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
        let airports = AirportFetch::new(&mut sim, FACILITY_DEFINE)?;
        Ok(Self {
            sim,
            simvars_by_send_id,
            trigger: Trigger::new(acquisition.columns()),
            airports,
            identity: Identity::default(),
        })
    }

    /// Follows the session's routing of an item. Flights are delimited in one
    /// place, `Session`; this only reacts. Closing a flight drops its airport
    /// requests and the set already captured, so the next flight fetches
    /// afresh and no late reply spills into it. Recorded frames drive the
    /// trigger. Opening a flight owes it the aircraft identity. True when
    /// the item is a touchdown frame.
    pub fn track(&mut self, item: Option<&Telemetry>, action: Action) -> bool {
        if action == Action::Close {
            self.airports.reset();
        }
        self.identity.follow(action);
        let Some(contact) = item.and_then(|item| self.trigger.follow(item, action)) else {
            return false;
        };
        if let Some(pos) = contact.pos {
            self.airports.search(pos, AIRPORT_RADIUS_M);
        }
        contact.touchdown
    }

    /// The next fetched airport as telemetry; progress and failures are logged.
    fn next_airport(&mut self) -> io::Result<Option<Telemetry>> {
        let fetched = self
            .airports
            .poll(&mut self.sim, Instant::now())
            .map_err(io::Error::other)?;
        Ok(match fetched {
            Some(Fetched::Airport(airport)) => {
                eprintln!("airport captured: {}", airport.icao);
                Some(Telemetry::Event(SimEvent::Airport(airport)))
            }
            Some(Fetched::Near(icaos)) if !icaos.is_empty() => {
                eprintln!("fetching airports near the aircraft: {icaos:?}");
                None
            }
            Some(Fetched::Failed { icao, reason }) => {
                eprintln!("airport {icao}: {reason}");
                None
            }
            Some(Fetched::Near(_)) | None => None,
        })
    }
}

impl TelemetrySource for LiveSource {
    /// Frames keep flowing while airport requests are outstanding: the fetch
    /// is polled between messages and never waits.
    fn recv(&mut self) -> io::Result<Option<Telemetry>> {
        loop {
            if let Some(id) = self.identity.next() {
                eprintln!("aircraft: {} ({})", id.title, id.atc_model);
                return Ok(Some(Telemetry::Event(SimEvent::Aircraft(id))));
            }
            if let Some(airport) = self.next_airport()? {
                return Ok(Some(airport));
            }
            let Some(msg) = self.sim.dispatch() else {
                thread::sleep(POLL);
                continue;
            };
            let item = match msg {
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
                        .simvars_by_send_id
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
                Recv::Quit => return Ok(None),
                Recv::CommBus { .. } | Recv::Other { .. } => None,
            };
            if item.is_some() {
                return Ok(item);
            }
        }
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
    /// `current` is to be emitted.
    due: bool,
    /// The next flight to open has not been given `current`.
    owed: bool,
}

impl Identity {
    /// The identity SimVars as the sim sent them, in `IDENTITY` order.
    fn report(&mut self, values: Vec<String>) {
        let Ok([title, atc_model, atc_type, atc_id]) = <[String; 4]>::try_from(values) else {
            return;
        };
        let id = AircraftId {
            title,
            atc_model,
            atc_type,
            atc_id,
        };
        if self.current.as_ref() != Some(&id) {
            self.current = Some(id);
            self.due = true;
        }
    }

    fn follow(&mut self, action: Action) {
        match action {
            Action::Close => self.owed = true,
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
    fn every_new_flight_gets_the_identity_once() {
        use Action::*;
        let mut identity = Identity::default();
        identity.report(reported("C172SP G1000 Passengers"));
        // Emitted before the first flight: held into it, nothing owed.
        assert!(identity.next().is_some());
        identity.follow(Open);
        assert_eq!(identity.next(), None);
        // The next flight with the same aircraft gets it repeated.
        identity.follow(Close);
        identity.follow(Open);
        assert!(identity.next().is_some());
        identity.follow(Record);
        assert_eq!(identity.next(), None);
        // A swap between flights is held into the next one: no repeat.
        identity.follow(Close);
        identity.report(reported("C172SP Classic"));
        assert!(identity.next().is_some());
        identity.follow(Open);
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
