use std::collections::HashMap;
use std::time::{Duration, Instant};

use fdm_core::{AircraftId, Airport};
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
const REQUEST: u32 = 1;
const IDENTITY_REQUEST: u32 = 2;
const PROBE_REQUEST: u32 = 3;
/// The identification SimVars, in `AircraftId` field order.
const IDENTITY: [&str; 4] = ["TITLE", "ATC MODEL", "ATC TYPE", "ATC ID"];
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
/// Every frame is streamed on the ground and below `NEAR_FT` radio height,
/// fewer only above `FAR_FT`: the band between keeps a level-off near it
/// from switching back and forth. Descending at 3000 fpm, `NEAR_FT` is
/// 40 s before touchdown, well before the FDAU's 10 s pre-touchdown buffer
/// (`TOUCHDOWN_WINDOW_US`), so that buffer and the window hold every frame.
const NEAR_FT: f64 = 2000.0;
const FAR_FT: f64 = 2500.0;
/// Away from the ground frames still come at this rate in sim time: twice
/// the fastest LFL v1 rate (8 Hz), so each FDAU slot takes a value at most
/// 62.5 ms old.
const FAR_MIN_HZ: f64 = 16.0;
/// Fewer frames are taken only with this much frame rate to spare, so a
/// frame rate near a step does not switch at every measurement.
const FAR_SLOWER_MARGIN: f64 = 1.2;
/// A fast sim still sends one frame in eight.
const FAR_MAX_SKIP: u32 = 7;
/// Sim seconds of frames the frame rate is measured over.
const CADENCE_WINDOW_S: f64 = 1.0;

/// The live simulator as a telemetry source: one SimConnect data definition
/// holding every acquisition column, streamed each sim frame near the
/// ground and fewer away from it (`Cadence`), plus the sim's
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
    cadence: Cadence,
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
        for (id, name) in (KEY_EVENT_BASE..).zip(KEY_EVENTS) {
            sent.insert(sim.map_event(id, name)?, format!("key event {name}"));
        }
        for (id, name) in (1..).zip(SYSTEM_EVENTS) {
            sim.subscribe_system_event(id, name)?;
        }
        sim.subscribe_flow_events()?;
        sim.request_sim_frames(REQUEST, DEFINE, 0)?;
        // The identity changes only when the aircraft is swapped: sent once
        // now and then on change, checked each second, instead of every
        // frame. `Identity` repeats it to each new flight.
        sim.request_on_change(IDENTITY_REQUEST, IDENTITY_DEFINE)?;
        let airports = AirportFetch::new(&mut sim, FACILITY_DEFINE)?;
        Ok(Self {
            sim,
            sent,
            trigger: Trigger::new(acquisition.columns()),
            airports,
            asks: AirportAsks::default(),
            identity: Identity::default(),
            cadence: Cadence::new(acquisition.columns()),
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
    /// nothing yet. `Err` once the connection is gone.
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
            return Ok(None);
        };
        let item = match msg {
            Recv::Data { values, .. } => {
                if let Some(skip) = self.cadence.follow(&values) {
                    self.sim.request_sim_frames(REQUEST, DEFINE, skip)?;
                }
                Some(Telemetry::Frame(values))
            }
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
    /// is polled between messages and never waits. An empty queue is waited
    /// on until a message arrives, the tick or the next probe.
    fn poll(&mut self) -> Polled {
        let tick = Instant::now() + TICK;
        loop {
            match self.step() {
                Ok(Some(polled)) => return polled,
                Ok(None) => {
                    let now = Instant::now();
                    if now >= tick {
                        return Polled::Tick;
                    }
                    let until = tick.min(self.next_probe);
                    self.sim.wait(until.saturating_duration_since(now));
                }
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

/// How many sim frames to skip between streamed frames. Near the ground
/// (on it, below `NEAR_FT` radio height, or either unknown) none: the FDAU
/// records the touchdown window at frame rate and the landing is graded
/// from it. Away from it the FDAU keeps no parameter faster than 8 Hz, so
/// frames are skipped down to `FAR_MIN_HZ`, measured in sim time: a lower
/// frame rate or a faster sim rate skips fewer.
#[derive(Debug)]
struct Cadence {
    sim_time: usize,
    on_ground: usize,
    radio_ft: usize,
    near: bool,
    skip: u32,
    window: Option<Window>,
}

/// Frames counted for the frame rate: those after `from` whose sim time
/// moved on, up to `last`.
#[derive(Debug, Clone, Copy)]
struct Window {
    from: f64,
    last: f64,
    frames: u32,
}

impl Cadence {
    fn new(columns: &[Column]) -> Self {
        Self {
            sim_time: column(columns, "SIMULATION TIME"),
            on_ground: column(columns, "SIM ON GROUND"),
            radio_ft: column(columns, "RADIO HEIGHT"),
            near: true,
            skip: 0,
            window: None,
        }
    }

    /// Follows one streamed frame: the frames to skip from now on, when
    /// that changes.
    fn follow(&mut self, frame: &[f64]) -> Option<u32> {
        let value = |i: usize| frame.get(i).copied().filter(|v| v.is_finite());
        let airborne = value(self.on_ground).is_some_and(|v| v < 0.5);
        let bound = if self.near { FAR_FT } else { NEAR_FT };
        self.near = !(airborne && value(self.radio_ft).is_some_and(|ft| ft > bound));
        let skip = if self.near {
            self.window = None;
            0
        } else {
            self.measure(value(self.sim_time)?)?
        };
        if skip == self.skip {
            return None;
        }
        (self.skip, self.window) = (skip, None);
        Some(skip)
    }

    /// The skip for the frame rate once a window of it is measured.
    fn measure(&mut self, t: f64) -> Option<u32> {
        let mut w = self.window.take().unwrap_or(Window {
            from: t,
            last: t,
            frames: 0,
        });
        // Sim time went back (a reload): measure afresh.
        if t < w.last {
            return None;
        }
        if t > w.last {
            (w.last, w.frames) = (t, w.frames + 1);
        }
        let span = t - w.from;
        if span < CADENCE_WINDOW_S {
            self.window = Some(w);
            return None;
        }
        // Frames per sim second, had none been skipped.
        let full_hz = f64::from(w.frames) / span * f64::from(self.skip + 1);
        let skip_at = |hz: f64| ((full_hz / hz) as u32).clamp(1, FAR_MAX_SKIP + 1) - 1;
        Some(match skip_at(FAR_MIN_HZ) {
            fewer if fewer <= self.skip => fewer,
            _ => skip_at(FAR_MIN_HZ * FAR_SLOWER_MARGIN).max(self.skip),
        })
    }
}

/// Where `simvar` is in `columns`; the acquisition always captures it.
fn column(columns: &[Column], simvar: &str) -> usize {
    columns
        .iter()
        .position(|c| c.simvar == simvar)
        .unwrap_or_else(|| panic!("acquisition does not capture {simvar}"))
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
        Self {
            on_ground: column(columns, "SIM ON GROUND"),
            lat: column(columns, "PLANE LATITUDE"),
            lon: column(columns, "PLANE LONGITUDE"),
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

    fn cadence() -> Cadence {
        Cadence::new(Acquisition::new(&fdm_core::Lfl::V1).columns())
    }

    /// An airborne frame at sim time `t` and `radio_ft`.
    fn aloft(t: f64, radio_ft: f64) -> Vec<f64> {
        let columns = Acquisition::new(&fdm_core::Lfl::V1).columns().to_vec();
        let mut frame = vec![f64::NAN; columns.len()];
        frame[column(&columns, "SIMULATION TIME")] = t;
        frame[column(&columns, "SIM ON GROUND")] = 0.0;
        frame[column(&columns, "RADIO HEIGHT")] = radio_ft;
        frame
    }

    /// Frames streamed at `hz` from sim time `t0` to `t0 + secs`, at
    /// `radio_ft`: the skips asked for.
    fn streamed(c: &mut Cadence, t0: f64, secs: f64, hz: f64, radio_ft: f64) -> Vec<u32> {
        (0..=(secs * hz) as u32)
            .filter_map(|k| c.follow(&aloft(t0 + f64::from(k) / hz, radio_ft)))
            .collect()
    }

    #[test]
    fn away_from_the_ground_frames_are_skipped_down_to_the_minimum_rate() {
        let mut c = cadence();
        // 60 fps at 5000 ft: two in three skipped, 20 Hz.
        assert_eq!(streamed(&mut c, 0.0, 1.0, 60.0, 5000.0), [2]);
        // Then 20 Hz arrive, as asked: no change.
        assert!(streamed(&mut c, 1.0, 3.0, 20.0, 5000.0).is_empty());
        // The sim slows to 30 fps: 10 Hz arrive, too few. None skipped.
        assert_eq!(streamed(&mut c, 4.0, 2.0, 10.0, 5000.0), [0]);
        // 35 fps would allow one in two (17.5 Hz), not with the margin.
        assert!(streamed(&mut c, 6.0, 3.0, 35.0, 5000.0).is_empty());
        // A fast sim: one in eight at most.
        assert_eq!(streamed(&mut c, 9.0, 2.0, 400.0, 5000.0), [7]);
    }

    #[test]
    fn near_the_ground_every_frame_is_streamed_with_hysteresis() {
        let mut c = cadence();
        assert_eq!(streamed(&mut c, 0.0, 1.0, 60.0, 3000.0), [2]);
        // Under 2000 ft at once, and not again above it before 2500 ft.
        assert_eq!(streamed(&mut c, 1.0, 0.1, 20.0, 1999.0), [0]);
        assert!(streamed(&mut c, 2.0, 3.0, 60.0, 2400.0).is_empty());
        assert_eq!(streamed(&mut c, 5.0, 1.0, 60.0, 2600.0), [2]);
        // An unknown height, or on the ground: every frame.
        assert_eq!(streamed(&mut c, 6.0, 0.1, 20.0, f64::NAN), [0]);
        assert_eq!(streamed(&mut c, 7.0, 1.0, 60.0, 3000.0), [2]);
        let mut on_ground = aloft(8.0, 3000.0);
        on_ground[c.on_ground] = 1.0;
        assert_eq!(c.follow(&on_ground), Some(0));
    }

    #[test]
    fn frames_whose_sim_time_stands_still_are_not_counted() {
        let mut c = cadence();
        // Paused at 5000 ft: 60 fps of one sim time.
        for _ in 0..300 {
            assert_eq!(c.follow(&aloft(1.0, 5000.0)), None);
        }
        assert_eq!(streamed(&mut c, 1.0, 1.0, 60.0, 5000.0), [2]);
    }

    /// `testkit`'s 60 Hz frames recorded through the on-board chain, every
    /// frame (`thin` false) or as `Cadence` asks the sim to stream them,
    /// the skip applying from the next frame.
    fn fly(thin: bool, script: impl Fn(f64) -> testkit::Pt) -> (analyzer::Flight, usize) {
        let mut recorder = recorder::Recorder::new(&fdm_core::Lfl::V1, Vec::new()).unwrap();
        let (mut c, mut skip, mut since, mut frames) = (cadence(), 0, 0, 0);
        testkit::telemetry(&[], testkit::AIRLINER_LB, 700.0, script, |item| {
            if let Telemetry::Frame(values) = &item {
                since += 1;
                if since <= skip {
                    return;
                }
                since = 0;
                frames += 1;
                if let Some(s) = c.follow(values).filter(|_| thin) {
                    skip = s;
                }
            }
            recorder.record(&item).unwrap();
        });
        (
            analyzer::Flight::read(&recorder.into_inner()[..]).unwrap(),
            frames,
        )
    }

    #[test]
    fn landings_are_graded_alike_with_fewer_frames_away_from_the_ground() {
        use testkit::{APPROACH_S, landing, lerp};
        // 8000 ft down to the 1500 ft approach, then a touchdown at
        // 600 s out of a curved flare, so the sink depends on every frame.
        let td = 600.0;
        let script = |t: f64| {
            let mut p = landing(t, td, 1.7);
            if t < td - APPROACH_S {
                p.alt = lerp(&[(0.0, 8000.0), (td - APPROACH_S, 1500.0)], t);
            } else if (td - 2.0..td).contains(&t) {
                p.alt = 10.0 * ((td - t) / 2.0).powi(2);
            }
            p
        };
        let (full, all) = fly(false, script);
        let (thinned, streamed) = fly(true, script);
        assert!(streamed < all * 2 / 3, "{streamed} of {all} frames");
        let graded = |f: &analyzer::Flight| {
            let tds = analyzer::touchdowns(f, &analyzer::Track::new(f).unwrap());
            let at = |name, t| f.at(name, t);
            let pos: Vec<_> = tds
                .iter()
                .map(|d| (at("pos.lat", d.t), at("pos.lon", d.t)))
                .collect();
            (tds, pos)
        };
        let (tds, pos) = graded(&full);
        assert_eq!(tds.len(), 1);
        assert!(tds[0].peak_g > 1.5 && tds[0].sink_fpm > 0.0, "{tds:?}");
        assert_eq!(graded(&thinned), (tds, pos));
    }
}
