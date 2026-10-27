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
| `setup_status()` | `SetupStatus` | The [installation](#installation) checks |
| `setup_install(package: string)` | `SetupStatus` | Installs, updates or repairs an in-sim package, then checks again |
| `setup_remove(package: string)` | `SetupStatus` | Removes an in-sim package from the Community folder |
| `setup_use_community(path: string \| null)` | `SetupStatus` | Uses that Community folder from now on; `null` goes back to the found install's |
| `setup_pick_community()` | `SetupStatus` | Asks for the Community folder in a folder dialog; cancelling changes nothing |

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
are off unless the app starts with `FOQA_RAW=1`. `debrief` analyses the
flight's file; `list_flights` analyses only flights that are new or changed
since they were last listed, keeping the summaries of compact flights in the
folder's `index.json` ([format.md](format.md#flight-index)), which the app
brings up to date in the background at start-up. A flight still being
recorded is listed as recorded so far. `fdm list <folder>` prints the same
list. A landing is reported 12 s of sim time after its
last touchdown (the recorder's ±10 s full-rate window, plus the airport
snapshot), so a bounce gives one notice; a flight closed sooner reports it
when it is saved.

## Installation

The app's built-in test: whether everything the recording and the in-sim
views need is in place, each failing check with its fix.

- **Sim.** MSFS 2024 is found by its `UserCfg.opt`: the MS Store build's in
  `%LOCALAPPDATA%\Packages\Microsoft.Limitless_8wekyb3d8bbwe\LocalCache`,
  Steam's in `%APPDATA%\Microsoft Flight Simulator 2024`. Its
  `InstalledPackagesPath "<root>"` line gives the root; packages go to
  `<root>\Community`, which the sim reads for every community package
  (`Community2024` beside it is optional and may be empty). With both builds
  installed the first is used, with a warning, until the user chooses one.
- **Chosen folder.** The install the user chooses (as `ms_store` / `steam`, so
  it follows that install's `InstalledPackagesPath`) or a folder they pick is
  kept in `%APPDATA%\com.egebilge.foqa-sim\community.txt`. A drive root is
  refused; a folder not named `Community` / `Community2024` warns.
- **Packages.** `foqa-sim-panel` (toolbar panel) and `foqa-sim-efb` (EFB app),
  shipped as resources in `packages/<name>`; debug builds use
  `msfs-panel\out\Packages\<name>` from [assemble.ps1](../msfs-panel/README.md).
  A folder of that name is ours when its `manifest.json` names `FOQA-Sim` as
  the manufacturer; any other is reported and never replaced or removed. Ours
  passes when its `package_version` is the shipped one and every file of the
  shipped `layout.json` is there with its size; an older one offers an update,
  a newer one warns.
- **Install** copies the shipped folder to `Community\.<name>.new`, moves the
  installed one aside to `.<name>.old`, renames the copy into place and deletes
  the old one; a failure leaves the installed one as it was. No other folder is
  touched. While the sim runs, its open files can make this fail (close the
  flight or the sim and try again) or leave `.<name>.old` behind: its
  `manifest.json` is then deleted so MSFS skips it, and the check reports it
  until a Repair deletes it. Whether a running sim picks up a change before its
  next start is not known. Every installed package folder is listed, one
  absolute path per line (UTF-16LE with BOM), in
  `%APPDATA%\com.egebilge.foqa-sim\community-packages.txt`, which the
  uninstaller reads; removing a package drops its line.

## Types

```ts
type Level = "advisory" | "low" | "medium" | "high";

type RecorderStatus =
  | { state: "waiting_for_sim" }                 // MSFS not running
  | { state: "connected" }                       // connected, no flight open
  | { state: "recording"; flight_id: string; elapsed_s: number }  // wall-clock seconds, whole
  | { state: "error"; message: string };

type CheckState = "pass" | "warn" | "fail";

interface SetupStatus {
  checks: SetupCheck[];     // in order: sim, community, package:<name>…, flights, recorder, sim_running
  sims: { store: "ms_store" | "steam"; user_cfg: string; community: string }[];  // every install found
  community: string | null; // the folder packages go to
  choice: "found" | "install" | "folder";  // the first install found, the install the user chose, the user's own folder
}

interface SetupCheck {
  id: string;               // "sim" | "community" | "package:<name>" | "flights" | "recorder" | "sim_running"
  title: string;
  state: CheckState;
  detail: string;           // the finding in words
  paths?: string[];         // the files and folders it looked at
  files?: string[];         // a package's files that are "<path> (missing)" or "<path> (changed)"
  fixes?: { action: "install" | "remove" | "pick_community"; package?: string; label: string }[];
}

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
    // The type profile's values that do not depend on the flight state, by
    // their rule name (`profile.<name>`), e.g. { hard_landing_g: { value: 2.2,
    // source: "…" } }; absent without a profile or values
    profile_limits?: Record<string, { value: number; source: string; note?: string }>;
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
    aiming_point_m: number;                // ICAO Annex 14 aiming point, by LDA
    in_tdz: boolean; long_landing: boolean; short_landing: boolean;
    zone: string;                          // "In TDZ" | "Long" | "Short"
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
  detail?: string;          // the event in words, from the rule's `detail`,
                            // e.g. "111 kt at flaps 12°, limit 85 kt"
}
```

A **pilot report** is a moment the pilot marked from the in-sim panel
(`mark`, journal event code 7, [format.md](format.md)). It is listed among
the events as `{ rule: "pilot-report", version: 1, title: "Pilot report",
description: <the note, may be empty>, level: "advisory", start_s = end_s =
peak_s: <when>, measure: "" }`, and counts in
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
