//! A runway end's ILS from the facility database, to tune NAV1 for a
//! challenge start. Two requests: the airport's runways name the ILS of
//! each end (`RUNWAY_FIELDS`), then that facility, a VOR-type one, gives
//! its frequency (`VOR_FIELDS`). `IlsFetch` drives them without blocking,
//! like `AirportFetch`.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use fdm_core::{Designator, RunwayEnd};

use crate::facility::{
    Collector, DATA_TIMEOUT, DecodeError, Failure, Field, Kind, RequestIds, Requests, Values,
    decode,
};
use crate::ffi::facility_type;
use crate::{Error, Facility, FacilityData};

/// Far above the airport fetch's request ids, which count from 1 on the
/// same connection.
const REQUEST_BASE: u32 = 1 << 31;
/// `*_ILS_TYPE` of a VOR-type facility (ILS or localizer): the char 'V'.
const VOR_TYPE: i32 = b'V' as i32;
/// The NAV band an ILS frequency must lie in, Hz.
const NAV_BAND_HZ: std::ops::RangeInclusive<u32> = 108_000_000..=118_000_000;

/// Each runway's two ends and the ILS each names (MSFS 2024 SDK,
/// `SimConnect_AddToFacilityDefinition`, RUNWAY): `*_ILS_TYPE` is the named
/// facility's type as a char code; an end without an ILS names none. The
/// airport block holds no values: only its runways are needed.
pub const RUNWAY_FIELDS: &[Field] = {
    use Field::{Close, Open, Value};
    use Kind::{I32, Str8};
    &[
        Open("AIRPORT"),
        Open("RUNWAY"),
        Value("PRIMARY_NUMBER", I32),
        Value("PRIMARY_DESIGNATOR", I32),
        Value("PRIMARY_ILS_ICAO", Str8),
        Value("PRIMARY_ILS_REGION", Str8),
        Value("PRIMARY_ILS_TYPE", I32),
        Value("SECONDARY_NUMBER", I32),
        Value("SECONDARY_DESIGNATOR", I32),
        Value("SECONDARY_ILS_ICAO", Str8),
        Value("SECONDARY_ILS_REGION", Str8),
        Value("SECONDARY_ILS_TYPE", I32),
        Close("RUNWAY"),
        Close("AIRPORT"),
    ]
};

/// The ILS's frequency. `FREQUENCY` is a UINT32, read as I32: the NAV band
/// fits. Hz assumed, as for the airport `FREQUENCY`; anything outside the
/// band is refused rather than tuned.
pub const VOR_FIELDS: &[Field] = &[
    Field::Open("VOR"),
    Field::Value("FREQUENCY", Kind::I32),
    Field::Close("VOR"),
];

/// A runway end, by airport ICAO and its name as painted (`34L`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IlsQuery {
    pub airport: String,
    pub runway: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ils {
    pub ident: String,
    pub region: String,
    pub frequency_hz: u32,
}

/// What an `IlsFetch` yields: the end's ILS, `None` when it has none.
pub type Found = (IlsQuery, Result<Option<Ils>, Failure>);

/// Fields of one runway end within the runway node.
struct EndFields {
    number: &'static str,
    designator: &'static str,
    icao: &'static str,
    region: &'static str,
    kind: &'static str,
}

const ENDS: [EndFields; 2] = [
    EndFields {
        number: "PRIMARY_NUMBER",
        designator: "PRIMARY_DESIGNATOR",
        icao: "PRIMARY_ILS_ICAO",
        region: "PRIMARY_ILS_REGION",
        kind: "PRIMARY_ILS_TYPE",
    },
    EndFields {
        number: "SECONDARY_NUMBER",
        designator: "SECONDARY_DESIGNATOR",
        icao: "SECONDARY_ILS_ICAO",
        region: "SECONDARY_ILS_REGION",
        kind: "SECONDARY_ILS_TYPE",
    },
];

#[derive(Debug)]
enum Stage {
    Runways,
    Vor { ident: String, region: String },
}

#[derive(Debug)]
struct Lookup {
    query: IlsQuery,
    stage: Stage,
    /// Set by the first request: both stages share it.
    deadline: Option<Instant>,
}

/// Finds runway ends' ILS without blocking: callers `want`, pass every
/// `Facility` reply to `feed`, and call `poll` often, which sends requests,
/// times out stalled lookups and yields results. Replies to other fetches
/// are ignored by request id.
#[derive(Debug)]
pub struct IlsFetch {
    runway_define: u32,
    vor_define: u32,
    requests: RequestIds,
    /// What each send id was, to name what SimConnect rejects.
    sent: HashMap<u32, String>,
    due: Vec<Lookup>,
    pending: HashMap<u32, Lookup>,
    collector: Collector,
    ready: VecDeque<Found>,
}

impl IlsFetch {
    /// Registers `RUNWAY_FIELDS` and `VOR_FIELDS` as facility definitions
    /// `runway_define` and `vor_define`.
    pub fn new(
        sim: &mut impl Requests,
        runway_define: u32,
        vor_define: u32,
    ) -> Result<Self, Error> {
        let mut sent = HashMap::new();
        for (define_id, fields) in [(runway_define, RUNWAY_FIELDS), (vor_define, VOR_FIELDS)] {
            for field in fields {
                let name = field.request();
                sent.insert(
                    sim.add_facility_field(define_id, &name)?,
                    format!("field {name}"),
                );
            }
        }
        Ok(Self {
            runway_define,
            vor_define,
            requests: RequestIds(REQUEST_BASE),
            sent,
            due: Vec::new(),
            pending: HashMap::new(),
            collector: Collector::default(),
            ready: VecDeque::new(),
        })
    }

    /// What the call with this send id asked for.
    pub fn describe(&self, send_id: u32) -> Option<&str> {
        self.sent.get(&send_id).map(String::as_str)
    }

    pub fn want(&mut self, query: IlsQuery) {
        self.due.push(Lookup {
            query,
            stage: Stage::Runways,
            deadline: None,
        });
    }

    pub fn feed(&mut self, reply: Facility) {
        match reply {
            Facility::Data(node) if self.pending.contains_key(&node.request_id) => {
                self.collector.push(node);
            }
            Facility::End { request_id } => {
                if let Some(lookup) = self.pending.remove(&request_id) {
                    let nodes = self.collector.take(request_id);
                    self.advance(lookup, &nodes);
                }
            }
            _ => {}
        }
    }

    /// Sends due requests, expires stalled lookups, and returns the next result.
    pub fn poll(&mut self, sim: &mut impl Requests, now: Instant) -> Result<Option<Found>, Error> {
        for mut lookup in std::mem::take(&mut self.due) {
            lookup.deadline.get_or_insert(now + DATA_TIMEOUT);
            let request_id = self.requests.next();
            let (define_id, icao, region) = match &lookup.stage {
                // Region left empty: an airport ICAO is unique on its own.
                Stage::Runways => (self.runway_define, lookup.query.airport.as_str(), ""),
                Stage::Vor { ident, region } => (self.vor_define, ident.as_str(), region.as_str()),
            };
            let send_id = sim.request_facility_data(define_id, request_id, icao, region)?;
            self.sent
                .insert(send_id, format!("facility request for {icao}"));
            self.pending.insert(request_id, lookup);
        }
        let expired: Vec<_> = self
            .pending
            .extract_if(|_, l| l.deadline.is_some_and(|d| now >= d))
            .collect();
        for (request_id, lookup) in expired {
            self.collector.discard(request_id);
            self.ready.push_back((lookup.query, Err(Failure::Timeout)));
        }
        Ok(self.ready.pop_front())
    }

    /// Takes a lookup past the reply to its current stage.
    fn advance(&mut self, mut lookup: Lookup, nodes: &[FacilityData]) {
        let result = match &lookup.stage {
            Stage::Runways => match ils_of(nodes, &lookup.query.runway) {
                Ok(Some((ident, region))) => {
                    lookup.stage = Stage::Vor { ident, region };
                    self.due.push(lookup);
                    return;
                }
                other => other.map(|_| None),
            },
            Stage::Vor { ident, region } => frequency(nodes).map(|frequency_hz| {
                Some(Ils {
                    ident: ident.clone(),
                    region: region.clone(),
                    frequency_hz,
                })
            }),
        };
        self.ready
            .push_back((lookup.query, result.map_err(Failure::Decode)));
    }
}

/// The ILS (ICAO, region) runway end `name` names: the primary end's fields
/// when the primary end is that end, else the secondary's. `None` when the
/// end names no VOR-type facility.
fn ils_of(nodes: &[FacilityData], name: &str) -> Result<Option<(String, String)>, DecodeError> {
    for node in nodes
        .iter()
        .filter(|n| n.data_type == facility_type::RUNWAY)
    {
        let v = decode(RUNWAY_FIELDS, "RUNWAY", node)?;
        if let Some(end) = ENDS
            .iter()
            .find(|end| end_name(&v, end).eq_ignore_ascii_case(name))
        {
            let icao = v.text(end.icao);
            let named = v.int(end.kind) == VOR_TYPE && !icao.is_empty();
            return Ok(named.then(|| (icao, v.text(end.region))));
        }
    }
    Err(DecodeError::NoRunway(name.to_owned()))
}

/// An end's name as painted; out-of-range codes name no runway.
fn end_name(v: &Values, end: &EndFields) -> String {
    let number = u8::try_from(v.int(end.number)).unwrap_or_default();
    let designator = Designator::from_code(v.int(end.designator)).unwrap_or(Designator::None);
    RunwayEnd {
        number,
        designator,
        displaced_m: 0.0,
    }
    .name()
}

/// The frequency of the VOR node of a reply, if in the NAV band.
fn frequency(nodes: &[FacilityData]) -> Result<u32, DecodeError> {
    let node = nodes
        .iter()
        .find(|n| n.data_type == facility_type::VOR)
        .ok_or(DecodeError::Missing("VOR"))?;
    let hz = decode(VOR_FIELDS, "VOR", node)?.int("FREQUENCY");
    u32::try_from(hz)
        .ok()
        .filter(|hz| NAV_BAND_HZ.contains(hz))
        .ok_or(DecodeError::Code {
            field: "FREQUENCY",
            value: hz,
        })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::facility::block_fields;
    use crate::facility::tests::{FakeSim, V, encode, node};

    /// Runway 16L/34R: 16L names `primary`, 34R `secondary` (ICAO, type).
    fn runway(
        request_id: u32,
        primary: (&'static str, u8),
        secondary: (&'static str, u8),
    ) -> FacilityData {
        use V::{I, S};
        let values = [
            I(16),
            I(1),
            S(primary.0),
            S("LT"),
            I(primary.1.into()),
            I(34),
            I(2),
            S(secondary.0),
            S("LT"),
            I(secondary.1.into()),
        ];
        let payload = encode(RUNWAY_FIELDS, "RUNWAY", &values);
        node(request_id, 2, 1, facility_type::RUNWAY, payload)
    }

    fn vor(request_id: u32, hz: i32) -> FacilityData {
        let payload = encode(VOR_FIELDS, "VOR", &[V::I(hz)]);
        node(request_id, 1, 0, facility_type::VOR, payload)
    }

    #[test]
    fn layouts_follow_the_field_lists() {
        let size = |fields, block| {
            block_fields(fields, block)
                .iter()
                .map(|(_, k): &(_, Kind)| k.size())
                .sum::<usize>()
        };
        assert_eq!(size(RUNWAY_FIELDS, "AIRPORT"), 0);
        assert_eq!(size(RUNWAY_FIELDS, "RUNWAY"), 2 * (3 * 4 + 2 * 8));
        assert_eq!(size(VOR_FIELDS, "VOR"), 4);
    }

    #[test]
    fn the_landing_end_picks_primary_or_secondary_ils() {
        let nodes = [runway(1, ("IZBA", b'V'), ("", 0))];
        let ils = |name| ils_of(&nodes, name);
        assert_eq!(ils("16L"), Ok(Some(("IZBA".into(), "LT".into()))));
        assert_eq!(ils("34r"), Ok(None), "no ILS on the secondary end");
        assert_eq!(ils("16R"), Err(DecodeError::NoRunway("16R".into())));

        let nodes = [runway(1, ("", 0), ("IZBB", b'V'))];
        assert_eq!(ils_of(&nodes, "16L"), Ok(None));
        assert_eq!(
            ils_of(&nodes, "34R"),
            Ok(Some(("IZBB".into(), "LT".into())))
        );
        let nodes = [runway(1, ("XX", b'N'), ("", 0))];
        assert_eq!(ils_of(&nodes, "16L"), Ok(None), "an NDB is no ILS");
    }

    fn query(runway: &str) -> IlsQuery {
        IlsQuery {
            airport: "LTBJ".into(),
            runway: runway.into(),
        }
    }

    #[test]
    fn a_lookup_asks_the_runways_then_the_ils() {
        let (mut sim, t0) = (FakeSim::default(), Instant::now());
        let mut f = IlsFetch::new(&mut sim, 8, 9).unwrap();
        assert_eq!(f.describe(1), Some("field OPEN AIRPORT"));
        f.want(query("16L"));
        assert_eq!(f.poll(&mut sim, t0).unwrap(), None);
        let first = REQUEST_BASE + 1;
        assert_eq!(sim.data, [(first, "LTBJ".into())]);

        f.feed(Facility::Data(runway(first, ("IZBA", b'V'), ("", 0))));
        f.feed(Facility::Data(runway(7, ("", 0), ("", 0))));
        f.feed(Facility::End { request_id: first });
        assert_eq!(f.poll(&mut sim, t0).unwrap(), None);
        assert_eq!(sim.data[1], (first + 1, "IZBA/LT".into()));

        f.feed(Facility::Data(vor(first + 1, 110_300_000)));
        f.feed(Facility::End {
            request_id: first + 1,
        });
        let ils = Ils {
            ident: "IZBA".into(),
            region: "LT".into(),
            frequency_hz: 110_300_000,
        };
        assert_eq!(
            f.poll(&mut sim, t0).unwrap(),
            Some((query("16L"), Ok(Some(ils))))
        );
    }

    #[test]
    fn no_ils_out_of_band_and_stalled_lookups_are_reported() {
        let (mut sim, t0) = (FakeSim::default(), Instant::now());
        let mut f = IlsFetch::new(&mut sim, 8, 9).unwrap();
        let first = REQUEST_BASE + 1;
        f.want(query("34R"));
        f.want(query("16L"));
        f.poll(&mut sim, t0).unwrap();
        f.feed(Facility::Data(runway(first, ("IZBA", b'V'), ("", 0))));
        f.feed(Facility::End { request_id: first });
        assert_eq!(
            f.poll(&mut sim, t0).unwrap(),
            Some((query("34R"), Ok(None)))
        );

        f.feed(Facility::Data(runway(first + 1, ("IZBA", b'V'), ("", 0))));
        f.feed(Facility::End {
            request_id: first + 1,
        });
        f.poll(&mut sim, t0 + Duration::from_secs(5)).unwrap();
        f.feed(Facility::Data(vor(first + 2, 1103)));
        f.feed(Facility::End {
            request_id: first + 2,
        });
        let code = DecodeError::Code {
            field: "FREQUENCY",
            value: 1103,
        };
        assert_eq!(
            f.poll(&mut sim, t0).unwrap(),
            Some((query("16L"), Err(Failure::Decode(code))))
        );

        f.want(query("16L"));
        f.poll(&mut sim, t0).unwrap();
        f.feed(Facility::Data(runway(first + 3, ("IZBA", b'V'), ("", 0))));
        f.feed(Facility::End {
            request_id: first + 3,
        });
        f.poll(&mut sim, t0 + Duration::from_secs(9)).unwrap();
        assert_eq!(
            f.poll(&mut sim, t0 + DATA_TIMEOUT).unwrap(),
            Some((query("16L"), Err(Failure::Timeout))),
            "one deadline for both stages"
        );
    }
}
