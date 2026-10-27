# FOQA-Sim file formats

Three files, mirroring the aircraft side of a real flight data chain:

| File | Real-world analogue | Content |
|---|---|---|
| Raw capture (`.raw.jsonl.gz`) | Sensor bus traffic | Every sim frame, before wiring and sampling |
| Journal (`.fqj`) | QAR / ARINC 767 recording | Parameters at LFL rates, as the FDAU recorded them |
| Compact flight (`.fqz`) | Archived download from the QAR | The same records of a closed journal, column-wise and compressed |

The raw capture is optional (`foqa-rec --raw`, `FOQA_RAW=1` for the app).
The recorder compacts each journal when its flight closes, and the journals
of earlier runs at start-up; `fdm compact` does it by hand. Readers accept
both journal and compact flight.
The ground station also keeps a [flight index](#flight-index), a cache of
the flight list.

All integers and floats are little-endian.

## Raw capture v1

Gzip-compressed UTF-8 JSON lines.

1. Header:
   `{"format":"foqa-raw","version":1,"columns":[["SIMULATION TIME","seconds"], ...]}`.
   Each column is a SimVar and the unit it was requested in. Column 0 is
   always `SIMULATION TIME` in seconds. Extra keys are informational.
2. Then, one line per item:
   - Frame: a JSON array with one number per column, `null` for no value.
   - Sim event: an object named after the SimConnect system event, e.g.
     `{"event":"Pause_EX1","data":8}` or `{"event":"FlightLoaded","file":"..."}`,
     or a flow event `{"event":"Flow","code":14}` (`SIMCONNECT_FLOW_EVENT`).
   - Airport snapshot: `{"event":"Airport","airport":{...}}`, the object
     described under [Airport snapshot](#airport-snapshot).
   - Aircraft identity: `{"event":"Aircraft","aircraft":{...}}`, the object
     described under [Aircraft identity](#aircraft-identity).
   - Pilot report: `{"event":"PilotReport","note":"..."}`.

Replay matches columns to the current wiring by SimVar and unit. A column
missing from an older capture yields no value for the parameters it feeds.

## Journal v1

### Header (8 bytes)

| Offset | Type | Value |
|---|---|---|
| 0 | 4 bytes | `FQJR` |
| 4 | u16 | Format version, `1` |
| 6 | u16 | LFL version the parameter ids refer to |

### Packets

Appended one after another until the end of the file:

| Field | Type | |
|---|---|---|
| kind | u8 | `1` sample, `2` event |
| len | u16 | Payload length in bytes |
| payload | `len` bytes | See below |
| crc | u32 | CRC-32 (IEEE) over kind, len and payload |

**Sample** (`len` = 19):

| Field | Type | |
|---|---|---|
| t | i64 | Sim time since recording start, µs |
| id | u16 | Parameter id in the LFL |
| ssm | u8 | `0` normal, `1` no computed data, `2` failure |
| value | f64 | Engineering units of the LFL |

**Event** (`len` ≥ 9):

| Field | Type | |
|---|---|---|
| t | i64 | Sim time of the last frame before the event, µs |
| code | u8 | `1` sim start, `2` sim stop, `3` crashed, `4` flight loaded, `5` airport snapshot, `6` aircraft identity, `7` pilot report, `32 + n` MSFS flow event `n` (`SIMCONNECT_FLOW_EVENT`) |
| text | UTF-8, rest of payload | Flight file for code `4`, airport snapshot JSON for code `5`, aircraft identity JSON for code `6`, the pilot's note for code `7` (may be empty), else empty |

A **pilot report** (code `7`) marks a moment the pilot chose in the in-sim
panel (`mark`, [ingame-panel.md](ingame-panel.md)), stamped like any
event with the last frame's time. Readers that predate it skip it as an
unknown code. The debrief lists it as an advisory event
([debrief-api.md](debrief-api.md)).

### Airport snapshot

The simulator's own facility data for one airport, recorded during the
flight so landings are judged against the scenery actually flown (it can
differ from the real world). `foqa-rec` records the airports within 10 km
of the aircraft at departure (first frame on the ground) and at each
touchdown, each airport once per flight; the event lands a moment after the
frame that triggered it. Raw values in SI units; the ground segment derives
thresholds and distances. One JSON object:

| Key | | |
|---|---|---|
| `icao`, `region` | string | |
| `lat_deg`, `lon_deg`, `elevation_m` | number | Aerodrome reference point |
| `magvar_deg` | number | As MSFS reports it: east-negative (354.4 = 5.6° E); magnetic = true + magvar |
| `runways` | array | Objects below |

Runway: `lat_deg`, `lon_deg`, `elevation_m` (centre of the paved length),
`heading_deg` (true, primary end), `length_m` (includes displaced
thresholds, not overruns), `width_m`, and `primary` / `secondary` ends, each
`{"number": 1–36 (37–45 compass codes), "designator": 0 none, 1 L, 2 R, 3 C,
4 W, 5 A, 6 B, "displaced_m": number}`.

### Aircraft identity

The user aircraft's identification SimVars, so the ground segment can pick
the type profile and its limits. `foqa-rec` records it into every flight,
shortly after the flight opens, and again whenever it changes (aircraft
swapped). The latest one in a flight applies. One JSON object of strings,
exactly as the sim reports them:

| Key | SimVar | Example (MSFS 2024, Cessna 172) |
|---|---|---|
| `title` | `TITLE` | `C172SP G1000 Passengers` |
| `atc_model` | `ATC MODEL` | `ATCCOM.AC_MODEL C172.0.text` |
| `atc_type` | `ATC TYPE` | `ATCCOM.ATC_NAME CESSNA.0.text` |
| `atc_id` | `ATC ID` | Tail number as set in the sim |

The ICAO type designator (`C172`) is read from `atc_model`, which MSFS
reports as a localisation key; older builds wrote
`TT:ATCCOM.AC_MODEL_C172.0.text`, and some aircraft give the bare
designator.

### Ordering and integrity

- Packets carry their own time and are **not** in time order. The touchdown
  pre-trigger buffer is written when the touchdown is detected, after
  later regular samples. Readers sort by `t`.
- The recorder flushes after every sim frame. A killed process leaves at
  most one cut-off packet at the end. Readers stop at the first packet that
  is short or fails its CRC and report its offset. Everything before it is
  intact.
- A packet with a valid CRC but an unknown kind or unknown event code is
  **skipped**: a newer recorder may add them, and reading continues.
- A packet of a known kind whose content cannot be decoded (sample of the
  wrong length, unknown SSM code, event shorter than its header, event text
  that is not UTF-8) is treated as damage: reading stops there and reports
  that packet's offset, with no error. Only I/O errors are errors.

## Compact flight v1

A closed, intact journal regrouped by parameter. Written next to the
journal under a temporary name, read back and compared with it, and only
then moved into place, after which the journal is deleted. A journal with a
cut-off or corrupt tail is not compacted. Records of an unknown kind or event code are
not carried over.

### Header (8 bytes)

| Offset | Type | Value |
|---|---|---|
| 0 | 4 bytes | `FQJC` |
| 4 | u16 | Format version, `1` |
| 6 | u16 | LFL version the parameter ids refer to |

The rest of the file is one zstd frame (level 19, with its content
checksum) around the body below. A frame that fails to decompress, or a
body that does not decode exactly, is an error: unlike a journal, a compact
flight is never cut short.

### Body

`varint` is unsigned LEB128; `zigzag` is a signed value mapped to unsigned
with `(n << 1) ^ (n >> 63)`, then `varint`.

| Field | Type | |
|---|---|---|
| series count | varint | |
| series | | Per parameter, in id order, below |
| event count | varint | |
| events | | In journal order, below |

**Series:** `id` (varint), `n` (varint), then `n` times `dt` (zigzag; the
first is relative to 0, the rest to the previous sample, in journal order,
so a pre-trigger sample appears as a negative step), then eight byte
planes of `n` bytes each (plane `k` holds byte `k` of every `x`, where
`x[i]` is the IEEE 754 bits of value `i` XOR those of value `i - 1`, and
value `-1` is `0`), then `n` SSM codes (as in the journal).

**Event:** `dt` (zigzag, relative to the previous event, the first to 0),
`code` (u8, as in the journal), `len` (varint), `len` bytes of text. Unknown
event codes are skipped, as in the journal.

On the 11-minute flight used to choose the format, the journal was
3.80 MB and the compact flight 0.32 MB (8.5 %).

## Flight index

`index.json` in the flights folder caches the app's flight list
(`FlightSummary`, [debrief-api.md](debrief-api.md)) so a start does not
analyse every flight. It is a cache, not a record: a missing, unreadable or
stale file is rebuilt from the flights, and it can be deleted at any time.

```json
{
  "station": "9f3c0e2a41b7d855",
  "flights": {
    "flight-1793043552.fqz": { "len": 368042, "modified_ns": 1793050212000000000, "summary": { ... } }
  }
}
```

- `station`: hash of the rule set, the type profiles, the landing
  assessment method (`scoring/landing-2.yaml`) and the analysis source
  (`crates/debrief/build.rs`). Any change drops every entry, so new rules,
  bands or code regrade every flight.

- `flights`: by file name, compact flights only (a journal may still be
  recording). An entry is used while the file's `len` (bytes) and
  `modified_ns` (modification time, ns since the Unix epoch) are unchanged;
  `summary` is `null` for a flight with nothing to analyse. Entries of
  deleted flights are dropped.

It is written whole to `index.json.part`, then renamed over the old file.
