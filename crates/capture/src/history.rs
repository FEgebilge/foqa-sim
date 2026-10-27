//! The EFB app's flight history: the views' `list_flights` and `debrief`
//! queries answered from the flights folder, off the feed's thread and one
//! at a time (`docs/ingame-panel.md`, "Flight history").

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use debrief::{
    Aircraft, Debrief, Event, FlightSummary, Index, Level, PILOT_REPORT, Station, Touchdown,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::panel::{DEBRIEF, FLIGHTS, VERSION};
use crate::service::ClearOnDrop;

/// Bytes of one Comm Bus message, its NUL terminator included. Provisional:
/// the real Comm Bus limit is not yet measured in the sim.
const CAP: usize = 6 * 1024;
/// Bytes of one message's UTF-8 JSON: `CAP` less the terminator.
const JSON_MAX: usize = CAP - 1;
/// Flights in `FOQA_SIM_FLIGHTS`, fewer when they do not fit `JSON_MAX`.
const FLIGHTS_SENT: usize = 30;
/// A longer `req` plus `id` is refused, so every reply fits `JSON_MAX`.
pub(crate) const REQUEST_MAX_BYTES: usize = 256;

/// A view's query, answered with its `req`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Query {
    pub req: String,
    pub ask: Ask,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Ask {
    Flights,
    /// Of this flight id.
    Debrief(String),
    /// Answered at once with this error.
    Refused(&'static str),
}

/// The messages answering a query, all on one Comm Bus event.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Reply {
    pub event: &'static str,
    pub msgs: Vec<String>,
}

impl Query {
    fn error(&self, message: &str) -> Reply {
        let mut msg = json!({ "v": VERSION, "req": self.req, "error": message });
        let event = match &self.ask {
            Ask::Debrief(id) => {
                msg["id"] = json!(id);
                DEBRIEF
            }
            _ => FLIGHTS,
        };
        Reply {
            event,
            msgs: vec![msg.to_string()],
        }
    }
}

/// Answers queries on threads of their own, one at a time; a query asked
/// while one runs waits, replaced by any later one.
pub(crate) struct History {
    dir: PathBuf,
    station: Arc<Station>,
    index: Arc<Index>,
    busy: Arc<AtomicBool>,
    pending: Option<Query>,
    tx: Sender<Reply>,
    rx: Receiver<Reply>,
}

impl History {
    /// Of the flights folder `dir`, listed by its `index`.
    pub fn new(dir: PathBuf, station: Arc<Station>, index: Arc<Index>) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            index,
            dir,
            station,
            busy: Arc::default(),
            pending: None,
            tx,
            rx,
        }
    }

    pub fn ask(&mut self, query: Query) {
        if let Ask::Refused(message) = query.ask {
            let _ = self.tx.send(query.error(message));
        } else {
            self.pending = Some(query);
        }
        self.start();
    }

    /// The replies ready since the last call; starts the waiting query.
    pub fn poll(&mut self) -> Vec<Reply> {
        self.start();
        self.rx.try_iter().collect()
    }

    fn start(&mut self) {
        let busy = &self.busy;
        let Some(query) = self
            .pending
            .take_if(|_| !busy.swap(true, Ordering::Acquire))
        else {
            return;
        };
        let (dir, station, index) = (self.dir.clone(), self.station.clone(), self.index.clone());
        let (tx, busy) = (self.tx.clone(), self.busy.clone());
        thread::spawn(move || {
            let _clear = ClearOnDrop(busy);
            let _ = tx.send(answer(&dir, &station, &index, &query));
        });
    }
}

fn answer(dir: &Path, station: &Station, index: &Index, query: &Query) -> Reply {
    match &query.ask {
        Ask::Flights => {
            let saved = index.saved(station).into_iter();
            let mut items: Vec<_> = saved.map(FlightItem::from).collect();
            items.sort_by_key(|f| Reverse(f.started_utc.clone()));
            Reply {
                event: FLIGHTS,
                msgs: vec![flights(&query.req, items)],
            }
        }
        Ask::Debrief(id) => match compact(dir, station, id) {
            Ok(doc) => Reply {
                event: DEBRIEF,
                msgs: parts(&query.req, id, &doc),
            },
            Err(e) => query.error(&e),
        },
        Ask::Refused(message) => query.error(message),
    }
}

/// The JSON text of flight `id`'s `CompactDebrief`.
fn compact(dir: &Path, station: &Station, id: &str) -> Result<String, String> {
    let path = debrief::flight_files(dir)
        .into_iter()
        .find(|p| debrief::flight_id(p) == id)
        .ok_or("no such flight")?;
    let analysis = station.open(&path).map_err(|e| e.to_string())?;
    serde_json::to_string(&CompactDebrief::from(analysis.debrief())).map_err(|e| e.to_string())
}

/// `FOQA_SIM_FLIGHTS` with the first of `items` that fit `JSON_MAX`.
fn flights(req: &str, mut items: Vec<FlightItem>) -> String {
    items.truncate(FLIGHTS_SENT);
    loop {
        let msg = json!({ "v": VERSION, "req": req, "flights": items }).to_string();
        if msg.len() <= JSON_MAX || items.pop().is_none() {
            return msg;
        }
    }
}

/// `doc` as `FOQA_SIM_DEBRIEF` messages of at most `JSON_MAX` bytes each, split
/// on character boundaries.
fn parts(req: &str, id: &str, doc: &str) -> Vec<String> {
    let msg = |seq: usize, total: usize, part: &str| {
        let msg =
            json!({ "v": VERSION, "req": req, "id": id, "seq": seq, "total": total, "part": part });
        msg.to_string()
    };
    // The budget leaves room for the widest seq, so it shrinks as total grows.
    let mut total = 1;
    let chunks = loop {
        let chunks = split(doc, JSON_MAX - msg(total - 1, total, "").len());
        if chunks.len() <= total {
            break chunks;
        }
        total = chunks.len();
    };
    let total = chunks.len();
    let enumerated = chunks.iter().enumerate();
    enumerated.map(|(seq, p)| msg(seq, total, p)).collect()
}

/// `doc` in pieces of at most `budget` bytes once escaped as JSON.
fn split(doc: &str, budget: usize) -> Vec<&str> {
    let (mut chunks, mut start, mut used) = (Vec::new(), 0, 0);
    for (i, c) in doc.char_indices() {
        let n = escaped_len(c);
        if used + n > budget && i > start {
            chunks.push(&doc[start..i]);
            (start, used) = (i, 0);
        }
        used += n;
    }
    chunks.push(&doc[start..]);
    chunks
}

fn escaped_len(c: char) -> usize {
    Value::from(c.to_string()).to_string().len() - 2
}

/// ICAO type, else sim title.
pub(crate) fn aircraft(a: &Aircraft) -> &str {
    a.icao.as_deref().unwrap_or(&a.title)
}

/// `FlightSummary`, trimmed.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct FlightItem {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    started_utc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aircraft: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    departure: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arrival: Option<String>,
    duration_s: u64,
    landings: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    worst_level: Option<Level>,
    event_counts: BTreeMap<Level, usize>,
}

impl From<FlightSummary> for FlightItem {
    fn from(s: FlightSummary) -> Self {
        Self {
            aircraft: s.aircraft.as_ref().map(|a| aircraft(a).to_owned()),
            id: s.id,
            started_utc: s.started_utc,
            departure: s.departure,
            arrival: s.arrival,
            duration_s: s.duration_s.round() as u64,
            landings: s.landings,
            worst_level: s.worst_level,
            event_counts: s.event_counts,
        }
    }
}

/// The debrief without series, phases or airports.
#[derive(Debug, Serialize)]
struct CompactDebrief {
    flight: CompactFlight,
    touchdowns: Vec<Touchdown>,
    events: Vec<CompactEvent>,
}

#[derive(Debug, Serialize)]
struct CompactFlight {
    #[serde(flatten)]
    item: FlightItem,
    profile: String,
}

#[derive(Debug, Serialize)]
struct CompactEvent {
    rule: String,
    level: Level,
    title: String,
    start_s: f64,
    peak_s: f64,
    measure: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    why_it_matters: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    advice: Option<String>,
    /// A pilot report's note.
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

impl From<Event> for CompactEvent {
    fn from(e: Event) -> Self {
        Self {
            note: (e.rule == PILOT_REPORT).then_some(e.description),
            rule: e.rule,
            level: e.level,
            title: e.title,
            start_s: e.start_s,
            peak_s: e.peak_s,
            measure: e.measure,
            value: e.value,
            threshold: e.threshold,
            unit: e.unit,
            detail: e.detail,
            why_it_matters: e.why_it_matters,
            advice: e.advice,
        }
    }
}

impl From<Debrief> for CompactDebrief {
    fn from(d: Debrief) -> Self {
        Self {
            flight: CompactFlight {
                item: d.flight.summary.into(),
                profile: d.flight.profile,
            },
            touchdowns: d.touchdowns,
            events: d.events.into_iter().map(CompactEvent::from).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(i: usize) -> FlightItem {
        FlightItem {
            id: format!("flight-{i:04}"),
            started_utc: Some(format!("2026-10-{:02}T12:00:00Z", i % 28 + 1)),
            aircraft: Some("C172".into()),
            departure: Some("LTBJ".into()),
            arrival: Some("LTFM".into()),
            duration_s: 3600,
            landings: 1,
            worst_level: None,
            event_counts: Level::ALL.map(|l| (l, 0)).into(),
        }
    }

    fn joined(msgs: &[String]) -> (String, Vec<Value>) {
        let parsed: Vec<Value> = msgs
            .iter()
            .map(|m| serde_json::from_str(m).unwrap())
            .collect();
        let doc = parsed.iter().map(|p| p["part"].as_str().unwrap()).collect();
        (doc, parsed)
    }

    #[test]
    fn parts_fit_the_cap_and_join_to_the_document() {
        // Multibyte, escaped and \u-escaped characters across part boundaries.
        let doc = "aé\"\\\n\u{1}€😀".repeat(2000);
        let msgs = parts("h1", "flight-1", &doc);
        assert!(msgs.len() > 3);
        assert!(
            msgs.iter().all(|m| m.len() <= JSON_MAX),
            "a part over the cap"
        );
        let (back, parsed) = joined(&msgs);
        assert_eq!(back, doc);
        for (i, p) in parsed.iter().enumerate() {
            assert_eq!(
                (&p["v"], &p["req"], &p["id"]),
                (&json!(2), &json!("h1"), &json!("flight-1"))
            );
            assert_eq!(
                (p["seq"].as_u64(), p["total"].as_u64()),
                (Some(i as u64), Some(msgs.len() as u64))
            );
        }
        assert_eq!(joined(&parts("h1", "f", "")).0, "");
    }

    #[test]
    fn a_part_fills_the_cap_exactly() {
        let room = JSON_MAX - parts("h1", "f", "").last().unwrap().len();
        let full = parts("h1", "f", &"x".repeat(room));
        assert_eq!(full.len(), 1);
        // With its NUL terminator.
        assert_eq!(full[0].len() + 1, CAP);
        let over = parts("h1", "f", &"x".repeat(room + 1));
        assert_eq!(over.len(), 2);
        assert_eq!(over[0].len() + 1, CAP);
    }

    #[test]
    fn the_list_is_cut_to_fit_the_cap() {
        let sent = |msg: &str| {
            let msg: Value = serde_json::from_str(msg).unwrap();
            assert_eq!(msg["flights"][0]["id"], "flight-0000");
            msg["flights"].as_array().unwrap().len()
        };
        assert_eq!(sent(&flights("h1", (0..40).map(item).collect())), 30);
        let wide = |i| FlightItem {
            aircraft: Some("x".repeat(300)),
            ..item(i)
        };
        let many = flights("h1", (0..30).map(wide).collect());
        let fit = sent(&many);
        assert!(many.len() <= JSON_MAX && fit < 30);
        assert_eq!(flights("h1", (0..=fit).map(wide).collect()), many);
        let mut big = item(0);
        big.aircraft = Some("x".repeat(CAP));
        assert_eq!(
            flights("h1", vec![big]),
            r#"{"flights":[],"req":"h1","v":2}"#
        );
    }

    #[test]
    fn a_debrief_goes_out_compact() {
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/c172-pattern.raw.jsonl.gz"
        );
        let station = Station::core().unwrap();
        let debrief = station.open(Path::new(fixture)).unwrap().debrief();
        let doc = serde_json::to_string(&CompactDebrief::from(debrief)).unwrap();
        let (back, _) = joined(&parts("h1", "c172", &doc));
        let d: Value = serde_json::from_str(&back).unwrap();
        let keys: Vec<_> = d.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["events", "flight", "touchdowns"]);
        assert_eq!(d["flight"]["id"], "c172-pattern");
        assert!(d["flight"]["profile"].is_string() && d["flight"]["duration_s"].is_u64());
        assert_eq!(d["touchdowns"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn an_unknown_flight_is_an_error() {
        let station = Station::core().unwrap();
        let dir = Path::new("no-such-folder");
        let index = Index::open(dir);
        let query = |ask| Query {
            req: "h7".into(),
            ask,
        };
        let reply = answer(dir, &station, &index, &query(Ask::Debrief("../x".into())));
        assert_eq!(reply.event, DEBRIEF);
        let msg: Value = serde_json::from_str(&reply.msgs[0]).unwrap();
        assert_eq!(
            msg,
            json!({"v": 2, "req": "h7", "id": "../x", "error": "no such flight"})
        );
        let list = answer(dir, &station, &index, &query(Ask::Flights));
        assert_eq!(list.msgs, [r#"{"flights":[],"req":"h7","v":2}"#]);
        let refused = answer(
            dir,
            &station,
            &index,
            &query(Ask::Refused("unsupported version")),
        );
        assert_eq!((refused.event, refused.msgs.len()), (FLIGHTS, 1));
    }
}
