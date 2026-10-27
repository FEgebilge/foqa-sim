//! Airports and runways from the simulator's facility database: the
//! navigation-database snapshot an FDM ground station matches landings
//! against. One definition (`FIELDS`) asks for an airport, its runways and
//! each runway's two threshold sub-structures; SimConnect answers with one
//! node per structure, linked by parent ids, which `Collector` reassembles
//! into an `fdm_core::Airport`. `AirportFetch` drives the requests.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::time::{Duration, Instant};

use fdm_core::geo::distance_m;
use fdm_core::{Airport, Designator, Runway, RunwayEnd};

use crate::ffi::facility_type;
use crate::{AirportListEntry, Error, Facility, FacilityData};

/// The airport list has no end message; a stalled one counts as complete.
const LIST_TIMEOUT: Duration = Duration::from_secs(5);
/// The bubble's list can be empty right after a flight loads; ask once more.
const LIST_RETRY_DELAY: Duration = Duration::from_secs(2);
const DATA_TIMEOUT: Duration = Duration::from_secs(10);

/// Wire type of a facility field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    F64,
    F32,
    I32,
    /// Fixed 8 bytes, NUL-padded text.
    Str8,
}

impl Kind {
    pub const fn size(self) -> usize {
        match self {
            Self::F64 | Self::Str8 => 8,
            Self::F32 | Self::I32 => 4,
        }
    }

    fn read(self, bytes: &[u8]) -> Value {
        match self {
            Self::F64 => Value::Float(f64::from_le_bytes(array(bytes))),
            Self::F32 => Value::Float(f32::from_le_bytes(array(bytes)).into()),
            Self::I32 => Value::Int(i32::from_le_bytes(array(bytes))),
            Self::Str8 => Value::Text(
                String::from_utf8_lossy(bytes.split(|&b| b == 0).next().unwrap_or_default())
                    .into_owned(),
            ),
        }
    }
}

/// One entry of a facility definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Open(&'static str),
    Close(&'static str),
    Value(&'static str, Kind),
}

impl Field {
    /// The name `SimConnect_AddToFacilityDefinition` takes.
    pub fn request(self) -> String {
        match self {
            Self::Open(block) => format!("OPEN {block}"),
            Self::Close(block) => format!("CLOSE {block}"),
            Self::Value(name, _) => name.to_owned(),
        }
    }
}

/// The airport-with-runways definition. Order is the payload layout of each
/// node, so this list is the only description of it. A threshold
/// sub-structure must be opened and closed (requesting `PRIMARY_THRESHOLD`
/// bare raises an exception); its `LENGTH` is the displaced threshold. Its
/// `ENABLE` flag is not requested: it is unreliable, `LENGTH > 0` says the
/// same. Runway `LENGTH` includes displaced thresholds, not blast pads.
pub const FIELDS: &[Field] = {
    use Field::{Close, Open, Value};
    use Kind::{F32, F64, I32, Str8};
    &[
        Open("AIRPORT"),
        Value("ICAO", Str8),
        Value("REGION", Str8),
        Value("LATITUDE", F64),
        Value("LONGITUDE", F64),
        Value("ALTITUDE", F64),
        Value("MAGVAR", F32),
        Value("N_RUNWAYS", I32),
        Open("RUNWAY"),
        Value("LATITUDE", F64),
        Value("LONGITUDE", F64),
        Value("ALTITUDE", F64),
        Value("HEADING", F32),
        Value("LENGTH", F32),
        Value("WIDTH", F32),
        Value("PRIMARY_NUMBER", I32),
        Value("PRIMARY_DESIGNATOR", I32),
        Value("SECONDARY_NUMBER", I32),
        Value("SECONDARY_DESIGNATOR", I32),
        Open("PRIMARY_THRESHOLD"),
        Value("LENGTH", F32),
        Close("PRIMARY_THRESHOLD"),
        Open("SECONDARY_THRESHOLD"),
        Value("LENGTH", F32),
        Close("SECONDARY_THRESHOLD"),
        Close("RUNWAY"),
        Close("AIRPORT"),
    ]
};

/// Why a facility reply could not become an `Airport`.
#[derive(Debug, Clone, PartialEq)]
pub enum DecodeError {
    /// A node's payload does not match the layout `FIELDS` gives its block.
    Length {
        block: &'static str,
        expected: usize,
        got: usize,
    },
    /// No airport node, e.g. an unknown ICAO.
    NoAirport,
    /// A node that `FIELDS` does not account for, or whose parent is missing.
    Unexpected {
        data_type: u32,
        unique_id: u32,
        parent_id: u32,
    },
    /// A runway without exactly its two threshold nodes.
    Thresholds { runway_id: u32, count: usize },
    /// `N_RUNWAYS` disagrees with the runway nodes received.
    RunwayCount { declared: i32, received: usize },
    /// A code outside its documented range.
    Code { field: &'static str, value: i32 },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length {
                block,
                expected,
                got,
            } => write!(
                f,
                "{block} payload is {got} bytes, layout expects {expected}"
            ),
            Self::NoAirport => write!(f, "no AIRPORT node in the reply"),
            Self::Unexpected {
                data_type,
                unique_id,
                parent_id,
            } => write!(
                f,
                "unexpected node type {data_type} (id {unique_id}, parent {parent_id})"
            ),
            Self::Thresholds { runway_id, count } => {
                write!(
                    f,
                    "runway node {runway_id} has {count} threshold nodes, expected 2"
                )
            }
            Self::RunwayCount { declared, received } => {
                write!(
                    f,
                    "N_RUNWAYS is {declared} but {received} runway nodes arrived"
                )
            }
            Self::Code { field, value } => write!(f, "{field} = {value} is out of range"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Gathers facility nodes per request until its `Facility::End`; replies to
/// several requests may interleave.
#[derive(Debug, Default)]
pub struct Collector {
    pending: HashMap<u32, Vec<FacilityData>>,
}

impl Collector {
    pub fn push(&mut self, node: FacilityData) {
        self.pending.entry(node.request_id).or_default().push(node);
    }

    /// Assembles a request whose `Facility::End` has arrived.
    pub fn finish(&mut self, request_id: u32) -> Result<Airport, DecodeError> {
        assemble(&self.pending.remove(&request_id).unwrap_or_default())
    }

    /// Drops the nodes of an abandoned request.
    pub fn discard(&mut self, request_id: u32) {
        self.pending.remove(&request_id);
    }
}

/// The SimConnect calls an `AirportFetch` makes, so tests can stand in for
/// the simulator.
pub trait Requests {
    /// Appends one field (or `OPEN x` / `CLOSE x`) to a facility definition.
    /// Returns the send id, which an `Exception` carries if it is rejected.
    fn add_facility_field(&mut self, define_id: u32, field: &str) -> Result<u32, Error>;
    /// Requests one facility through a definition; replies arrive as
    /// `Facility::Data` nodes then `Facility::End`. Returns the send id.
    fn request_facility_data(
        &mut self,
        define_id: u32,
        request_id: u32,
        icao: &str,
        region: &str,
    ) -> Result<u32, Error>;
    /// Requests the airports of the simulator's reality bubble (the area
    /// loaded around the user aircraft) as `Facility::AirportList` chunks.
    fn request_airports_nearby(&mut self, request_id: u32) -> Result<(), Error>;
}

/// What an `AirportFetch` yields.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetched {
    /// A search's list arrived: ICAOs within its radius not asked for
    /// before, nearest first. Each comes back as `Airport` or `Failed`.
    Near(Vec<String>),
    Airport(Airport),
    Failed {
        icao: String,
        reason: Failure,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    Decode(DecodeError),
    Timeout,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(e) => write!(f, "cannot decode: {e}"),
            Self::Timeout => write!(f, "no DATA_END within {DATA_TIMEOUT:?}"),
        }
    }
}

/// Where a position search stands.
#[derive(Debug)]
enum Phase {
    /// The list request goes out on the next poll, or once `Some` time is reached.
    Due(Option<Instant>),
    Listing {
        request_id: u32,
        deadline: Instant,
        airports: Vec<AirportListEntry>,
    },
    /// Every chunk is in (or the list stalled).
    Listed(Vec<AirportListEntry>),
}

#[derive(Debug)]
struct Search {
    pos: (f64, f64),
    radius_m: f64,
    retried: bool,
    phase: Phase,
}

/// Request ids are never reused, so a late reply to a request dropped by
/// `AirportFetch::reset` cannot be taken for a new one.
#[derive(Debug, Default)]
struct RequestIds(u32);

impl RequestIds {
    fn next(&mut self) -> u32 {
        self.0 += 1;
        self.0
    }
}

#[derive(Debug)]
struct Pending {
    icao: String,
    deadline: Instant,
}

/// Fetches airports from the facility database without blocking: callers
/// ask (`search`, `want`), pass every `Facility` reply to `feed`, and call
/// `poll` often, which sends requests, times out stalled ones and yields
/// results. A search fetches an ICAO once until `reset` (a new flight) or
/// until it fails, so a bounce or a touch-and-go does not fetch the same
/// airport again; `want` fetches it again unless a request is outstanding.
#[derive(Debug)]
pub struct AirportFetch {
    define_id: u32,
    requests: RequestIds,
    /// What each send id was, to name what SimConnect rejects.
    sent: HashMap<u32, String>,
    search: Option<Search>,
    seen: HashSet<String>,
    queue: Vec<String>,
    pending: HashMap<u32, Pending>,
    collector: Collector,
    ready: VecDeque<Fetched>,
}

impl AirportFetch {
    /// Registers `FIELDS` as facility definition `define_id`.
    pub fn new(sim: &mut impl Requests, define_id: u32) -> Result<Self, Error> {
        let mut sent = HashMap::new();
        for field in FIELDS {
            let name = field.request();
            sent.insert(
                sim.add_facility_field(define_id, &name)?,
                format!("field {name}"),
            );
        }
        Ok(Self {
            define_id,
            requests: RequestIds::default(),
            sent,
            search: None,
            seen: HashSet::new(),
            queue: Vec::new(),
            pending: HashMap::new(),
            collector: Collector::default(),
            ready: VecDeque::new(),
        })
    }

    /// What the call with this send id asked for.
    pub fn describe(&self, send_id: u32) -> Option<&str> {
        self.sent.get(&send_id).map(String::as_str)
    }

    /// Fetches the reality-bubble airports within `radius_m` of `pos`
    /// (lat, lon). Replaces a search still in progress.
    pub fn search(&mut self, pos: (f64, f64), radius_m: f64) {
        self.search = Some(Search {
            pos,
            radius_m,
            retried: false,
            phase: Phase::Due(None),
        });
    }

    /// Fetches one airport by ICAO, wherever it is, unless a request for it
    /// is queued or in flight: an asker whose earlier fetch was not answered
    /// under its name (or not in time) is not left without a request.
    pub fn want(&mut self, icao: &str) {
        let outstanding =
            self.queue.iter().any(|q| q == icao) || self.pending.values().any(|p| p.icao == icao);
        if !outstanding {
            self.seen.insert(icao.to_owned());
            self.queue.push(icao.to_owned());
        }
    }

    /// Forgets every request and result; replies still in flight are ignored.
    pub fn reset(&mut self) {
        self.search = None;
        self.seen.clear();
        self.queue.clear();
        self.pending.clear();
        self.collector = Collector::default();
        self.ready.clear();
    }

    /// Nothing asked for is outstanding.
    pub fn is_idle(&self) -> bool {
        self.search.is_none()
            && self.queue.is_empty()
            && self.pending.is_empty()
            && self.ready.is_empty()
    }

    pub fn feed(&mut self, reply: Facility) {
        match reply {
            Facility::AirportList {
                request_id,
                entry_number,
                out_of,
                airports: chunk,
            } => {
                if let Some(search) = &mut self.search
                    && let Phase::Listing {
                        request_id: listing,
                        airports,
                        ..
                    } = &mut search.phase
                    && *listing == request_id
                {
                    airports.extend(chunk);
                    if entry_number + 1 >= out_of {
                        search.phase = Phase::Listed(std::mem::take(airports));
                    }
                }
            }
            Facility::Data(node) => {
                if self.pending.contains_key(&node.request_id) {
                    self.collector.push(node);
                }
            }
            Facility::End { request_id } => {
                if let Some(p) = self.pending.remove(&request_id) {
                    match self.collector.finish(request_id) {
                        Ok(airport) => self.ready.push_back(Fetched::Airport(airport)),
                        Err(e) => self.fail(p.icao, Failure::Decode(e)),
                    }
                }
            }
        }
    }

    /// Sends due requests, expires stalled ones, and returns the next result.
    pub fn poll(
        &mut self,
        sim: &mut impl Requests,
        now: Instant,
    ) -> Result<Option<Fetched>, Error> {
        self.advance_search(sim, now)?;
        for icao in std::mem::take(&mut self.queue) {
            let request_id = self.requests.next();
            // Region left empty: an airport ICAO is unique on its own.
            let send_id = sim.request_facility_data(self.define_id, request_id, &icao, "")?;
            self.sent
                .insert(send_id, format!("facility request for {icao}"));
            let deadline = now + DATA_TIMEOUT;
            self.pending.insert(request_id, Pending { icao, deadline });
        }
        let expired: Vec<_> = self.pending.extract_if(|_, p| now >= p.deadline).collect();
        for (request_id, p) in expired {
            self.collector.discard(request_id);
            self.fail(p.icao, Failure::Timeout);
        }
        Ok(self.ready.pop_front())
    }

    fn advance_search(&mut self, sim: &mut impl Requests, now: Instant) -> Result<(), Error> {
        let Some(search) = &mut self.search else {
            return Ok(());
        };
        match &mut search.phase {
            Phase::Due(at) if at.is_none_or(|t| now >= t) => {
                let request_id = self.requests.next();
                sim.request_airports_nearby(request_id)?;
                search.phase = Phase::Listing {
                    request_id,
                    deadline: now + LIST_TIMEOUT,
                    airports: Vec::new(),
                };
            }
            Phase::Listing {
                deadline, airports, ..
            } if now >= *deadline => search.phase = Phase::Listed(std::mem::take(airports)),
            Phase::Listed(airports) if airports.is_empty() && !search.retried => {
                search.retried = true;
                search.phase = Phase::Due(Some(now + LIST_RETRY_DELAY));
            }
            Phase::Listed(airports) => {
                let mut near: Vec<_> = airports
                    .iter()
                    .map(|a| (distance_m(search.pos, (a.lat_deg, a.lon_deg)), &a.ident))
                    .filter(|(d, _)| *d <= search.radius_m)
                    .collect();
                near.sort_by(|a, b| a.0.total_cmp(&b.0));
                let new: Vec<String> = near
                    .into_iter()
                    .filter(|(_, icao)| self.seen.insert(icao.to_string()))
                    .map(|(_, icao)| icao.clone())
                    .collect();
                self.queue.extend(new.iter().cloned());
                self.ready.push_back(Fetched::Near(new));
                self.search = None;
            }
            _ => {}
        }
        Ok(())
    }

    /// A failed airport may be asked for again.
    fn fail(&mut self, icao: String, reason: Failure) {
        self.seen.remove(&icao);
        self.ready.push_back(Fetched::Failed { icao, reason });
    }
}

/// Builds the airport from all nodes of one request, in arrival order.
fn assemble(nodes: &[FacilityData]) -> Result<Airport, DecodeError> {
    let children = |parent: &FacilityData, data_type| {
        let id = parent.unique_id;
        nodes
            .iter()
            .filter(move |n| n.parent_id == id && n.data_type == data_type)
    };
    let root = nodes
        .iter()
        .find(|n| n.data_type == facility_type::AIRPORT)
        .ok_or(DecodeError::NoAirport)?;
    let runway_nodes: Vec<_> = children(root, facility_type::RUNWAY).collect();
    let known = |n: &FacilityData| {
        n.unique_id == root.unique_id
            || runway_nodes.iter().any(|rw| {
                n.unique_id == rw.unique_id
                    || (n.parent_id == rw.unique_id && n.data_type == facility_type::PAVEMENT)
            })
    };
    if let Some(n) = nodes.iter().find(|n| !known(n)) {
        return Err(DecodeError::Unexpected {
            data_type: n.data_type,
            unique_id: n.unique_id,
            parent_id: n.parent_id,
        });
    }
    let runways = runway_nodes
        .iter()
        .map(|rw| {
            let thresholds: Vec<_> = children(rw, facility_type::PAVEMENT).collect();
            runway(rw, &thresholds)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let v = decode("AIRPORT", root)?;
    let declared = v.int("N_RUNWAYS");
    if usize::try_from(declared) != Ok(runways.len()) {
        return Err(DecodeError::RunwayCount {
            declared,
            received: runways.len(),
        });
    }
    Ok(Airport {
        icao: v.text("ICAO"),
        region: v.text("REGION"),
        lat_deg: v.float("LATITUDE"),
        lon_deg: v.float("LONGITUDE"),
        elevation_m: v.float("ALTITUDE"),
        magvar_deg: v.float("MAGVAR"),
        runways,
    })
}

/// Fields of one runway end within the runway node, plus its threshold block.
struct EndFields {
    number: &'static str,
    designator: &'static str,
    threshold: &'static str,
}

const PRIMARY: EndFields = EndFields {
    number: "PRIMARY_NUMBER",
    designator: "PRIMARY_DESIGNATOR",
    threshold: "PRIMARY_THRESHOLD",
};
const SECONDARY: EndFields = EndFields {
    number: "SECONDARY_NUMBER",
    designator: "SECONDARY_DESIGNATOR",
    threshold: "SECONDARY_THRESHOLD",
};

fn runway(node: &FacilityData, thresholds: &[&FacilityData]) -> Result<Runway, DecodeError> {
    let v = decode("RUNWAY", node)?;
    let [primary, secondary] = assign_thresholds(node, thresholds)?;
    Ok(Runway {
        lat_deg: v.float("LATITUDE"),
        lon_deg: v.float("LONGITUDE"),
        elevation_m: v.float("ALTITUDE"),
        heading_deg: v.float("HEADING"),
        length_m: v.float("LENGTH"),
        width_m: v.float("WIDTH"),
        primary: runway_end(&v, &PRIMARY, primary)?,
        secondary: runway_end(&v, &SECONDARY, secondary)?,
    })
}

/// Splits a runway's threshold nodes into [primary, secondary]. The nodes do
/// not say which end they belong to (both are `PAVEMENT`, same parent); they
/// arrive in definition order, primary first. Verified 2026-10-26 with
/// `foqa-airports --icao` against OurAirports: on 31 runways with one-sided
/// displaced thresholds at 15 major airports, values land on the same named
/// end in both (e.g. EHAM 18L 577 m vs 575 m); the few mismatches were
/// scenery values, not order.
fn assign_thresholds<'a>(
    runway: &FacilityData,
    thresholds: &[&'a FacilityData],
) -> Result<[&'a FacilityData; 2], DecodeError> {
    match thresholds {
        [primary, secondary] => Ok([primary, secondary]),
        _ => Err(DecodeError::Thresholds {
            runway_id: runway.unique_id,
            count: thresholds.len(),
        }),
    }
}

fn runway_end(
    v: &Values,
    end: &EndFields,
    threshold: &FacilityData,
) -> Result<RunwayEnd, DecodeError> {
    let (number, designator) = (v.int(end.number), v.int(end.designator));
    Ok(RunwayEnd {
        // 1-36 as painted, 37-45 compass-point codes.
        number: u8::try_from(number)
            .ok()
            .filter(|n| *n <= 45)
            .ok_or(DecodeError::Code {
                field: end.number,
                value: number,
            })?,
        designator: Designator::from_code(designator).ok_or(DecodeError::Code {
            field: end.designator,
            value: designator,
        })?,
        displaced_m: decode(end.threshold, threshold)?.float("LENGTH"),
    })
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Float(f64),
    Int(i32),
    Text(String),
}

/// A decoded node: its block's fields by name.
struct Values(HashMap<&'static str, Value>);

impl Values {
    // Names and kinds come from `FIELDS`, so a miss is a bug in this module.
    fn float(&self, name: &str) -> f64 {
        match self.0.get(name) {
            Some(Value::Float(x)) => *x,
            other => unreachable!("{name} is not a float field: {other:?}"),
        }
    }

    fn int(&self, name: &str) -> i32 {
        match self.0.get(name) {
            Some(Value::Int(x)) => *x,
            other => unreachable!("{name} is not an int field: {other:?}"),
        }
    }

    fn text(&self, name: &str) -> String {
        match self.0.get(name) {
            Some(Value::Text(x)) => x.clone(),
            other => unreachable!("{name} is not a text field: {other:?}"),
        }
    }
}

/// Decodes a node as `block`, rejecting a payload of any other length.
fn decode(block: &'static str, node: &FacilityData) -> Result<Values, DecodeError> {
    let fields = block_fields(block);
    let expected = fields.iter().map(|(_, kind)| kind.size()).sum();
    if node.payload.len() != expected {
        return Err(DecodeError::Length {
            block,
            expected,
            got: node.payload.len(),
        });
    }
    let mut rest = node.payload.as_slice();
    Ok(Values(
        fields
            .into_iter()
            .map(|(name, kind)| {
                let (bytes, tail) = rest.split_at(kind.size());
                rest = tail;
                (name, kind.read(bytes))
            })
            .collect(),
    ))
}

/// The value fields directly inside `block`, in order; nested blocks are
/// separate nodes and skipped.
fn block_fields(block: &str) -> Vec<(&'static str, Kind)> {
    let mut depth = None;
    let mut fields = Vec::new();
    for &field in FIELDS {
        match (field, depth) {
            (Field::Open(b), None) if b == block => depth = Some(0),
            (Field::Open(_), Some(d)) => depth = Some(d + 1),
            (Field::Close(_), Some(0)) => break,
            (Field::Close(_), Some(d)) => depth = Some(d - 1),
            (Field::Value(name, kind), Some(0)) => fields.push((name, kind)),
            _ => {}
        }
    }
    fields
}

fn array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    bytes.try_into().expect("slice cut to Kind::size")
}

#[cfg(test)]
mod tests {
    use super::*;

    enum V {
        F(f64),
        I(i32),
        S(&'static str),
    }

    /// Encodes values in `block`'s layout, the inverse of `decode`.
    fn payload(block: &str, values: &[V]) -> Vec<u8> {
        let fields = block_fields(block);
        assert_eq!(fields.len(), values.len(), "{block}");
        let mut out = Vec::new();
        for ((_, kind), v) in fields.into_iter().zip(values) {
            match (kind, v) {
                (Kind::F64, V::F(x)) => out.extend(x.to_le_bytes()),
                (Kind::F32, V::F(x)) => out.extend((*x as f32).to_le_bytes()),
                (Kind::I32, V::I(x)) => out.extend(x.to_le_bytes()),
                (Kind::Str8, V::S(s)) => {
                    let mut text = [0u8; 8];
                    text[..s.len()].copy_from_slice(s.as_bytes());
                    out.extend(text);
                }
                _ => panic!("value does not match {kind:?}"),
            }
        }
        out
    }

    fn node(
        request_id: u32,
        unique_id: u32,
        parent_id: u32,
        data_type: u32,
        payload: Vec<u8>,
    ) -> FacilityData {
        FacilityData {
            request_id,
            unique_id,
            parent_id,
            data_type,
            is_list_item: 0,
            item_index: 0,
            list_size: 0,
            payload,
        }
    }

    fn airport_node(request_id: u32, id: u32, icao: &'static str, runways: i32) -> FacilityData {
        use V::*;
        let values = [
            S(icao),
            S("LT"),
            F(38.29),
            F(27.16),
            F(125.0),
            F(-5.0),
            I(runways),
        ];
        node(
            request_id,
            id,
            0,
            facility_type::AIRPORT,
            payload("AIRPORT", &values),
        )
    }

    /// A runway node (16L/34R) and its two threshold nodes.
    fn runway_nodes(
        request_id: u32,
        id: u32,
        parent: u32,
        displaced: [f64; 2],
    ) -> [FacilityData; 3] {
        use V::*;
        let values = [
            F(38.30),
            F(27.15),
            F(120.0),
            F(160.5),
            F(3240.0),
            F(45.0),
            I(16),
            I(1),
            I(34),
            I(2),
        ];
        let threshold = |offset, len| {
            node(
                request_id,
                id + offset,
                id,
                facility_type::PAVEMENT,
                payload("PRIMARY_THRESHOLD", &[F(len)]),
            )
        };
        [
            node(
                request_id,
                id,
                parent,
                facility_type::RUNWAY,
                payload("RUNWAY", &values),
            ),
            threshold(1, displaced[0]),
            threshold(2, displaced[1]),
        ]
    }

    fn collect(nodes: impl IntoIterator<Item = FacilityData>) -> Collector {
        let mut c = Collector::default();
        nodes.into_iter().for_each(|n| c.push(n));
        c
    }

    #[test]
    fn layouts_follow_the_field_list() {
        let size = |block| {
            block_fields(block)
                .iter()
                .map(|(_, k)| k.size())
                .sum::<usize>()
        };
        assert_eq!(size("AIRPORT"), 8 + 8 + 3 * 8 + 4 + 4);
        assert_eq!(size("RUNWAY"), 3 * 8 + 3 * 4 + 4 * 4);
        assert_eq!(size("PRIMARY_THRESHOLD"), 4);
        assert_eq!(size("SECONDARY_THRESHOLD"), 4);
        assert_eq!(FIELDS[0].request(), "OPEN AIRPORT");
    }

    #[test]
    fn assembles_airport_runway_and_thresholds_in_order() {
        let mut c = collect(
            [airport_node(1, 10, "LTBJ", 1)]
                .into_iter()
                .chain(runway_nodes(1, 20, 10, [0.0, 120.0])),
        );
        let airport = c.finish(1).unwrap();
        let end = |number, designator, displaced_m| RunwayEnd {
            number,
            designator,
            displaced_m,
        };
        assert_eq!(
            airport,
            Airport {
                icao: "LTBJ".into(),
                region: "LT".into(),
                lat_deg: 38.29,
                lon_deg: 27.16,
                elevation_m: 125.0,
                magvar_deg: -5.0,
                runways: vec![Runway {
                    lat_deg: 38.30,
                    lon_deg: 27.15,
                    elevation_m: 120.0,
                    heading_deg: 160.5,
                    length_m: 3240.0,
                    width_m: 45.0,
                    primary: end(16, Designator::Left, 0.0),
                    secondary: end(34, Designator::Right, 120.0),
                }],
            }
        );
    }

    #[test]
    fn interleaved_requests_are_kept_apart() {
        let [rw1, p1, s1] = runway_nodes(1, 20, 10, [0.0, 0.0]);
        let [rw2, p2, s2] = runway_nodes(2, 60, 50, [0.0, 0.0]);
        let [rw3, p3, s3] = runway_nodes(2, 70, 50, [0.0, 0.0]);
        let mut c = collect([
            airport_node(1, 10, "LTBJ", 1),
            airport_node(2, 50, "LTFB", 2),
            rw1,
            rw2,
            p2,
            p1,
            s1,
            s2,
            rw3,
            p3,
            s3,
        ]);
        assert_eq!(c.finish(2).unwrap().runways.len(), 2);
        assert_eq!(c.finish(1).unwrap().icao, "LTBJ");
        assert_eq!(c.finish(1), Err(DecodeError::NoAirport));
    }

    #[test]
    fn airport_without_runways() {
        let mut c = collect([airport_node(4, 10, "LTFM", 0)]);
        assert!(c.finish(4).unwrap().runways.is_empty());
    }

    #[test]
    fn malformed_payload_length_is_rejected() {
        let [mut rw, p, s] = runway_nodes(1, 20, 10, [0.0, 0.0]);
        rw.payload.pop();
        let mut c = collect([airport_node(1, 10, "LTBJ", 1), rw, p, s]);
        assert_eq!(
            c.finish(1),
            Err(DecodeError::Length {
                block: "RUNWAY",
                expected: 52,
                got: 51
            })
        );
    }

    #[test]
    fn structural_mismatches_are_reported() {
        let [rw, p, _] = runway_nodes(1, 20, 10, [0.0, 0.0]);
        let mut c = collect([airport_node(1, 10, "LTBJ", 1), rw.clone(), p]);
        assert_eq!(
            c.finish(1),
            Err(DecodeError::Thresholds {
                runway_id: 20,
                count: 1
            })
        );

        let orphan = node(1, 99, 98, facility_type::PAVEMENT, vec![0; 4]);
        let mut c = collect([airport_node(1, 10, "LTBJ", 0), orphan]);
        assert_eq!(
            c.finish(1),
            Err(DecodeError::Unexpected {
                data_type: facility_type::PAVEMENT,
                unique_id: 99,
                parent_id: 98
            })
        );

        let mut c = collect([airport_node(1, 10, "LTBJ", 2)]);
        assert_eq!(
            c.finish(1),
            Err(DecodeError::RunwayCount {
                declared: 2,
                received: 0
            })
        );
    }

    /// Stands in for the simulator: records the requests made.
    #[derive(Default)]
    struct FakeSim {
        lists: Vec<u32>,
        data: Vec<(u32, String)>,
        sends: u32,
    }

    impl FakeSim {
        fn send(&mut self) -> u32 {
            self.sends += 1;
            self.sends
        }
    }

    impl Requests for FakeSim {
        fn add_facility_field(&mut self, _: u32, _: &str) -> Result<u32, Error> {
            Ok(self.send())
        }

        fn request_facility_data(
            &mut self,
            _: u32,
            request_id: u32,
            icao: &str,
            _: &str,
        ) -> Result<u32, Error> {
            self.data.push((request_id, icao.to_owned()));
            Ok(self.send())
        }

        fn request_airports_nearby(&mut self, request_id: u32) -> Result<(), Error> {
            self.lists.push(request_id);
            Ok(())
        }
    }

    fn fetch(sim: &mut FakeSim) -> AirportFetch {
        AirportFetch::new(sim, 7).unwrap()
    }

    fn list(
        request_id: u32,
        entry_number: u32,
        out_of: u32,
        near: &[(&str, f64, f64)],
    ) -> Facility {
        let airports = near
            .iter()
            .map(|&(ident, lat_deg, lon_deg)| AirportListEntry {
                ident: ident.into(),
                region: "LT".into(),
                lat_deg,
                lon_deg,
                elevation_m: 0.0,
            })
            .collect();
        Facility::AirportList {
            request_id,
            entry_number,
            out_of,
            airports,
        }
    }

    const IZMIR: (f64, f64) = (38.29, 27.16);
    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn search_selects_listed_airports_in_range_and_collects_them() {
        let (mut sim, t0) = (FakeSim::default(), Instant::now());
        let mut f = fetch(&mut sim);
        assert_eq!(f.describe(1), Some("field OPEN AIRPORT"));
        f.search(IZMIR, 10_000.0);
        assert_eq!(f.poll(&mut sim, t0).unwrap(), None);
        assert_eq!(sim.lists, [1]);

        f.feed(list(
            1,
            0,
            2,
            &[("LTFB", 37.95, 27.0), ("LTBJ", 38.2924, 27.157)],
        ));
        assert_eq!(f.poll(&mut sim, t0).unwrap(), None, "list incomplete");
        f.feed(list(
            1,
            1,
            2,
            &[("LTBL", 38.51, 27.01), ("LTXX", 38.30, 27.20)],
        ));
        assert_eq!(
            f.poll(&mut sim, t0).unwrap(),
            Some(Fetched::Near(vec!["LTBJ".into(), "LTXX".into()]))
        );
        assert_eq!(sim.data, [(2, "LTBJ".into()), (3, "LTXX".into())]);
        let send = sim.sends;
        assert_eq!(f.describe(send), Some("facility request for LTXX"));

        let stray = node(99, 1, 0, facility_type::AIRPORT, vec![0; 3]);
        [airport_node(2, 10, "LTBJ", 1), stray]
            .into_iter()
            .chain(runway_nodes(2, 20, 10, [0.0, 0.0]))
            .for_each(|n| f.feed(Facility::Data(n)));
        f.feed(Facility::End { request_id: 3 });
        f.feed(Facility::End { request_id: 2 });
        assert_eq!(
            f.poll(&mut sim, t0).unwrap(),
            Some(Fetched::Failed {
                icao: "LTXX".into(),
                reason: Failure::Decode(DecodeError::NoAirport)
            })
        );
        let Some(Fetched::Airport(airport)) = f.poll(&mut sim, t0).unwrap() else {
            panic!("LTBJ not fetched");
        };
        assert_eq!((airport.icao.as_str(), airport.runways.len()), ("LTBJ", 1));
        assert!(f.is_idle());
    }

    #[test]
    fn empty_list_is_asked_again_once_after_a_delay() {
        let (mut sim, t0) = (FakeSim::default(), Instant::now());
        let mut f = fetch(&mut sim);
        f.search(IZMIR, 10_000.0);
        f.poll(&mut sim, t0).unwrap();
        f.feed(list(1, 0, 1, &[]));
        assert_eq!(f.poll(&mut sim, t0).unwrap(), None);
        assert_eq!(f.poll(&mut sim, t0 + SECOND).unwrap(), None);
        assert_eq!(sim.lists, [1]);
        f.poll(&mut sim, t0 + 2 * SECOND).unwrap();
        assert_eq!(sim.lists, [1, 2]);
        f.feed(list(1, 0, 1, &[("LTBJ", 38.29, 27.16)]));
        f.feed(list(2, 0, 1, &[]));
        assert_eq!(
            f.poll(&mut sim, t0 + 2 * SECOND).unwrap(),
            Some(Fetched::Near(vec![])),
            "the stale first list is ignored and no third is asked"
        );
        assert!(f.is_idle() && sim.data.is_empty());
    }

    #[test]
    fn stalled_list_and_data_requests_time_out() {
        let (mut sim, t0) = (FakeSim::default(), Instant::now());
        let mut f = fetch(&mut sim);
        f.search(IZMIR, 10_000.0);
        f.poll(&mut sim, t0).unwrap();
        f.feed(list(1, 0, 2, &[("LTBJ", 38.29, 27.16)]));
        assert_eq!(f.poll(&mut sim, t0 + 4 * SECOND).unwrap(), None);
        f.poll(&mut sim, t0 + 5 * SECOND).unwrap();
        assert_eq!(
            f.poll(&mut sim, t0 + 5 * SECOND).unwrap(),
            Some(Fetched::Near(vec!["LTBJ".into()])),
            "a stalled list counts with the chunks it has"
        );

        f.feed(Facility::Data(airport_node(2, 10, "LTBJ", 1)));
        assert_eq!(f.poll(&mut sim, t0 + 14 * SECOND).unwrap(), None);
        assert_eq!(
            f.poll(&mut sim, t0 + 15 * SECOND).unwrap(),
            Some(Fetched::Failed {
                icao: "LTBJ".into(),
                reason: Failure::Timeout
            })
        );
        f.feed(Facility::End { request_id: 2 });
        assert_eq!(f.poll(&mut sim, t0 + 15 * SECOND).unwrap(), None);
        assert!(f.is_idle());
        f.want("LTBJ");
        f.poll(&mut sim, t0 + 15 * SECOND).unwrap();
        assert_eq!(
            sim.data,
            [(2, "LTBJ".into()), (3, "LTBJ".into())],
            "failed may retry"
        );
    }

    #[test]
    fn an_airport_is_fetched_once_until_reset() {
        let (mut sim, t0) = (FakeSim::default(), Instant::now());
        let mut f = fetch(&mut sim);
        f.want("LTBJ");
        f.want("LTBJ");
        f.search(IZMIR, 10_000.0);
        f.poll(&mut sim, t0).unwrap();
        f.feed(list(1, 0, 1, &[("LTBJ", 38.29, 27.16)]));
        assert_eq!(f.poll(&mut sim, t0).unwrap(), Some(Fetched::Near(vec![])));
        assert_eq!(sim.data, [(2, "LTBJ".into())]);

        f.reset();
        assert!(f.is_idle());
        f.want("LTBJ");
        f.poll(&mut sim, t0).unwrap();
        assert_eq!(sim.data[1], (3, "LTBJ".into()), "ids are not reused");
        f.feed(Facility::Data(airport_node(2, 10, "LTBJ", 0)));
        f.feed(Facility::End { request_id: 2 });
        assert_eq!(
            f.poll(&mut sim, t0).unwrap(),
            None,
            "reply from before reset"
        );
    }

    #[test]
    fn a_fetched_airport_wanted_again_is_requested_again() {
        let (mut sim, t0) = (FakeSim::default(), Instant::now());
        let mut f = fetch(&mut sim);
        f.want("LTBJ");
        f.poll(&mut sim, t0).unwrap();
        f.feed(Facility::Data(airport_node(1, 10, "LTBJ", 0)));
        f.feed(Facility::End { request_id: 1 });
        assert!(matches!(
            f.poll(&mut sim, t0).unwrap(),
            Some(Fetched::Airport(_))
        ));
        f.want("LTBJ");
        f.want("LTBJ");
        f.poll(&mut sim, t0).unwrap();
        assert_eq!(sim.data, [(1, "LTBJ".into()), (2, "LTBJ".into())]);
        f.want("LTBJ");
        f.poll(&mut sim, t0).unwrap();
        assert_eq!(sim.data.len(), 2, "not while one is in flight");
    }
}
