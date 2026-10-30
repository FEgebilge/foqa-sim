//! Flight journal: the on-board record, like a QAR. ARINC 767 style: a
//! stream of self-describing, timestamped packets, each with its own CRC,
//! appended as they are produced. A crash can only cut the last packet short;
//! the reader detects that and stops cleanly. Spec: `docs/format.md`.

use std::io::{self, Read, Write};

use fdm_core::{Lfl, Sample, SignalId, SimTime, Ssm};

use crate::{FlowEvent, SimEvent, aircraft_json, airport_json, marker_json};

const MAGIC: &[u8; 4] = b"FQJR";
const FORMAT_VERSION: u16 = 1;
const SAMPLE: u8 = 1;
const EVENT: u8 = 2;
const SAMPLE_LEN: usize = 19;
/// Sample payload layout: time (i64 µs), signal id (u16), SSM code, value (f64).
const T_AT: usize = 0;
const ID_AT: usize = 8;
const SSM_AT: usize = 10;
const VALUE_AT: usize = 11;
/// Event payload: time (i64 µs), event code, then optional UTF-8 text.
const EVENT_CODE_AT: usize = 8;
const EVENT_HEAD_LEN: usize = 9;
const SIM_START: u8 = 1;
const SIM_STOP: u8 = 2;
const CRASHED: u8 = 3;
const FLIGHT_LOADED: u8 = 4;
/// Text is the airport snapshot as JSON.
const AIRPORT: u8 = 5;
/// Text is the aircraft identity as JSON.
const AIRCRAFT: u8 = 6;
/// Text is the pilot's note, possibly empty.
const PILOT_REPORT: u8 = 7;
/// Text is the attempt marker as JSON.
const ATTEMPT: u8 = 8;
/// No text. The recording's `gear.extended` is in percent (docs/format.md).
const GEAR_SCALED_V1: u8 = 9;
/// Retired: the tuned localizer's identity strings, written only by
/// 0.11.3 development builds (docs/format.md). Skipped like an unknown code.
#[cfg(test)]
const RETIRED_LOCALIZER: u8 = 10;
/// SSM wire codes are indexes into this table.
const SSM_CODES: [Ssm; 3] = [Ssm::Normal, Ssm::NoComputedData, Ssm::Failure];
/// Event code of flow event `n` is `FLOW_BASE + n`.
const FLOW_BASE: u8 = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    Sample(Sample),
    Event(SimTime, SimEvent),
}

pub struct JournalWriter<W: Write> {
    out: W,
}

impl<W: Write> JournalWriter<W> {
    pub fn new(mut out: W, lfl: &Lfl) -> io::Result<Self> {
        out.write_all(MAGIC)?;
        out.write_all(&FORMAT_VERSION.to_le_bytes())?;
        out.write_all(&lfl.version.to_le_bytes())?;
        Ok(Self { out })
    }

    pub fn sample(&mut self, s: &Sample) -> io::Result<()> {
        let mut p = [0; SAMPLE_LEN];
        p[T_AT..ID_AT].copy_from_slice(&s.t.as_micros().to_le_bytes());
        p[ID_AT..SSM_AT].copy_from_slice(&s.signal.0.to_le_bytes());
        p[SSM_AT] = ssm_code(s.ssm);
        p[VALUE_AT..].copy_from_slice(&s.value.to_le_bytes());
        self.packet(SAMPLE, &p)
    }

    pub fn event(&mut self, t: SimTime, event: &SimEvent) -> io::Result<()> {
        let (code, text) = event_parts(event);
        let mut p = Vec::with_capacity(EVENT_HEAD_LEN + text.len());
        p.extend(t.as_micros().to_le_bytes());
        p.push(code);
        p.extend(text.as_bytes());
        self.packet(EVENT, &p)
    }

    /// Hands buffered packets to the OS. After this, killing the process
    /// loses nothing; only a power loss can.
    pub fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }

    pub fn into_inner(self) -> W {
        self.out
    }

    fn packet(&mut self, kind: u8, payload: &[u8]) -> io::Result<()> {
        let len =
            u16::try_from(payload.len()).map_err(|_| invalid("packet payload over 64 KiB"))?;
        let [lo, hi] = len.to_le_bytes();
        let head = [kind, lo, hi];
        let mut crc = crc32fast::Hasher::new();
        crc.update(&head);
        crc.update(payload);
        self.out.write_all(&head)?;
        self.out.write_all(payload)?;
        self.out.write_all(&crc.finalize().to_le_bytes())
    }
}

/// Reads packets until the end, or until the first truncated, corrupt or
/// undecodable packet, whose offset is then reported by `damaged_at`.
/// Packets with a valid CRC but an unknown kind or event code are skipped.
pub struct JournalReader<R: Read> {
    input: R,
    lfl_version: u16,
    offset: u64,
    damaged_at: Option<u64>,
}

impl<R: Read> JournalReader<R> {
    pub fn new(mut input: R) -> io::Result<Self> {
        let mut head = [0; 8];
        input.read_exact(&mut head)?;
        if &head[..4] != MAGIC || u16::from_le_bytes([head[4], head[5]]) != FORMAT_VERSION {
            return Err(invalid("not a FOQA-Sim v1 journal"));
        }
        Ok(Self {
            input,
            lfl_version: u16::from_le_bytes([head[6], head[7]]),
            offset: 8,
            damaged_at: None,
        })
    }

    pub fn lfl_version(&self) -> u16 {
        self.lfl_version
    }

    pub fn damaged_at(&self) -> Option<u64> {
        self.damaged_at
    }

    /// The next intact packet's bytes as (kind, payload); `None` at a clean
    /// end or at damage.
    fn packet(&mut self) -> io::Result<Option<(u8, Vec<u8>)>> {
        let mut head = [0; 3];
        match fill(&mut self.input, &mut head)? {
            0 => return Ok(None),
            3 => {}
            _ => return Ok(self.damage()),
        }
        let mut payload = vec![0; usize::from(u16::from_le_bytes([head[1], head[2]]))];
        let mut crc = [0; 4];
        if fill(&mut self.input, &mut payload)? < payload.len()
            || fill(&mut self.input, &mut crc)? < 4
        {
            return Ok(self.damage());
        }
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(&head);
        hasher.update(&payload);
        if hasher.finalize() != u32::from_le_bytes(crc) {
            return Ok(self.damage());
        }
        self.offset += (3 + payload.len() + 4) as u64;
        Ok(Some((head[0], payload)))
    }

    fn damage(&mut self) -> Option<(u8, Vec<u8>)> {
        self.damaged_at = Some(self.offset);
        None
    }
}

impl<R: Read> Iterator for JournalReader<R> {
    type Item = io::Result<Record>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.damaged_at.is_none() {
            let start = self.offset;
            let (kind, p) = match self.packet() {
                Ok(packet) => packet?,
                Err(e) => return Some(Err(e)),
            };
            match decode(kind, &p) {
                Ok(Some(record)) => return Some(Ok(record)),
                Ok(None) => {}
                Err(Malformed) => self.damaged_at = Some(start),
            }
        }
        None
    }
}

/// A packet of a known kind whose content cannot be decoded: damage.
pub(crate) struct Malformed;

/// Decodes one CRC-valid packet. `None` is a kind or event code this reader
/// does not know: skipped, since a newer recorder may add them.
fn decode(kind: u8, p: &[u8]) -> Result<Option<Record>, Malformed> {
    let time = || {
        p.get(T_AT..ID_AT)
            .map(|b| SimTime::from_micros(i64::from_le_bytes(b.try_into().unwrap())))
            .ok_or(Malformed)
    };
    Ok(match kind {
        SAMPLE if p.len() == SAMPLE_LEN => Some(Record::Sample(Sample {
            t: time()?,
            signal: SignalId(u16::from_le_bytes([p[ID_AT], p[ID_AT + 1]])),
            value: f64::from_le_bytes(p[VALUE_AT..].try_into().unwrap()),
            ssm: ssm_from(p[SSM_AT]).ok_or(Malformed)?,
        })),
        EVENT if p.len() >= EVENT_HEAD_LEN => {
            let t = time()?;
            event_from(p[EVENT_CODE_AT], &p[EVENT_HEAD_LEN..])?.map(|e| Record::Event(t, e))
        }
        SAMPLE | EVENT => return Err(Malformed),
        _ => None,
    })
}

/// An event's wire code and text.
pub(crate) fn event_parts(event: &SimEvent) -> (u8, String) {
    match event {
        SimEvent::SimStart => (SIM_START, String::new()),
        SimEvent::SimStop => (SIM_STOP, String::new()),
        SimEvent::Crashed => (CRASHED, String::new()),
        SimEvent::FlightLoaded(file) => (FLIGHT_LOADED, file.clone()),
        SimEvent::Flow(flow) => (FLOW_BASE + flow.code() as u8, String::new()),
        SimEvent::Airport(airport) => (AIRPORT, airport_json::to_json(airport).to_string()),
        SimEvent::Aircraft(id) => (AIRCRAFT, aircraft_json::to_json(id).to_string()),
        SimEvent::PilotReport(note) => (PILOT_REPORT, note.clone()),
        SimEvent::Attempt(m) => (ATTEMPT, marker_json::to_json(m).to_string()),
        SimEvent::GearScaledV1 => (GEAR_SCALED_V1, String::new()),
    }
}

pub(crate) fn event_from(code: u8, text: &[u8]) -> Result<Option<SimEvent>, Malformed> {
    Ok(Some(match code {
        SIM_START => SimEvent::SimStart,
        SIM_STOP => SimEvent::SimStop,
        GEAR_SCALED_V1 => SimEvent::GearScaledV1,
        CRASHED => SimEvent::Crashed,
        FLIGHT_LOADED => SimEvent::FlightLoaded(utf8(text)?),
        PILOT_REPORT => SimEvent::PilotReport(utf8(text)?),
        AIRPORT => SimEvent::Airport(json(text, airport_json::from_json)?),
        AIRCRAFT => SimEvent::Aircraft(json(text, aircraft_json::from_json)?),
        ATTEMPT => SimEvent::Attempt(json(text, marker_json::from_json)?),
        _ => {
            return Ok(code
                .checked_sub(FLOW_BASE)
                .and_then(|n| FlowEvent::from_code(n.into()))
                .map(SimEvent::Flow));
        }
    }))
}

fn utf8(text: &[u8]) -> Result<String, Malformed> {
    String::from_utf8(text.to_vec()).map_err(|_| Malformed)
}

/// Event text decoded as JSON by `from_json`.
fn json<T>(text: &[u8], from_json: fn(&serde_json::Value) -> Option<T>) -> Result<T, Malformed> {
    serde_json::from_slice(text)
        .ok()
        .and_then(|v| from_json(&v))
        .ok_or(Malformed)
}

pub(crate) fn ssm_code(ssm: Ssm) -> u8 {
    SSM_CODES
        .iter()
        .position(|&s| s == ssm)
        .expect("SSM in table") as u8
}

pub(crate) fn ssm_from(code: u8) -> Option<Ssm> {
    SSM_CODES.get(usize::from(code)).copied()
}

/// Reads until `buf` is full or the input ends; returns bytes read.
fn fill(input: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match input.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

pub(crate) fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal() -> (Vec<u8>, Vec<Record>) {
        let mut w = JournalWriter::new(Vec::new(), &Lfl::V1).unwrap();
        let mut records = Vec::new();
        let e = SimEvent::FlightLoaded("flights/c172.FLT".into());
        w.event(SimTime::ZERO, &e).unwrap();
        records.push(Record::Event(SimTime::ZERO, e));
        let flow = SimEvent::Flow(FlowEvent::FlightStart);
        w.event(SimTime::ZERO, &flow).unwrap();
        records.push(Record::Event(SimTime::ZERO, flow));
        for i in 0..50 {
            let mut s = Sample::new(
                SimTime::from_micros(i * 125_000),
                SignalId(7),
                1.0 + i as f64 / 100.0,
            );
            if i % 10 == 0 {
                s.ssm = Ssm::NoComputedData;
            }
            w.sample(&s).unwrap();
            records.push(Record::Sample(s));
        }
        (w.into_inner(), records)
    }

    fn read_all(bytes: &[u8]) -> (Vec<Record>, Option<u64>) {
        let mut r = JournalReader::new(bytes).unwrap();
        let records = r.by_ref().map(Result::unwrap).collect();
        (records, r.damaged_at())
    }

    #[test]
    fn round_trips_samples_and_events() {
        let (bytes, written) = journal();
        assert_eq!(read_all(&bytes), (written, None));
    }

    #[test]
    fn airport_and_aircraft_events_round_trip() {
        let mut w = JournalWriter::new(Vec::new(), &Lfl::V1).unwrap();
        let t = SimTime::from_micros(5);
        let records = [
            SimEvent::Airport(crate::airport_json::tests::ltbj()),
            SimEvent::Aircraft(crate::aircraft_json::tests::c172()),
            SimEvent::PilotReport("long flare, ballooned".into()),
            SimEvent::Attempt(crate::marker_json::tests::begin(Some(
                "Qe3v1hJ0dEa9fP2kLm7xRw",
            ))),
        ]
        .map(|e| {
            w.event(t, &e).unwrap();
            Record::Event(t, e)
        });
        assert_eq!(read_all(&w.into_inner()), (records.into(), None));
    }

    const EVENT_PACKET: usize = 3 + EVENT_HEAD_LEN + "flights/c172.FLT".len() + 4;
    const FLOW_PACKET: usize = 3 + EVENT_HEAD_LEN + 4;
    const SAMPLE_PACKET: usize = 3 + SAMPLE_LEN + 4;

    /// Journal length holding the first `k` records of `journal()`.
    fn encoded_len(k: usize) -> usize {
        match k {
            0 => 8,
            1 => 8 + EVENT_PACKET,
            _ => 8 + EVENT_PACKET + FLOW_PACKET + (k - 2) * SAMPLE_PACKET,
        }
    }

    #[test]
    fn a_cut_at_any_byte_keeps_every_complete_packet() {
        let (bytes, written) = journal();
        for cut in 8..bytes.len() {
            let (read, damaged) = read_all(&bytes[..cut]);
            assert_eq!(read[..], written[..read.len()], "cut at {cut}");
            assert_eq!(
                damaged.is_none(),
                encoded_len(read.len()) == cut,
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn a_flipped_bit_stops_reading_at_that_packet() {
        let (mut bytes, _) = journal();
        let third_sample = encoded_len(4);
        bytes[third_sample + 12] ^= 0x10;
        let (read, damaged) = read_all(&bytes);
        assert_eq!(read.len(), 4); // two events and two samples
        assert_eq!(damaged, Some(third_sample as u64));
    }

    /// A journal of one sample, then `packet`, then another sample.
    fn around(kind: u8, payload: &[u8]) -> (Vec<u8>, usize) {
        let mut w = JournalWriter::new(Vec::new(), &Lfl::V1).unwrap();
        let s = Sample::new(SimTime::ZERO, SignalId(7), 1.0);
        w.sample(&s).unwrap();
        let at = w.out.len();
        w.packet(kind, payload).unwrap();
        w.sample(&s).unwrap();
        (w.into_inner(), at)
    }

    fn event_payload(code: u8, text: &[u8]) -> Vec<u8> {
        let mut p = 0i64.to_le_bytes().to_vec();
        p.push(code);
        p.extend(text);
        p
    }

    #[test]
    fn unknown_event_codes_and_packet_kinds_are_skipped() {
        let retired = event_payload(RETIRED_LOCALIZER, br#"{"airport":"LTBJ"}"#);
        for (kind, payload) in [
            (EVENT, event_payload(200, b"")),
            (EVENT, retired),
            (99, vec![1, 2, 3]),
        ] {
            let (bytes, _) = around(kind, &payload);
            let (read, damaged) = read_all(&bytes);
            assert_eq!((read.len(), damaged), (2, None), "kind {kind}");
        }
    }

    #[test]
    fn malformed_known_packets_are_damage_at_their_offset() {
        let mut bad_ssm = vec![0; SAMPLE_LEN];
        bad_ssm[SSM_AT] = 9;
        for (kind, payload) in [
            (SAMPLE, bad_ssm),
            (SAMPLE, vec![0; SAMPLE_LEN - 1]),
            (EVENT, vec![0; EVENT_HEAD_LEN - 1]),
            (EVENT, event_payload(FLIGHT_LOADED, &[0xff, 0xfe])),
            (EVENT, event_payload(AIRCRAFT, b"{\"title\":\"C172\"}")),
            (EVENT, event_payload(ATTEMPT, b"{\"phase\":\"begin\"}")),
        ] {
            let (bytes, at) = around(kind, &payload);
            let (read, damaged) = read_all(&bytes);
            assert_eq!((read.len(), damaged), (1, Some(at as u64)), "{payload:?}");
        }
    }
}
