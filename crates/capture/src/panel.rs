//! The in-sim panel feed: what the service reports, mirrored over the
//! SimConnect Comm Bus to the sim's JavaScript views (the FOQA-Sim toolbar
//! panel, later an EFB app), so the pilot sees a landing without leaving
//! the sim, and the views' commands taken back. Messages and payloads:
//! `docs/ingame-panel.md`.
//!
//! A view hears only what is sent while it is open, so the feed repeats:
//! the status each second, the live values while recording, the flight's
//! events and landings every few seconds, a reply to a query for a few
//! seconds.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use debrief::{Event, FlightSummary, LandingNotice, RecorderStatus};
use serde::Serialize;
use serde_json::{Value, json};
use sim_link::{Recv, SimConnect};

use crate::Happening;
use crate::history::{self, Ask, History, Query, REQUEST_MAX_BYTES, Reply};
use crate::monitor::Live;

pub const STATUS: &str = "FOQA_SIM_STATUS";
pub const LIVE: &str = "FOQA_SIM_LIVE";
pub const EVENTS: &str = "FOQA_SIM_EVENTS";
pub const LANDING: &str = "FOQA_SIM_LANDING";
pub const LANDINGS: &str = "FOQA_SIM_LANDINGS";
pub const FLIGHTS: &str = "FOQA_SIM_FLIGHTS";
pub const DEBRIEF: &str = "FOQA_SIM_DEBRIEF";
/// App to the EFB: set the weather (`WeatherSpec`); the EFB answers on `CMD`.
#[allow(dead_code)] // not wired into a flow yet
pub const WEATHER: &str = "FOQA_SIM_WEATHER";
/// View to app.
pub const CMD: &str = "FOQA_SIM_CMD";
/// Payload version, `v` in every message.
pub(crate) const VERSION: u32 = 2;
const STATUS_EVERY: Duration = Duration::from_secs(1);
const REPEAT_EVERY: Duration = Duration::from_secs(5);
/// A query's reply goes out this many times, a second apart.
const REPLY_TIMES: u32 = 5;
const REPLY_EVERY: Duration = Duration::from_secs(1);
/// Landings in `FOQA_SIM_LANDINGS`.
const LANDINGS_SENT: usize = 3;
/// Client event id of the `CMD` subscription.
const CMD_EVENT_ID: u32 = 1;
/// A longer pilot-report note is cut (a journal packet holds 64 KiB).
const NOTE_MAX_BYTES: usize = 1000;
/// Wait between attempts to open the feed's own SimConnect connection.
const RECONNECT: Duration = Duration::from_secs(5);
const TICK: Duration = Duration::from_millis(250);

/// What the feed shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    /// Also passed on to the app.
    Happening(Happening),
    Live(Live),
    Snapshot(Snapshot),
    /// The answer to a view's command.
    Ack(Ack),
}

impl From<Happening> for Msg {
    fn from(h: Happening) -> Self {
        Self::Happening(h)
    }
}

/// The flight being recorded, analysed as it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub summary: FlightSummary,
    pub events: Vec<Event>,
    /// In time order.
    pub landings: Vec<LandingNotice>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ack {
    pub cmd: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Ack {
    pub fn new(cmd: &str, result: Result<(), String>) -> Self {
        Self {
            cmd: cmd.to_owned(),
            ok: result.is_ok(),
            message: result.err(),
        }
    }
}

/// A command from a view (`FOQA_SIM_CMD`).
#[derive(Debug, Clone, PartialEq)]
pub enum ViewCommand {
    /// Record a new flight while no flight is open.
    StartFlight,
    EndFlight,
    DiscardFlight,
    Mark(String),
}

impl ViewCommand {
    pub fn name(&self) -> &'static str {
        match self {
            Self::StartFlight => "start_flight",
            Self::EndFlight => "end_flight",
            Self::DiscardFlight => "discard_flight",
            Self::Mark(_) => "mark",
        }
    }
}

/// What a view sent.
#[derive(Debug, Clone, PartialEq)]
enum Request {
    Command(ViewCommand),
    /// Answered on its own event, not in `Status.ack`.
    Query(Query),
}

impl Request {
    /// From the view's JSON, with or without trailing NULs; a bad command
    /// is answered at once.
    fn parse(data: &str) -> Result<Self, Ack> {
        let data = data.trim_end_matches('\0');
        let v: Value =
            serde_json::from_str(data).map_err(|e| Ack::new("?", Err(format!("not JSON: {e}"))))?;
        let cmd = v["cmd"].as_str().unwrap_or("?");
        let fail = |m: &str| Ack::new(cmd, Err(m.to_owned()));
        let text = |key: &str| v[key].as_str().unwrap_or_default().to_owned();
        let ask = match cmd {
            "list_flights" => Some(Ask::Flights),
            "debrief" => Some(Ask::Debrief(text("id"))),
            _ => None,
        };
        if let Some(ask) = ask {
            let req = text("req");
            if req.len() + text("id").len() > REQUEST_MAX_BYTES {
                return Err(fail("request too long"));
            }
            let ask = if v["v"] == VERSION {
                ask
            } else {
                Ask::Refused("unsupported version")
            };
            return Ok(Self::Query(Query { req, ask }));
        }
        if v["v"] != VERSION {
            return Err(fail("unsupported version"));
        }
        Ok(Self::Command(match cmd {
            "start_flight" => ViewCommand::StartFlight,
            "end_flight" => ViewCommand::EndFlight,
            "discard_flight" => ViewCommand::DiscardFlight,
            "mark" => {
                let mut note = v["note"].as_str().unwrap_or_default().to_owned();
                let mut end = note.len().min(NOTE_MAX_BYTES);
                while !note.is_char_boundary(end) {
                    end -= 1;
                }
                note.truncate(end);
                ViewCommand::Mark(note)
            }
            _ => return Err(fail("unknown command")),
        }))
    }
}

/// Largest message: 6 KB as UTF-8 JSON, the terminating NUL included.
#[allow(dead_code)]
const MESSAGE_MAX_BYTES: usize = 6 * 1024 - 1;

/// The base preset of a `WeatherSpec`: `"clear"` or a preset index.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum WeatherBase {
    Index(u32),
    Name(String),
}

/// One wind layer: where the wind blows from, no gusts.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct WindLayer {
    pub alt_ft: f64,
    pub from_deg: f64,
    pub kt: f64,
}

/// What the EFB sets (`set_weather`, docs/ingame-panel.md): a preset with
/// its wind layers, QNH and temperature replaced.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct WeatherSpec {
    pub base: WeatherBase,
    pub wind: Vec<WindLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qnh_hpa: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp_c: Option<f64>,
}

#[allow(dead_code)]
impl WeatherSpec {
    pub fn validate(&self) -> Result<(), String> {
        if let WeatherBase::Name(n) = &self.base
            && n != "clear"
        {
            return Err(format!("unknown base preset \"{n}\""));
        }
        if self.wind.is_empty() || self.wind.len() > 8 {
            return Err("wind needs 1 to 8 layers".into());
        }
        for w in &self.wind {
            let ok = w.alt_ft.is_finite()
                && (-1000.0..=60000.0).contains(&w.alt_ft)
                && (0.0..=360.0).contains(&w.from_deg)
                && (0.0..=150.0).contains(&w.kt);
            if !ok {
                return Err("wind layer out of range".into());
            }
        }
        if self.qnh_hpa.is_some_and(|q| !(850.0..=1085.0).contains(&q)) {
            return Err("qnh_hpa out of range".into());
        }
        if self.temp_c.is_some_and(|t| !(-80.0..=60.0).contains(&t)) {
            return Err("temp_c out of range".into());
        }
        Ok(())
    }

    /// The `FOQA_SIM_WEATHER` message for request `req`, within the size cap.
    pub fn message(&self, req: &str) -> Result<String, String> {
        self.validate()?;
        let data =
            json!({ "v": VERSION, "cmd": "set_weather", "req": req, "spec": self }).to_string();
        if data.len() > MESSAGE_MAX_BYTES {
            return Err("message too long".into());
        }
        Ok(data)
    }
}

/// Values the EFB read back (the preset) or measured (SimVars).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Default, Serialize, serde::Deserialize)]
pub struct WeatherReading {
    #[serde(default)]
    pub wind: Vec<WindLayer>,
    pub qnh_hpa: Option<f64>,
    pub temp_c: Option<f64>,
    /// Measured only: the ambient wind at the aircraft.
    pub wind_dir_deg: Option<f64>,
    pub wind_kt: Option<f64>,
    pub alt_ft: Option<f64>,
}

/// The EFB's answer to `set_weather` (`FOQA_SIM_CMD`, `cmd: "weather_result"`).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct WeatherResult {
    pub req: String,
    pub ok: bool,
    pub applied: Option<WeatherReading>,
    pub measured: Option<WeatherReading>,
    #[serde(default)]
    pub mismatches: Vec<String>,
    pub error: Option<String>,
}

#[allow(dead_code)]
impl WeatherResult {
    /// From the view's JSON, with or without trailing NULs.
    pub fn parse(data: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(data.trim_end_matches(' '))
            .map_err(|e| format!("not JSON: {e}"))?;
        if v["v"] != VERSION {
            return Err("unsupported version".into());
        }
        if v["cmd"] != "weather_result" {
            return Err("not a weather_result".into());
        }
        serde_json::from_value(v).map_err(|e| e.to_string())
    }
}

/// Forwards happenings to `out`, mirrors them into the sim panel on its own
/// thread and SimConnect connection, passes view commands to `commands` and
/// has `history` answer the views' queries. Ends when every sender of the
/// returned channel is gone.
pub(crate) fn tee(
    out: Sender<Happening>,
    commands: Sender<ViewCommand>,
    mut history: History,
) -> Sender<Msg> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let (mut board, mut link) = (Board::default(), Link::default());
        loop {
            match rx.recv_timeout(TICK) {
                Ok(msg) => {
                    board.take(&msg, Instant::now());
                    if let Msg::Happening(h) = msg {
                        // The receiver is gone only when the app is closing.
                        let _ = out.send(h);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            let now = Instant::now();
            for reply in history.poll() {
                board.reply(reply, now);
            }
            for data in link.send(board.due(now), board.sim_up(), now) {
                match Request::parse(&data) {
                    Ok(Request::Query(query)) => history.ask(query),
                    Ok(Request::Command(cmd))
                        if for_recorder(&cmd, board.recording().is_some()) =>
                    {
                        let _ = commands.send(cmd);
                    }
                    Ok(Request::Command(cmd)) => {
                        let ack = Ack::new(cmd.name(), Err("no flight open".into()));
                        board.take(&Msg::Ack(ack), now);
                    }
                    Err(ack) => board.take(&Msg::Ack(ack), now),
                }
            }
        }
    });
    tx
}

/// Whether a view command goes to the recorder: with a flight open any
/// does; without one only a start, the rest having nothing to act on.
fn for_recorder(cmd: &ViewCommand, recording: bool) -> bool {
    recording || *cmd == ViewCommand::StartFlight
}

/// `Status.flight`.
#[derive(Debug, Serialize)]
struct FlightInfo<'a> {
    id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    aircraft: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    departure: Option<&'a str>,
    elapsed_s: f64,
    phase: &'static str,
    landings: usize,
}

/// What the panel is to show, and when each message is next due.
#[derive(Debug, Default)]
struct Board {
    status: Option<RecorderStatus>,
    /// The last landing and when it arrived.
    landing: Option<(LandingNotice, Instant)>,
    /// Not yet sent.
    live: Option<Live>,
    phase: Option<&'static str>,
    /// Of the flight being recorded.
    snapshot: Option<Snapshot>,
    /// Sent once, with the next status.
    ack: Option<Ack>,
    /// The latest reply on each event, while it repeats.
    replies: Vec<Repeat>,
    /// `None`: due now.
    next_status: Option<Instant>,
    next_landing: Option<Instant>,
    next_events: Option<Instant>,
    next_landings: Option<Instant>,
}

/// A reply, the times it is still to go out and when next.
#[derive(Debug)]
struct Repeat {
    reply: Reply,
    left: u32,
    next: Instant,
}

impl Board {
    /// Replaces an earlier reply on the same event.
    fn reply(&mut self, reply: Reply, now: Instant) {
        self.replies.retain(|r| r.reply.event != reply.event);
        self.replies.push(Repeat {
            reply,
            left: REPLY_TIMES,
            next: now,
        });
    }

    fn take(&mut self, msg: &Msg, now: Instant) {
        match msg {
            Msg::Happening(Happening::Status(status)) => {
                self.status = Some(status.clone());
                if self.recording().is_none() {
                    (self.live, self.phase, self.snapshot) = (None, None, None);
                }
            }
            Msg::Happening(Happening::Landing(notice)) => {
                self.landing = Some((notice.clone(), now));
                self.next_landing = None;
            }
            Msg::Happening(Happening::FlightSaved(_)) => {}
            Msg::Live(live) => {
                self.phase = Some(live.phase());
                self.live = Some(live.clone());
            }
            Msg::Snapshot(s) if self.recording() == Some(&s.summary.id) => {
                if self
                    .snapshot
                    .as_ref()
                    .is_none_or(|old| old.events != s.events)
                {
                    self.next_events = None;
                }
                self.snapshot = Some(s.clone());
            }
            // Of a flight no longer recorded.
            Msg::Snapshot(_) => {}
            Msg::Ack(ack) => {
                self.ack = Some(ack.clone());
                self.next_status = None;
            }
        }
    }

    /// The flight being recorded.
    fn recording(&self) -> Option<&String> {
        match &self.status {
            Some(RecorderStatus::Recording { flight_id, .. }) => Some(flight_id),
            _ => None,
        }
    }

    /// The recorder is connected to the sim, so the feed can be too.
    fn sim_up(&self) -> bool {
        matches!(
            self.status,
            Some(RecorderStatus::Connected | RecorderStatus::Recording { .. })
        )
    }

    fn flight(&self) -> Option<FlightInfo<'_>> {
        let Some(RecorderStatus::Recording {
            flight_id,
            elapsed_s,
        }) = &self.status
        else {
            return None;
        };
        let summary = self.snapshot.as_ref().map(|s| &s.summary);
        let aircraft = summary.and_then(|s| s.aircraft.as_ref());
        Some(FlightInfo {
            id: flight_id,
            aircraft: aircraft.map(history::aircraft),
            departure: summary.and_then(|s| s.departure.as_deref()),
            elapsed_s: *elapsed_s,
            phase: self.phase.unwrap_or("Standing"),
            landings: summary.map_or(0, |s| s.landings),
        })
    }

    /// The messages due at `now`, as (Comm Bus event, JSON).
    fn due(&mut self, now: Instant) -> Vec<(&'static str, String)> {
        let is_due = |next: &mut Option<Instant>, every| {
            let due = next.is_none_or(|t| now >= t);
            if due {
                *next = Some(now + every);
            }
            due
        };
        let mut due = Vec::new();
        if self.status.is_some() && is_due(&mut self.next_status, STATUS_EVERY) {
            let mut msg = json!({ "v": VERSION, "status": self.status });
            if let Some(flight) = self.flight() {
                msg["flight"] = json!(flight);
            }
            if let Some(ack) = self.ack.take() {
                msg["ack"] = json!(ack);
            }
            due.push((STATUS, msg.to_string()));
        }
        if let Some(live) = self.live.take() {
            due.push((LIVE, json!(live).to_string()));
        }
        if let Some(s) = &self.snapshot {
            if is_due(&mut self.next_events, REPEAT_EVERY) {
                let msg = json!({
                    "v": VERSION,
                    "flight_id": s.summary.id,
                    "events": s.events,
                    "counts": s.summary.event_counts,
                });
                due.push((EVENTS, msg.to_string()));
            }
            if is_due(&mut self.next_landings, REPEAT_EVERY) {
                let last: Vec<_> = s.landings.iter().rev().take(LANDINGS_SENT).collect();
                let msg = json!({ "v": VERSION, "landings": last });
                due.push((LANDINGS, msg.to_string()));
            }
        }
        if let Some((notice, at)) = &self.landing
            && is_due(&mut self.next_landing, REPEAT_EVERY)
        {
            let age_s = now.duration_since(*at).as_secs();
            let msg = json!({ "v": VERSION, "age_s": age_s, "notice": notice });
            due.push((LANDING, msg.to_string()));
        }
        for r in self.replies.iter_mut().filter(|r| now >= r.next) {
            (r.left, r.next) = (r.left - 1, now + REPLY_EVERY);
            let event = r.reply.event;
            due.extend(r.reply.msgs.iter().map(|m| (event, m.clone())));
        }
        self.replies.retain(|r| r.left > 0);
        due
    }
}

/// The feed's SimConnect connection, opened on demand.
#[derive(Default)]
struct Link {
    sim: Option<SimConnect>,
    retry_at: Option<Instant>,
}

impl Link {
    /// Sends `msgs` while `up` and returns the commands the views sent;
    /// messages with no connection are dropped, the board repeats them.
    fn send(&mut self, msgs: Vec<(&'static str, String)>, up: bool, now: Instant) -> Vec<String> {
        if !up {
            self.sim = None;
            return Vec::new();
        }
        if self.sim.is_none() && self.retry_at.is_none_or(|t| now >= t) {
            self.retry_at = Some(now + RECONNECT);
            self.sim = SimConnect::open("FOQA-Sim panel feed")
                .inspect(|_| eprintln!("panel feed: connected"))
                .inspect_err(|e| eprintln!("panel feed: {e}"))
                .ok();
            if let Some(sim) = &mut self.sim
                && let Err(e) = sim.subscribe_comm_bus(CMD_EVENT_ID, CMD)
            {
                eprintln!("panel feed: no commands: {e}");
            }
        }
        let Some(sim) = &mut self.sim else {
            return Vec::new();
        };
        publish(sim, &msgs).unwrap_or_else(|e| {
            eprintln!("panel feed: {e}");
            self.sim = None;
            Vec::new()
        })
    }
}

/// Drains the connection's queue, keeping the views' commands, and
/// broadcasts `msgs` to the sim's views.
fn publish(sim: &mut SimConnect, msgs: &[(&str, String)]) -> Result<Vec<String>, String> {
    let mut commands = Vec::new();
    while let Some(msg) = sim.dispatch() {
        match msg {
            Recv::Quit => return Err("simulator quit".into()),
            Recv::CommBus {
                event_id: CMD_EVENT_ID,
                data,
            } => commands.push(data),
            _ => {}
        }
    }
    msgs.iter()
        .try_for_each(|(name, data)| sim.call_comm_bus_js(name, data))
        .map_err(|e| e.to_string())?;
    Ok(commands)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use debrief::{Aircraft, Level, Touchdown};

    use super::*;

    fn notice() -> LandingNotice {
        LandingNotice {
            flight_id: "flight-1".into(),
            touchdown: Touchdown {
                t_s: 259.2,
                bounce: false,
                sink_fpm: 200.0,
                sim_sink_fpm: None,
                peak_g: 1.27,
                pitch_deg: None,
                roll_deg: None,
                ias_kt: None,
                runway: None,
                wind: None,
            },
            events: vec![],
        }
    }

    fn recording(elapsed_s: f64) -> Msg {
        Msg::Happening(Happening::Status(RecorderStatus::Recording {
            flight_id: "flight-1".into(),
            elapsed_s,
        }))
    }

    fn snapshot(landings: usize) -> Snapshot {
        Snapshot {
            summary: FlightSummary {
                id: "flight-1".into(),
                started_utc: None,
                duration_s: 600.0,
                aircraft: Some(Aircraft {
                    icao: Some("C172".into()),
                    title: "C172SP".into(),
                }),
                departure: Some("LTBJ".into()),
                arrival: None,
                landings,
                worst_level: None,
                event_counts: BTreeMap::from([(Level::Advisory, 0)]),
                assessment: debrief::Assessment::of(&[]),
            },
            events: vec![],
            landings: vec![notice(); landings],
        }
    }

    fn parsed(due: &[(&str, String)]) -> Vec<(String, Value)> {
        due.iter()
            .map(|(n, j)| (n.to_string(), serde_json::from_str(j).unwrap()))
            .collect()
    }

    fn named<'a>(due: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
        due.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    #[test]
    fn nothing_is_due_before_anything_happened() {
        assert!(Board::default().due(Instant::now()).is_empty());
    }

    #[test]
    fn status_goes_out_at_most_each_second() {
        let (mut board, t0) = (Board::default(), Instant::now());
        board.take(&Happening::Status(RecorderStatus::Connected).into(), t0);
        let first = parsed(&board.due(t0));
        assert_eq!(
            first,
            [(
                STATUS.into(),
                json!({"v": 2, "status": {"state": "connected"}})
            )]
        );
        board.take(&recording(3.0), t0);
        assert!(board.due(t0 + Duration::from_millis(900)).is_empty());
        let next = parsed(&board.due(t0 + STATUS_EVERY));
        assert_eq!(next[0].1["status"]["state"], "recording");
        assert!(board.sim_up());
    }

    #[test]
    fn status_tells_the_flight_while_recording() {
        let (mut board, t0) = (Board::default(), Instant::now());
        board.take(&recording(42.0), t0);
        board.take(&Msg::Snapshot(snapshot(2)), t0);
        let due = parsed(&board.due(t0));
        let flight = &named(&due, STATUS).unwrap()["flight"];
        assert_eq!(
            flight,
            &json!({"id": "flight-1", "aircraft": "C172", "departure": "LTBJ",
                    "elapsed_s": 42.0, "phase": "Standing", "landings": 2})
        );
        assert_eq!(named(&due, EVENTS).unwrap()["flight_id"], "flight-1");
        let landings = &named(&due, LANDINGS).unwrap()["landings"];
        assert_eq!(landings.as_array().unwrap().len(), 2);
        // Back to the menu: the flight goes, and with it its snapshot.
        let connected = Happening::Status(RecorderStatus::Connected);
        board.take(&connected.into(), t0);
        let due = parsed(&board.due(t0 + STATUS_EVERY));
        assert_eq!(due.len(), 1);
        assert!(due[0].1.get("flight").is_none());
    }

    #[test]
    fn landings_are_the_last_three_newest_first() {
        let (mut board, t0) = (Board::default(), Instant::now());
        board.take(&recording(1.0), t0);
        let mut s = snapshot(4);
        for (i, l) in s.landings.iter_mut().enumerate() {
            l.touchdown.t_s = i as f64;
        }
        board.take(&Msg::Snapshot(s), t0);
        let due = parsed(&board.due(t0));
        let times: Vec<_> = named(&due, LANDINGS).unwrap()["landings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["touchdown"]["t_s"].as_f64().unwrap())
            .collect();
        assert_eq!(times, [3.0, 2.0, 1.0]);
        // A snapshot of another flight is ignored.
        let mut other = snapshot(1);
        other.summary.id = "flight-0".into();
        board.take(&Msg::Snapshot(other), t0);
        assert_eq!(board.snapshot.as_ref().unwrap().landings.len(), 4);
    }

    #[test]
    fn an_ack_goes_out_once_with_the_next_status_at_once() {
        let (mut board, t0) = (Board::default(), Instant::now());
        board.take(&Happening::Status(RecorderStatus::Connected).into(), t0);
        board.due(t0);
        let ack = Ack::new("end_flight", Err("no flight open".into()));
        board.take(&Msg::Ack(ack), t0);
        let due = parsed(&board.due(t0 + Duration::from_millis(100)));
        assert_eq!(
            due[0].1["ack"],
            json!({"cmd": "end_flight", "ok": false, "message": "no flight open"})
        );
        let next = parsed(&board.due(t0 + Duration::from_millis(1100)));
        assert!(next[0].1.get("ack").is_none());
    }

    #[test]
    fn landing_goes_out_at_once_then_repeats_with_its_age() {
        let (mut board, t0) = (Board::default(), Instant::now());
        board.take(&Happening::Landing(notice()).into(), t0);
        let first = parsed(&board.due(t0));
        assert_eq!(first[0].0, LANDING);
        assert_eq!(first[0].1["v"], 2);
        assert_eq!(first[0].1["age_s"], 0);
        assert_eq!(first[0].1["notice"]["touchdown"]["sink_fpm"], 200.0);
        assert!(board.due(t0 + Duration::from_secs(4)).is_empty());
        let again = parsed(&board.due(t0 + REPEAT_EVERY));
        assert_eq!(again[0].1["age_s"], 5);
        // A new landing is due at once.
        let later = t0 + Duration::from_secs(6);
        board.take(&Happening::Landing(notice()).into(), later);
        assert_eq!(board.due(later).len(), 1);
        assert!(!board.sim_up());
    }

    #[test]
    fn a_reply_repeats_each_second_five_times_the_latest_only() {
        let (mut board, t0) = (Board::default(), Instant::now());
        let reply = |event, msgs: &[&str]| Reply {
            event,
            msgs: msgs.iter().map(|m| m.to_string()).collect(),
        };
        board.reply(reply(DEBRIEF, &["a0", "a1"]), t0);
        board.reply(reply(FLIGHTS, &["f"]), t0);
        assert_eq!(board.due(t0).len(), 3);
        assert!(board.due(t0 + Duration::from_millis(900)).is_empty());
        // A newer debrief replaces the older one.
        let t1 = t0 + REPLY_EVERY;
        board.reply(reply(DEBRIEF, &["b0"]), t1);
        let sent = |due: Vec<(&str, String)>| due.into_iter().map(|(_, m)| m).collect::<Vec<_>>();
        assert_eq!(sent(board.due(t1)), ["f", "b0"]);
        for s in 2..REPLY_TIMES {
            assert_eq!(sent(board.due(t0 + REPLY_EVERY * s)), ["f", "b0"]);
        }
        let t5 = t0 + REPLY_EVERY * REPLY_TIMES;
        assert_eq!(sent(board.due(t5)), ["b0"]);
        assert!(board.due(t5 + REPLY_EVERY * 9).is_empty());
    }

    #[test]
    fn queries_parse_and_are_never_acked() {
        let query = |data: &str| match Request::parse(data) {
            Ok(Request::Query(q)) => q,
            other => panic!("{other:?}"),
        };
        let list = query("{\"v\":2,\"cmd\":\"list_flights\",\"req\":\"h1\"}\0");
        assert_eq!((list.req.as_str(), list.ask), ("h1", Ask::Flights));
        let one = query(r#"{"v":2,"cmd":"debrief","req":"h2","id":"flight-1"}"#);
        assert_eq!(one.ask, Ask::Debrief("flight-1".into()));
        let old = query(r#"{"v":1,"cmd":"debrief","req":"h3","id":"flight-1"}"#);
        assert_eq!(old.ask, Ask::Refused("unsupported version"));
        assert_eq!(query(r#"{"v":2,"cmd":"list_flights"}"#).req, "");
        let long = format!(
            r#"{{"v":2,"cmd":"list_flights","req":"{}"}}"#,
            "r".repeat(300)
        );
        assert_eq!(
            Request::parse(&long).unwrap_err().message.as_deref(),
            Some("request too long")
        );
    }

    #[test]
    fn commands_parse_with_or_without_terminators() {
        use ViewCommand::*;
        let parse = |data: &str| {
            Request::parse(data).map(|r| match r {
                Request::Command(cmd) => cmd,
                Request::Query(q) => panic!("{q:?}"),
            })
        };
        assert_eq!(parse(r#"{"v":2,"cmd":"end_flight"}"#), Ok(EndFlight));
        assert_eq!(parse(r#"{"v":2,"cmd":"start_flight"}"#), Ok(StartFlight));
        // After a flight is ended or discarded only a new start reaches the
        // recorder, which opens the next flight on its next frame.
        assert!(for_recorder(&StartFlight, false));
        assert!(!for_recorder(&EndFlight, false));
        assert!(for_recorder(&EndFlight, true) && for_recorder(&StartFlight, true));
        let terminated = "{\"v\":2,\"cmd\":\"discard_flight\"}\0\0";
        assert_eq!(parse(terminated), Ok(DiscardFlight));
        assert_eq!(parse(r#"{"v":2,"cmd":"mark"}"#), Ok(Mark(String::new())));
        let mark = parse(r#"{"v":2,"cmd":"mark","note":"long flare"}"#);
        assert_eq!(mark, Ok(Mark("long flare".into())));
        // Cut at a character boundary.
        let long = format!(r#"{{"v":2,"cmd":"mark","note":"{}"}}"#, "é".repeat(600));
        let Ok(Mark(note)) = parse(&long) else {
            panic!("{long}")
        };
        assert_eq!(note.len(), NOTE_MAX_BYTES);
        let failed = |data: &str| parse(data).unwrap_err();
        let old = failed(r#"{"v":1,"cmd":"mark"}"#);
        assert_eq!(old.message.as_deref(), Some("unsupported version"));
        let unknown = failed(r#"{"v":2,"cmd":"eject"}"#);
        assert_eq!((unknown.cmd.as_str(), unknown.ok), ("eject", false));
        assert_eq!(failed("mark").cmd, "?");
    }

    #[test]
    fn weather_spec_message_and_result() {
        let spec = WeatherSpec {
            base: WeatherBase::Name("clear".into()),
            wind: vec![WindLayer {
                alt_ft: 0.0,
                from_deg: 270.0,
                kt: 18.0,
            }],
            qnh_hpa: Some(1013.0),
            temp_c: None,
        };
        let msg = spec.message("w1").unwrap();
        let v: Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(v["v"], 2);
        assert_eq!(v["cmd"], "set_weather");
        assert_eq!(v["spec"]["base"], "clear");
        assert_eq!(v["spec"]["wind"][0]["from_deg"], 270.0);
        assert!(v["spec"].get("temp_c").is_none());
        let back: WeatherSpec = serde_json::from_value(v["spec"].clone()).unwrap();
        assert_eq!(back, spec);
        let by_index: WeatherSpec =
            serde_json::from_str(r#"{"base":3,"wind":[{"alt_ft":0,"from_deg":0,"kt":0}]}"#)
                .unwrap();
        assert_eq!(by_index.base, WeatherBase::Index(3));
        let mut bad = spec.clone();
        bad.base = WeatherBase::Name("storm".into());
        assert!(bad.validate().is_err());
        bad = spec.clone();
        bad.wind[0].kt = 900.0;
        assert!(bad.message("w").is_err());
        bad = spec.clone();
        bad.wind = vec![spec.wind[0].clone(); 8];
        bad.wind.push(spec.wind[0].clone());
        assert!(bad.validate().is_err());
    }

    #[test]
    fn weather_result_parses() {
        let ok = r#"{"v":2,"cmd":"weather_result","req":"w1","ok":false,
            "applied":{"wind":[{"alt_ft":0,"from_deg":270,"kt":18}],"qnh_hpa":1013.2},
            "measured":{"wind_dir_deg":268,"wind_kt":17.5},"mismatches":["temp"]}"#;
        let r = WeatherResult::parse(&format!("{ok}  ")).unwrap();
        assert_eq!((r.req.as_str(), r.ok), ("w1", false));
        assert_eq!(r.mismatches, ["temp"]);
        assert_eq!(r.measured.unwrap().wind_kt, Some(17.5));
        assert_eq!(r.applied.unwrap().wind[0].from_deg, 270.0);
        assert!(
            WeatherResult::parse(r#"{"v":1,"cmd":"weather_result","req":"a","ok":true}"#).is_err()
        );
        assert!(WeatherResult::parse(r#"{"v":2,"cmd":"mark"}"#).is_err());
        let e = WeatherResult::parse(
            r#"{"v":2,"cmd":"weather_result","req":"a","ok":false,"error":"no listener"}"#,
        );
        assert_eq!(e.unwrap().error.as_deref(), Some("no listener"));
    }
}
