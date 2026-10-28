# FOQA-Sim file formats

Three files, mirroring the aircraft side of a real flight data chain:

| File | Real-world analogue | Content |
|---|---|---|
| Raw capture (`.raw.jsonl.gz`) | Sensor bus traffic | Every streamed sim frame, before wiring and sampling: all of them on the ground and below 2000 ft radio height, at least 16 Hz of sim time above 2500 ft (`crates/capture/src/live.rs`) |
| Journal (`.fqj`) | QAR / ARINC 767 recording | Parameters at LFL rates, as the FDAU recorded them |
| Compact flight (`.fqz`) | Archived download from the QAR | The same records of a closed journal, column-wise and compressed |

The raw capture is optional (`foqa-rec --raw`, `FOQA_RAW=1` for the app).
The recorder compacts each journal when its flight closes, and the journals
of earlier runs at start-up; `fdm compact` does it by hand. Readers accept
both journal and compact flight.
A recording that is not a flight is deleted when it closes instead (journal and raw capture, never listed): under 10 s of frames (the sim also streams them in its menus), or every position at the menu placeholder (lat 0, lon 90, within 0.01 deg). A ground-only session at a real position is kept. A journal an earlier run left open (the app or the PC stopped mid-flight) gets the same verdict at start-up. Constants in `crates/capture/src/discard.rs`.
A flight closes at flight end or the main menu, when a flight file loads
(MSFS 2024 flow event, or in MSFS 2020 the `FlightLoaded` system event),
before a teleport, time skip or back-on-track jump (the next one opens when
it is done; the app's own challenge placement stays in the flight), and on
an aircraft swap (title or ATC model), so one flight has one aircraft and
no jump. It also closes when the sim quits or its connection is lost (a
crash). A flight whose files cannot be written (disk full, folder locked)
is closed with what it has; recording resumes 10 s later, doubling to 5 min
while it keeps failing. A new flight never takes an existing flight's name:
a taken `flight-<unix seconds>` moves on a second.
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
   - Attempt marker: `{"event":"Attempt","marker":{...}}`, the object
     described under [Attempt marker](#attempt-marker).

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
| code | u8 | `1` sim start, `2` sim stop, `3` crashed, `4` flight loaded, `5` airport snapshot, `6` aircraft identity, `7` pilot report, `8` attempt marker, `9` gear scaled (v1), `32 + n` MSFS flow event `n` (`SIMCONNECT_FLOW_EVENT`) |
| text | UTF-8, rest of payload | Flight file for code `4`, airport snapshot JSON for code `5`, aircraft identity JSON for code `6`, the pilot's note for code `7` (may be empty), attempt marker JSON for code `8`, else empty |

A **gear-scaled marker** (code `9`, no text) is written once at the start of
every recording made since 0.5.0, stamped `0`: its `gear.extended` is in
percent. MSFS reports `GEAR TOTAL PCT EXTENDED` as a 0-1 fraction; earlier
recordings stored it unscaled and have no marker, so a reader scales
`gear.extended` by 100 only when the marker is absent and the series never
passes 1. A slice and a compact flight keep it. Readers that predate it skip
it as an unknown code.

A **pilot report** (code `7`) marks a moment the pilot chose in the in-sim
panel (`mark`, [ingame-panel.md](ingame-panel.md)), stamped like any
event with the last frame's time. Readers that predate it skip it as an
unknown code. The debrief lists it as an advisory event
([debrief-api.md](debrief-api.md)).

### Attempt marker

Where an attempt at a landing challenge begins (the aircraft placed, or a
`self` attempt armed) and ends (finished or abandoned)
([challenge-protocol.md](challenge-protocol.md) A7). Written by the app
through the recorder into the open flight, stamped with the last frame's
time; never into a flight that is not open. Readers that predate it skip
it as an unknown code. One JSON object:

| Key | | |
|---|---|---|
| `phase` | string | `begin` or `end` |
| `challenge`, `version` | string, integer | The challenge flown |
| `ref` | string or `null` | Opaque, chosen by the caller, `^[A-Za-z0-9_-]{8,64}$`; `null` for a local attempt. The recorder gives it no meaning |
| `reason` | string | `end` only: `stopped`, `abandoned`, `replaced`, `flight_closed`, … |

```json
{"challenge":"ltfj-06-gusty-2026w44","phase":"begin","ref":"Qe3v1hJ0dEa9fP2kLm7xRw","version":1}
```

Unknown keys are ignored; an unknown `phase`, a malformed `ref` or a
missing key makes the packet undecodable (damage, below).

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
swapped, which also closes the flight and opens a new one). The latest
one in a flight applies. One JSON object of strings,
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
- The recorder flushes about once a second, at once after an event, and
  when the flight closes. A killed process loses at most about the last
  second and leaves at most one cut-off packet at the end. Readers stop at the first packet that
  is short or fails its CRC and report its offset. Everything before it is
  intact.
- A packet with a valid CRC but an unknown kind or unknown event code is
  **skipped**: a newer recorder may add them, and reading continues.
- A packet of a known kind whose content cannot be decoded (sample of the
  wrong length, unknown SSM code, event shorter than its header, event text
  that is not UTF-8) is treated as damage: reading stops there and reports
  that packet's offset, with no error. Only I/O errors are errors.

### Known sim quirk: gear extension

`gear.extended` is % (0 up, 100 down), but MSFS 2024 returns
`GEAR TOTAL PCT EXTENDED` as a 0-1 fraction whatever unit it is asked in
(measured 2026-10-28, C172SP: 1 in "percent", 0.01 in "percent over 100";
`GEAR CENTER POSITION` in "percent" gives 100). The wiring scales it by 100,
so a raw capture replayed today also gives 0-100. Recordings made before
that store 0-1: a reader that finds a `gear.extended` series whose maximum
is at most 1 scales it by 100. Safe, since a true % series reaches 100 once
the gear is down and an always-retracted one is 0 on either scale.

## Compact flight v1

A closed journal regrouped by parameter. Written next to the
journal under a temporary name, read back and compared with it, and only
then moved into place, after which the journal is deleted. A journal with a
cut-off or corrupt tail keeps the records before the damage, all a reader
gets from it. A temporary file left by a quit while compacting is deleted at
the next start-up. Records of an unknown kind or event code are not carried
over.

### Header (8 bytes)

| Offset | Type | Value |
|---|---|---|
| 0 | 4 bytes | `FQJC` |
| 4 | u16 | Format version, `1` |
| 6 | u16 | LFL version the parameter ids refer to |

The rest of the file is one zstd frame (level 19, with its content
checksum) around the body below. A frame that fails to decompress, or a
body that does not decode exactly, is an error: unlike a journal, a compact
flight is never cut short. A reader refuses a body past 256 MiB, and
past 32 MiB for a challenge input (`fdm challenge`, `fdm slice`).

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

## Recording slice

The part of a flight a site receives for one attempt
([challenge-protocol.md](challenge-protocol.md) A5): a compact flight v1,
cut by `recorder::slice` (`fdm slice`, and the app when an attempt is
sliced). By marker `ref`, the window runs from 10 s before the attempt's
latest `begin` marker to 5 s after the `end` marker that follows it, or to
the end of the data without one. It holds:

- every sample inside the window, plus each parameter's last sample before
  it; sim times are not rebased;
- the aircraft identity in force at the window start and any change inside
  it, with `title` and `atc_id` blanked (`atc_model` and `atc_type` kept);
- the airport snapshots recorded inside the window, plus the latest earlier
  snapshot of each airport whose reference point is within 10 km of a
  position recorded inside the window (the challenge's runway);
- the sim events inside the window, and of the attempt markers only the
  attempt's own (every marker when cut by time range).

Never: the flight-loaded event (a local file path), pilot reports,
anything else outside the window. The same input gives the same bytes, so
its SHA-256 is stable.

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
