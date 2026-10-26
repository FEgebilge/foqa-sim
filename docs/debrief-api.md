# Debrief API

Contract between the FOQA-Sim desktop app's Rust backend (Tauri commands and
events) and its React frontend. The backend builds these objects from a
journal with the analyzer and the rule engine; `fdm debrief <flight> --json`
prints the same `Debrief` object, so the frontend can be developed against
real data without the app. Times are seconds of sim time since the start of
the recording; units are in the field names.

## Commands

| Command | Returns | |
|---|---|---|
| `list_flights()` | `FlightSummary[]` | Newest first |
| `debrief(id: string)` | `Debrief` | Full analysis of one flight |
| `recorder_status()` | `RecorderStatus` | Current state, also pushed as an event |
| `open_flights_folder()` | `void` | Opens the folder in Explorer |
| `storage_usage()` | `StorageUsage` | Flight count and the folder's total size on disk (raw captures included) |

## Events (backend → frontend)

| Event | Payload | |
|---|---|---|
| `recorder-status` | `RecorderStatus` | Whenever the state changes, and each second while recording (`elapsed_s`) |
| `flight-saved` | `FlightSummary` | A flight was closed and analysed |
| `landing` | `LandingNotice` | Seconds after a touchdown (also a Windows notification) |
| `open-flight` | `{ flight_id: string }` | The window was opened on a flight (a landing notification was clicked): show its debrief |

Flights are recorded to `Documents\FOQA-Sim\flights`. A closed flight's
journal is compacted to a `.fqz` file (about a tenth of the size) and the
journal deleted; flights are kept until the user deletes them. Raw captures
are off unless the app starts with `FOQA_RAW=1`. `list_flights` and
`debrief` analyse the journals and compact flights there; a flight still being recorded is
listed as recorded so far. A landing is reported 12 s of sim time after its
last touchdown (the recorder's ±10 s full-rate window, plus the airport
snapshot), so a bounce gives one notice; a flight closed sooner reports it
when it is saved.

## Types

```ts
type Level = "advisory" | "low" | "medium" | "high";

type RecorderStatus =
  | { state: "waiting_for_sim" }                 // MSFS not running
  | { state: "connected" }                       // connected, no flight open
  | { state: "recording"; flight_id: string; elapsed_s: number }  // wall-clock seconds, whole
  | { state: "error"; message: string };

interface StorageUsage {
  flights: number;        // stored flights
  bytes: number;          // every file in the folder
  folder: string;         // absolute path
}

interface Aircraft {
  icao?: string;          // ICAO type designator, e.g. "C172"
  title: string;          // sim TITLE, e.g. "C172SP G1000 Passengers"
}

interface FlightSummary {
  id: string;             // flight file stem, e.g. "flight-1793027213"
  started_utc?: string;   // ISO 8601, from the file name / zulu time
  duration_s: number;
  aircraft?: Aircraft;
  departure?: string;     // ICAO of the departure airport snapshot, if any
  arrival?: string;       // ICAO of the airport of the last touchdown, if any
  landings: number;
  worst_level?: Level;    // most severe event, if any
  event_counts: Record<Level, number>;
}

interface Debrief {
  flight: FlightSummary & {
    lfl_version: number;
    profile: string;        // e.g. "C172 profile", "transport class default (no B744 profile)"
    class?: "light" | "transport";   // the class the rules ran with; absent when unknown
    damaged_at?: number;    // byte offset if the journal was cut off
  };
  phases: { phase: string; start_s: number; end_s: number }[];   // CICTT, contiguous
  touchdowns: Touchdown[];
  events: Event[];          // time order, most severe first at equal times
  series: Series;
  airports: Airport[];      // runway geometry for the map
}

interface Touchdown {
  t_s: number;
  bounce: boolean;
  sink_fpm: number;
  sim_sink_fpm?: number;
  peak_g: number;
  pitch_deg?: number;
  roll_deg?: number;
  ias_kt?: number;
  runway?: {
    icao: string; runway: string;          // e.g. "LTBJ", "16L"
    along_m: number; cross_m: number;      // from the landing threshold; cross + = right
    lda_m: number; tdz_m: number;
    in_tdz: boolean; long_landing: boolean; short_landing: boolean;
  };
}

interface Event {
  rule: string; version: number; title: string; description: string;
  level: Level;
  start_s: number; end_s: number;
  peak_s: number;           // where `value` was measured; start_s for a condition
  measure: string;          // the rule's measure expression, for "why"
  value?: number; threshold?: number; unit?: string;
  why_it_matters?: string;  // plain-language consequence, from the rule
  advice?: string;          // plain-language technique to avoid it
  // Context at peak_s, e.g. "111 kt at flaps 12, limit 85 kt" for a flap
  // overspeed, whose value is only the excess over VFE:
  ias_kt?: number;
  flap_deg?: number;
  vfe_kt?: number;          // the profile's flap-speed limit at flap_deg
}
```

A **pilot report** is a moment the pilot marked from the in-sim panel
(`mark`, journal event code 7, [format.md](format.md)). It is listed among
the events as `{ rule: "pilot-report", version: 1, title: "Pilot report",
description: <the note, may be empty>, level: "advisory", start_s = end_s =
peak_s: <when>, measure: "" }` with the context fields, and counts in
`event_counts` like any advisory event.

```ts
// Parallel arrays on one time base: 2 Hz, plus every 8 Hz point inside the
// ±10 s touchdown windows. null where the parameter has no valid value.
interface Series {
  t_s: number[];
  alt_ft: (number | null)[];      // pressure altitude
  aal_ft: (number | null)[];      // height above the aerodrome
  ias_kt: (number | null)[];
  gs_kt: (number | null)[];
  vs_fpm: (number | null)[];      // inertial vertical speed
  nz_g: (number | null)[];
  pitch_deg: (number | null)[];
  roll_deg: (number | null)[];
  flap_deg: (number | null)[];
  vfe_kt: (number | null)[];      // profile's flap-speed limit at flap_deg; null without a profile band or with flaps up
  lat: (number | null)[];
  lon: (number | null)[];
}

interface Airport {
  icao: string;
  runways: {
    width_m: number;
    ends: { runway: string; lat: number; lon: number; heading_deg: number; lda_m: number; tdz_m: number }[]; // the two landing thresholds
  }[];
}

interface LandingNotice {
  flight_id: string;
  touchdown: Touchdown;     // the landing's first touchdown; a bounce follows it
  events: Event[];          // events from the start of its approach to 10 s after touchdown
}
```
