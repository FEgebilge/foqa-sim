//! Compact flight (`.fqz`): a closed journal stored column-wise, one run of
//! values per parameter, delta-coded and compressed. Lossless for every
//! record this reader understands; about a tenth of the journal's size.
//! Spec: `docs/format.md`.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Chain, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::vec;

use fdm_core::{Sample, SignalId, SimTime};

use crate::journal::{Malformed, event_from, event_parts, invalid, ssm_code, ssm_from};
use crate::{JournalReader, Record, SimEvent};

pub const EXT: &str = "fqz";
const MAGIC: &[u8; 4] = b"FQJC";
const FORMAT_VERSION: u16 = 1;
/// zstd level: within 2% of xz -9e on a journal, at about 10 MB/s.
const ZSTD_LEVEL: i32 = 19;

/// A whole flight grouped by parameter, the shape the compact file stores.
/// Per parameter the samples keep their journal order, and events theirs.
#[derive(Debug)]
pub struct Recording {
    lfl_version: u16,
    series: BTreeMap<u16, Vec<Sample>>,
    events: Vec<(SimTime, SimEvent)>,
}

impl Recording {
    pub fn from_records(
        lfl_version: u16,
        records: impl IntoIterator<Item = io::Result<Record>>,
    ) -> io::Result<Self> {
        let mut r = Self {
            lfl_version,
            series: BTreeMap::new(),
            events: Vec::new(),
        };
        for record in records {
            match record? {
                Record::Sample(s) => r.series.entry(s.signal.0).or_default().push(s),
                Record::Event(t, e) => r.events.push((t, e)),
            }
        }
        Ok(r)
    }

    pub fn into_records(self) -> impl Iterator<Item = Record> {
        let samples = self.series.into_values().flatten().map(Record::Sample);
        samples.chain(self.events.into_iter().map(|(t, e)| Record::Event(t, e)))
    }

    pub fn write(&self, mut out: impl Write) -> io::Result<()> {
        out.write_all(MAGIC)?;
        out.write_all(&FORMAT_VERSION.to_le_bytes())?;
        out.write_all(&self.lfl_version.to_le_bytes())?;
        let mut zstd = zstd::Encoder::new(out, ZSTD_LEVEL)?;
        zstd.include_checksum(true)?;
        zstd.write_all(&self.encode())?;
        zstd.finish()?.flush()
    }

    /// Reads a compact file whose magic is already consumed.
    fn read_after_magic(mut input: impl Read) -> io::Result<Self> {
        let mut head = [0; 4];
        input.read_exact(&mut head)?;
        if u16::from_le_bytes([head[0], head[1]]) != FORMAT_VERSION {
            return Err(invalid("not a FOQA-Sim v1 compact flight"));
        }
        let mut body = Vec::new();
        zstd::Decoder::new(input)?.read_to_end(&mut body)?;
        Self::decode(u16::from_le_bytes([head[2], head[3]]), &body)
            .ok_or_else(|| invalid("corrupt compact flight"))
    }

    fn encode(&self) -> Vec<u8> {
        let mut b = Vec::new();
        put(&mut b, self.series.len() as u64);
        for (id, samples) in &self.series {
            put(&mut b, (*id).into());
            put(&mut b, samples.len() as u64);
            let mut last = 0;
            for s in samples {
                put_signed(&mut b, s.t.as_micros().wrapping_sub(last));
                last = s.t.as_micros();
            }
            // XOR with the previous value's bits, then byte planes: a slowly
            // changing parameter leaves long runs of zero bytes.
            let mut prev = 0;
            let xors: Vec<u64> = samples
                .iter()
                .map(|s| {
                    let bits = s.value.to_bits();
                    bits ^ std::mem::replace(&mut prev, bits)
                })
                .collect();
            for plane in 0..8 {
                b.extend(xors.iter().map(|x| (x >> (8 * plane)) as u8));
            }
            b.extend(samples.iter().map(|s| ssm_code(s.ssm)));
        }
        put(&mut b, self.events.len() as u64);
        let mut last = 0;
        for (t, e) in &self.events {
            put_signed(&mut b, t.as_micros().wrapping_sub(last));
            last = t.as_micros();
            let (code, text) = event_parts(e);
            b.push(code);
            put(&mut b, text.len() as u64);
            b.extend(text.as_bytes());
        }
        b
    }

    fn decode(lfl_version: u16, body: &[u8]) -> Option<Self> {
        let mut c = Cursor::new(body);
        let mut series = BTreeMap::new();
        for _ in 0..get(&mut c)? {
            let id = u16::try_from(get(&mut c)?).ok()?;
            let n = usize::try_from(get(&mut c)?).ok()?;
            // Every sample takes at least a time byte, 8 value bytes and an SSM.
            if n > body.len() - c.position() as usize {
                return None;
            }
            let times: Vec<i64> = (0..n).map(|_| get_signed(&mut c)).collect::<Option<_>>()?;
            let planes: Vec<&[u8]> = (0..8).map(|_| take(&mut c, n)).collect::<Option<_>>()?;
            let ssms = take(&mut c, n)?;
            let (mut t, mut bits) = (0i64, 0u64);
            let samples = (0..n)
                .map(|i| {
                    t = t.wrapping_add(times[i]);
                    bits ^= (0..8).fold(0u64, |x, p| x | u64::from(planes[p][i]) << (8 * p));
                    Some(Sample {
                        t: SimTime::from_micros(t),
                        signal: SignalId(id),
                        value: f64::from_bits(bits),
                        ssm: ssm_from(ssms[i])?,
                    })
                })
                .collect::<Option<_>>()?;
            series.insert(id, samples);
        }
        let mut events = Vec::new();
        let mut t = 0i64;
        for _ in 0..get(&mut c)? {
            t = t.wrapping_add(get_signed(&mut c)?);
            let code = take(&mut c, 1)?[0];
            let len = usize::try_from(get(&mut c)?).ok()?;
            let text = take(&mut c, len)?;
            match event_from(code, text) {
                Ok(Some(e)) => events.push((SimTime::from_micros(t), e)),
                // Unknown to this reader: skipped, as in a journal.
                Ok(None) => {}
                Err(Malformed) => return None,
            }
        }
        (c.position() as usize == body.len()).then_some(Self {
            lfl_version,
            series,
            events,
        })
    }
}

/// Equal down to the bit pattern of every value (a NaN equals itself).
impl PartialEq for Recording {
    fn eq(&self, other: &Self) -> bool {
        let same = |a: &Sample, b: &Sample| {
            (a.t, a.signal, a.ssm, a.value.to_bits()) == (b.t, b.signal, b.ssm, b.value.to_bits())
        };
        self.lfl_version == other.lfl_version
            && self.events == other.events
            && self.series.len() == other.series.len()
            && self
                .series
                .iter()
                .zip(&other.series)
                .all(|((ia, a), (ib, b))| {
                    ia == ib && a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
                })
    }
}

/// The records of a journal or a compact flight, whichever the input is.
pub struct FlightRecords<R: Read> {
    lfl_version: u16,
    source: Source<R>,
}

enum Source<R: Read> {
    Journal(JournalReader<Chain<Cursor<[u8; 4]>, R>>),
    Compact(vec::IntoIter<Record>),
}

/// Opens a `.fqj` journal or a `.fqz` compact flight by its magic.
pub fn read_records<R: Read>(mut input: R) -> io::Result<FlightRecords<R>> {
    let mut magic = [0; 4];
    input.read_exact(&mut magic)?;
    if &magic == MAGIC {
        let recording = Recording::read_after_magic(input)?;
        return Ok(FlightRecords {
            lfl_version: recording.lfl_version,
            source: Source::Compact(recording.into_records().collect::<Vec<_>>().into_iter()),
        });
    }
    let reader = JournalReader::new(Cursor::new(magic).chain(input))?;
    Ok(FlightRecords {
        lfl_version: reader.lfl_version(),
        source: Source::Journal(reader),
    })
}

impl<R: Read> FlightRecords<R> {
    pub fn lfl_version(&self) -> u16 {
        self.lfl_version
    }

    /// Offset of a cut-off or corrupt journal tail; always `None` for a
    /// compact flight, which is only written from an intact journal.
    pub fn damaged_at(&self) -> Option<u64> {
        match &self.source {
            Source::Journal(r) => r.damaged_at(),
            Source::Compact(_) => None,
        }
    }
}

impl<R: Read> Iterator for FlightRecords<R> {
    type Item = io::Result<Record>;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.source {
            Source::Journal(r) => r.next(),
            Source::Compact(r) => r.next().map(Ok),
        }
    }
}

/// Replaces the closed, intact journal at `journal` with a compact flight
/// beside it. The compact file is written under a temporary name, read back
/// and compared with the journal's records; only then is it moved into place
/// and the journal deleted. `None`, journal untouched, for a cut-off or
/// corrupt journal: compacting would drop its damaged tail.
pub fn compact_file(journal: &Path) -> io::Result<Option<PathBuf>> {
    let mut reader = JournalReader::new(BufReader::new(File::open(journal)?))?;
    let recording = Recording::from_records(reader.lfl_version(), reader.by_ref())?;
    if reader.damaged_at().is_some() {
        return Ok(None);
    }
    let (part, dest) = (
        journal.with_extension("fqz.part"),
        journal.with_extension(EXT),
    );
    let written = File::create(&part).and_then(|f| {
        recording.write(BufWriter::new(&f))?;
        f.sync_all()?;
        let mut check = BufReader::new(File::open(&part)?);
        check.read_exact(&mut [0; 4])?;
        Recording::read_after_magic(check)
    });
    match written {
        Ok(back) if back == recording => {}
        other => {
            let _ = fs::remove_file(&part);
            return other.and_then(|_| Err(invalid("compact flight differs from its journal")));
        }
    }
    fs::rename(&part, &dest)?;
    fs::remove_file(journal)?;
    Ok(Some(dest))
}

/// LEB128.
fn put(b: &mut Vec<u8>, mut n: u64) {
    while n > 0x7f {
        b.push(n as u8 | 0x80);
        n >>= 7;
    }
    b.push(n as u8);
}

/// Zigzag, then LEB128: small magnitudes of either sign stay short.
fn put_signed(b: &mut Vec<u8>, n: i64) {
    put(b, ((n << 1) ^ (n >> 63)) as u64);
}

fn get(c: &mut Cursor<&[u8]>) -> Option<u64> {
    let (mut n, mut shift) = (0u64, 0);
    loop {
        let byte = take(c, 1)?[0];
        n |= u64::from(byte & 0x7f).checked_shl(shift)?;
        if byte < 0x80 {
            return Some(n);
        }
        shift += 7;
    }
}

fn get_signed(c: &mut Cursor<&[u8]>) -> Option<i64> {
    let n = get(c)?;
    Some((n >> 1) as i64 ^ -((n & 1) as i64))
}

fn take<'a>(c: &mut Cursor<&'a [u8]>, n: usize) -> Option<&'a [u8]> {
    let body = *c.get_ref();
    let at = c.position() as usize;
    let bytes = body.get(at..at.checked_add(n)?)?;
    c.set_position((at + n) as u64);
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use fdm_core::{Lfl, Ssm};

    use super::*;
    use crate::{FlowEvent, JournalWriter};

    fn journal() -> Vec<u8> {
        let mut w = JournalWriter::new(Vec::new(), &Lfl::V1).unwrap();
        let t = SimTime::from_micros(5);
        for e in [
            SimEvent::FlightLoaded("flights/c172.FLT".into()),
            SimEvent::Flow(FlowEvent::FlightStart),
            SimEvent::Airport(crate::airport_json::tests::ltbj()),
            SimEvent::Aircraft(crate::aircraft_json::tests::c172()),
            SimEvent::PilotReport("unstable at 500 ft".into()),
        ] {
            w.event(t, &e).unwrap();
        }
        let values = [0.0, -0.0, 1.5, f64::NAN, f64::INFINITY, 1e-300, -7.25e12];
        for i in 0..400i64 {
            for (id, rate) in [(3u16, 1), (7, 4), (1200, 16)] {
                // Out of order now and then, as the touchdown buffer is.
                let us = if i % 97 == 0 {
                    i * 1000 - 50_000
                } else {
                    i * 1000
                };
                let mut s = Sample::new(
                    SimTime::from_micros(us * rate),
                    SignalId(id),
                    values[(i as usize * rate as usize) % values.len()] + (i % 3) as f64,
                );
                s.ssm = [Ssm::Normal, Ssm::NoComputedData, Ssm::Failure]
                    [(i % 7 == 0) as usize * (id as usize % 3)];
                w.sample(&s).unwrap();
            }
        }
        w.into_inner()
    }

    fn recording(journal: &[u8]) -> Recording {
        let r = JournalReader::new(journal).unwrap();
        Recording::from_records(1, r).unwrap()
    }

    #[test]
    fn compact_flight_reads_back_the_same_records() {
        let journal = journal();
        let original = recording(&journal);
        let mut file = Vec::new();
        original.write(&mut file).unwrap();
        assert!(
            file.len() < journal.len() / 4,
            "{} of {}",
            file.len(),
            journal.len()
        );

        let records = read_records(&file[..]).unwrap();
        assert_eq!((records.lfl_version(), records.damaged_at()), (1, None));
        let back = Recording::from_records(1, records).unwrap();
        assert_eq!(back, original);
        let report = SimEvent::PilotReport("unstable at 500 ft".into());
        assert!(back.events.iter().any(|(_, e)| *e == report));
        // And through the journal reader, grouped by parameter.
        let from_journal = read_records(&journal[..]).unwrap();
        assert_eq!(Recording::from_records(1, from_journal).unwrap(), original);
    }

    #[test]
    fn compact_flight_with_a_damaged_body_is_an_error() {
        let mut file = Vec::new();
        recording(&journal()).write(&mut file).unwrap();
        let cut = file.len() - 10;
        assert!(read_records(&file[..cut]).is_err());
        let mid = file.len() / 2;
        file[mid] ^= 0x55;
        assert!(read_records(&file[..]).is_err());
    }

    #[test]
    fn compacting_a_file_swaps_it_for_the_compact_flight() {
        let dir = std::env::temp_dir().join(format!("foqa-compact-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("flight-1.fqj");
        let bytes = journal();
        fs::write(&path, &bytes).unwrap();
        let dest = compact_file(&path).unwrap().unwrap();
        assert!(!path.exists() && dest.extension().unwrap() == EXT);
        let back = Recording::from_records(1, read_records(File::open(&dest).unwrap()).unwrap());
        assert_eq!(back.unwrap(), recording(&bytes));

        // A cut-off journal is left alone.
        fs::write(&path, &bytes[..bytes.len() - 5]).unwrap();
        assert_eq!(compact_file(&path).unwrap(), None);
        assert!(path.exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn varints_round_trip() {
        for n in [0i64, 1, -1, 63, -64, 1 << 40, i64::MIN, i64::MAX] {
            let mut b = Vec::new();
            put_signed(&mut b, n);
            assert_eq!(get_signed(&mut Cursor::new(&b[..])), Some(n));
        }
    }
}

#[cfg(test)]
mod real_flight {
    use super::*;

    /// `FOQA_REAL_FLIGHT=<flight.fqj> cargo test -p recorder -- --ignored`
    #[test]
    #[ignore]
    fn real_journal_round_trips() {
        let path = std::env::var("FOQA_REAL_FLIGHT").unwrap();
        let journal = fs::read(path).unwrap();
        let original =
            Recording::from_records(1, JournalReader::new(&journal[..]).unwrap()).unwrap();
        let mut file = Vec::new();
        original.write(&mut file).unwrap();
        eprintln!("journal {} B, compact {} B", journal.len(), file.len());
        let back = Recording::from_records(1, read_records(&file[..]).unwrap()).unwrap();
        assert!(back == original);
    }
}
