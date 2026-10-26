//! Raw capture: full-rate sim frames before conditioning, stored so fixtures
//! can be re-run through the FDAU whenever the wiring or the LFL changes.
//! Spec: `docs/format.md`.

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Lines, Write};
use std::path::Path;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde_json::{Map, Value, json};

use crate::{
    Acquisition, Column, FlowEvent, SimEvent, Telemetry, TelemetrySource, aircraft_json,
    airport_json,
};

const FORMAT: &str = "foqa-raw";
const VERSION: u64 = 1;
/// Frames between flushes; a killed capture loses at most about a second.
const FLUSH_EVERY: u32 = 128;

/// Writes a raw capture while recording.
pub struct RawWriter {
    out: GzEncoder<BufWriter<File>>,
    unflushed: u32,
}

impl RawWriter {
    pub fn create(path: impl AsRef<Path>, columns: &[Column]) -> io::Result<Self> {
        let mut writer = Self {
            out: GzEncoder::new(BufWriter::new(File::create(path)?), Compression::default()),
            unflushed: 0,
        };
        let columns: Vec<_> = columns.iter().map(|c| [c.simvar, c.unit]).collect();
        writer.line(&json!({ "format": FORMAT, "version": VERSION, "columns": columns }))?;
        Ok(writer)
    }

    pub fn write(&mut self, item: &Telemetry) -> io::Result<()> {
        let line = match item {
            // serde_json turns NaN into null.
            Telemetry::Frame(values) => json!(values),
            Telemetry::Pause(paused) => json!({ "event": "Pause_EX1", "data": u32::from(*paused) }),
            Telemetry::Event(SimEvent::SimStart) => json!({ "event": "SimStart" }),
            Telemetry::Event(SimEvent::SimStop) => json!({ "event": "SimStop" }),
            Telemetry::Event(SimEvent::Crashed) => json!({ "event": "Crashed" }),
            Telemetry::Event(SimEvent::FlightLoaded(file)) => {
                json!({ "event": "FlightLoaded", "file": file })
            }
            Telemetry::Event(SimEvent::Flow(flow)) => {
                json!({ "event": "Flow", "code": flow.code() })
            }
            Telemetry::Event(SimEvent::Airport(airport)) => {
                json!({ "event": "Airport", "airport": airport_json::to_json(airport) })
            }
            Telemetry::Event(SimEvent::Aircraft(id)) => {
                json!({ "event": "Aircraft", "aircraft": aircraft_json::to_json(id) })
            }
            Telemetry::Event(SimEvent::PilotReport(note)) => {
                json!({ "event": "PilotReport", "note": note })
            }
        };
        self.line(&line)?;
        self.unflushed += 1;
        if self.unflushed >= FLUSH_EVERY || !matches!(item, Telemetry::Frame(_)) {
            self.unflushed = 0;
            self.out.flush()?;
        }
        Ok(())
    }

    /// Completes the gzip stream. A capture that was never finished still
    /// replays up to its last flush.
    pub fn finish(self) -> io::Result<()> {
        self.out.finish()?.flush()
    }

    fn line(&mut self, value: &Value) -> io::Result<()> {
        serde_json::to_writer(&mut self.out, value)?;
        self.out.write_all(b"\n")
    }
}

/// Replays a raw capture. Columns are matched to the current wiring by SimVar
/// and unit; columns the capture lacks arrive as `NaN`.
pub struct ReplaySource {
    lines: Lines<BufReader<GzDecoder<File>>>,
    /// For each acquisition column, its position in the capture.
    map: Vec<Option<usize>>,
}

impl ReplaySource {
    pub fn open(path: impl AsRef<Path>, acq: &Acquisition) -> io::Result<Self> {
        let mut lines = BufReader::new(GzDecoder::new(File::open(path)?)).lines();
        let header = parse(&lines.next().ok_or_else(|| invalid("empty capture"))??)?;
        if header["format"] != FORMAT || header["version"] != VERSION {
            return Err(invalid("not a foqa-raw v1 capture"));
        }
        let columns: Vec<(&str, &str)> = header["columns"]
            .as_array()
            .ok_or_else(|| invalid("header has no columns"))?
            .iter()
            .map(|c| {
                (
                    c[0].as_str().unwrap_or_default(),
                    c[1].as_str().unwrap_or_default(),
                )
            })
            .collect();
        let map = acq
            .columns()
            .iter()
            .map(|c| {
                columns
                    .iter()
                    .position(|&(simvar, unit)| simvar == c.simvar && unit == c.unit)
            })
            .collect();
        Ok(Self { lines, map })
    }
}

impl TelemetrySource for ReplaySource {
    fn recv(&mut self) -> io::Result<Option<Telemetry>> {
        let line = match self.lines.next() {
            None => return Ok(None),
            // An unfinished capture (killed recorder) ends mid-stream.
            Some(Err(e)) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Some(line) => line?,
        };
        match parse(&line)? {
            Value::Array(row) => Ok(Some(Telemetry::Frame(
                self.map
                    .iter()
                    .map(|i| i.and_then(|i| row.get(i)?.as_f64()).unwrap_or(f64::NAN))
                    .collect(),
            ))),
            Value::Object(event) => event_line(&event).map(Some),
            _ => Err(invalid("expected a frame array or an event object")),
        }
    }
}

fn event_line(event: &Map<String, Value>) -> io::Result<Telemetry> {
    let field = |key| event.get(key);
    let name = field("event").and_then(Value::as_str).unwrap_or_default();
    let item = if name == "Flow" {
        field("code")
            .and_then(Value::as_u64)
            .and_then(|code| FlowEvent::from_code(u32::try_from(code).ok()?))
            .map(|flow| Telemetry::Event(SimEvent::Flow(flow)))
    } else if name == "Airport" {
        field("airport")
            .and_then(airport_json::from_json)
            .map(|a| Telemetry::Event(SimEvent::Airport(a)))
    } else if name == "Aircraft" {
        field("aircraft")
            .and_then(aircraft_json::from_json)
            .map(|a| Telemetry::Event(SimEvent::Aircraft(a)))
    } else if name == "PilotReport" {
        field("note")
            .and_then(Value::as_str)
            .map(|n| Telemetry::Event(SimEvent::PilotReport(n.to_owned())))
    } else {
        let data = field("data")
            .and_then(Value::as_u64)
            .and_then(|d| u32::try_from(d).ok())
            .unwrap_or(0);
        Telemetry::from_system_event(
            name,
            data,
            field("file").and_then(Value::as_str).unwrap_or_default(),
        )
    };
    item.ok_or_else(|| invalid("unknown event"))
}

fn parse(line: &str) -> io::Result<Value> {
    serde_json::from_str(line).map_err(io::Error::from)
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

    use fdm_core::Lfl;

    use super::*;

    #[test]
    fn written_capture_replays_identically() {
        let acq = Acquisition::new(&Lfl::V1);
        let mut frame = vec![f64::NAN; acq.columns().len()];
        frame[0] = 12.5;
        frame[1] = 3500.0;
        let items = [
            Telemetry::Event(SimEvent::Flow(FlowEvent::FlightStart)),
            Telemetry::Event(SimEvent::FlightLoaded("c172.FLT".into())),
            Telemetry::Frame(frame),
            Telemetry::Pause(true),
            Telemetry::Event(SimEvent::Crashed),
            Telemetry::Event(SimEvent::Airport(crate::airport_json::tests::ltbj())),
            Telemetry::Event(SimEvent::Aircraft(crate::aircraft_json::tests::c172())),
            Telemetry::Event(SimEvent::PilotReport("go-around, traffic".into())),
        ];
        let path = env::temp_dir().join(format!("foqa-raw-{}.jsonl.gz", process::id()));
        let mut writer = RawWriter::create(&path, acq.columns()).unwrap();
        items.iter().for_each(|item| writer.write(item).unwrap());
        writer.finish().unwrap();

        let mut source = ReplaySource::open(&path, &acq).unwrap();
        let mut replayed = Vec::new();
        while let Some(item) = source.recv().unwrap() {
            replayed.push(item);
        }
        fs::remove_file(&path).unwrap();
        // NaN != NaN, so compare frames through their debug text.
        assert_eq!(format!("{replayed:?}"), format!("{items:?}"));
    }
}
