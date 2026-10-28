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
| `trends(filter: TrendFilter)` | `Trends` | The counted flights aggregated ([trends.md](trends.md)), with `recordings: {total, counted, ground_only, too_short, partial}` saying what was left out; `filter` is `{aircraft?, airport?, days?}` |
| `delete_flight(id: string)` | `void` | Deletes the flight's stored files for good; refused for the flight being recorded |
| `place_challenge(definition: string)` | `StartState` | Puts the aircraft at a landing challenge's start (the challenge JSON, [challenges.md](challenges.md)); an error while the sim is not connected or for a `self` start |
| `recorder_status()` | `RecorderStatus` | Current state, also pushed as an event |
| `open_flights_folder()` | `void` | Opens the folder in Explorer |
| `open_log_folder()` | `void` | Opens the folder holding `foqa-sim.log` (stderr of the app and its crates; rotated at 2 MB) |
| `update_mark_clear()` | `void` | Removes the mark `update_mark` left; call it when the download or install fails |
| `storage_usage()` | `StorageUsage` | Flight count and the folder's total size on disk (raw captures included) |
| `setup_status()` | `SetupStatus` | The [installation](#installation) checks |
| `setup_install(package: string)` | `SetupStatus` | Installs, updates or repairs an in-sim package, then checks again |
| `setup_remove(package: string)` | `SetupStatus` | Removes an in-sim package from the Community folder |
| `setup_use_community(path: string \| null)` | `SetupStatus` | Uses that Community folder from now on; `null` goes back to the found install's |
| `setup_pick_community()` | `SetupStatus` | Asks for the Community folder in a folder dialog; cancelling changes nothing |
| `export_flight(id, format, identity)` | `string \| null` | Saves the flight as `parquet`, `csv`, `kml` or `gpx` where a save dialog says ([exports.md](exports.md)); the file written, `null` when cancelled. `identity: true` adds the aircraft's title and tail number |
| `export_logbook(identity)` | `string \| null` | Saves the flight list as a Little Navmap logbook CSV, the same way |
| `save_image(name, png)` | `string \| null` | Saves a PNG the window drew (the landing card, `png` its bytes) under a name the save dialog suggests; anything but a PNG is refused |
| `challenges_list()` | `ChallengeEntry[]` | The local challenge library, by title |
| `challenge_import()` | `ChallengeEntry \| null` | Asks for a challenge file, checks it with `Challenge::parse` and keeps it; `null` when cancelled; errors in plain words |
| `challenge_remove(key)` | `void` | Forgets a challenge; its attempts stay |
| `challenge_briefing(key)` | `Briefing` | The entry and its definition as written |
| `challenge_preflight(key)` | `PreflightLine[]` | The pre-flight card ([protocol](challenge-protocol.md) B8): sim, aircraft, weather, ranking |
| `attempt_start(key)` | `AttemptRecord` | Creates an attempt and places the aircraft (`place_challenge`'s path), or arms a `self` start; a failed placement leaves it `failed_start` |
| `attempt_fly_again(id)` | `AttemptRecord` | Abandons the attempt if still open (`replaced`) and starts the same definition again |
| `attempt_finish(id)` | `AttemptRecord` | Ends the attempt and grades it from its flight (`challenge::evaluate`) |
| `attempt_abandon(id)` | `AttemptRecord` | Abandons an open attempt (`cancelled`) |
| `attempts_list()` | `AttemptView[]` | Every attempt with its local grade and log, newest first |
| `deep_link_pending()` | `DeepLink \| null` | What to ask about next ([protocol](challenge-protocol.md) A3, B2), asked while no link is shown: a refused link's words or the link waiting, each given once; also in builds without `connect` (`state: "disabled"`) |
| `deep_link_consent(seq, allow)` | `SiteView \| null` | `connect` only: the answer to "Connect to <host>?" for link `seq`; allowed, the site is added (nothing is fetched from it before) |
| `deep_link_open(seq, replace)` | `{ entry: ChallengeEntry \| null, conflict: ChallengeEntry \| null }` | `connect` only: the Open click for link `seq` (its site signed in): takes the challenge in the link's version; `conflict` when another copy is in the library (kept until `replace`) |
| `deep_link_done(seq)` | `void` | Link `seq` was acted on or dismissed; the next one waits |
| `site_settings()` / `site_settings_set(settings)` | `{ links, notify }` | `connect` only: "Open challenge links from websites" and "Notify when a site scores an attempt" (B3), kept in the config folder |

## Events (backend → frontend)

| Event | Payload | |
|---|---|---|
| `recorder-status` | `RecorderStatus` | Whenever the state changes, and each second while recording (`elapsed_s`) |
| `flight-saved` | `FlightSummary` | A flight was closed and analysed |
| `landing` | `LandingNotice` | Seconds after a touchdown (also a Windows notification) |
| `open-flight` | `{ flight_id: string }` | The window was opened on a flight (a landing notification was clicked): show its debrief |
| `attempt-changed` | `AttemptRecord` | An attempt changed state |
| `deep-link` | `null` | A link from a website arrived and the window is brought up: ask `deep_link_pending` unless a link is shown |
| `open-challenge` | `{ key: string }` | A site's verdict notification was clicked: show that challenge |

Challenges and attempts are kept in `%APPDATA%\com.egebilge.foqa-sim`: `challenges\<id>-v<version>.json` (the bytes imported) and `attempts\<uuid>\` with `attempt.json`, `challenge.json`, `result.local.json` and the append-only `log.jsonl` (protocol B5; the types are in `app/src/types.ts`). Until the recording carries the attempt marker, an attempt grades only a landing after the sim time its flight had reached when it was placed (`begin_s`). After a landing the attempt is graded 120 s later, on Finish, or when the flight closes; a flight closed before the landing abandons it. At start-up an attempt left between placing and its grade is graded from its flight (`recovered`) if it landed, else abandoned (`interrupted`).

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
- **After an update** the app, on start, installs again each listed package
  that is missing (the old version's uninstaller removed it) or older than the
  one it ships, unless MSFS is running; then the check offers the update. A
  package that was never installed is left to the user.

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
  landings: number;       // touchdowns that are not bounces (see Touchdown.bounce)
  worst_level?: Level;    // most severe event, if any
  event_counts: Record<Level, number>;
  assessment: Assessment;
  landings_detail?: LandingFacts[];                      // per landing (bounces grouped), time order; what trends.md reads
  rule_counts?: Record<string, { level: Level; count: number }>;   // rules fired at low or above, by rule id
  skills?: SkillsSummary;   // the flying-skills figures trends reads (docs/skills.md); absent on older summaries
  flown?: Flown;            // what trends counts a flight by (trends.md); absent on older summaries: not flown
}

// How much of the recording was real flying, from the airborne legs, `sim.slew` points excluded.
interface Flown {
  airborne_s: number;
  distance_nm: number;      // ground speed integrated over the airborne time
  max_gs_kt: number;
  started_airborne: boolean;   // the first leg begins at the first track point
  ended_airborne: boolean;     // the last leg ends at the last track point
}

// One landing's facts for the trends ([trends.md](trends.md)); `Debrief.flight` carries them too.
interface LandingFacts {
  t_s: number;               // first touchdown
  full_stop: boolean;        // false: touch-and-go
  icao?: string; runway?: string;          // matched runway
  score?: number; grade?: "good" | "fair" | "poor"; confidence?: "full" | "partial";   // landing-3; none when unscored
  domains: Record<"stability" | "energy" | "point" | "lateral" | "contact" | "rollout", number | null>;   // level 0-3, null: n/a; empty when unscored
  sink_fpm: number; peak_g: number;        // first contact
  along_m?: number; cross_m?: number;      // past the threshold; right of the centreline positive
  zone?: "In TDZ" | "Long" | "Short";
  crosswind_kt?: number;     // right positive
  tailwind_kt?: number;      // negative: a headwind
  bounces: number;
}

// The flight graded as a whole, as the flight list shows it: by landing-3
// (docs/scoring.md) when a landing was scored, else by the points method.
// `points` and `findings` are the points method's, kept for one release:
// each rule the flight broke counts once, at its worst level, high 4
// points, medium 2, low 1, advisory 0 (good 0-1, fair 2-5, poor 6+).
// Summaries cached before landing-2 have no `method`.
interface Assessment {
  grade: "good" | "fair" | "poor";
  points: number;
  findings: number;       // rules broken at low or above
  method: "landing-3" | "points";
  score?: number;         // 0-100, to 0.1, with a scored landing
  confidence?: "full" | "partial";
}

// How the assessment was reached (docs/scoring.md §7.5). Each landing is
// scored in six domains; the flight takes the score of its last full-stop
// landing, held down by an earlier landing's safety cap and by high
// findings outside every landing (one: at most Fair, two: Poor). Without a
// scored landing, `method` is "points" and the parts and bands grade it.
interface AssessmentDetail {
  method: "landing-3" | "points";
  score: number | null;
  grade: "good" | "fair" | "poor";
  confidence: "full" | "partial" | null;
  caps: CapNote[];           // the grading landing's caps, then the flight's own (kind "flight")
  graded_by: number | null;  // index in `landings` of the landing that sets the grade
  landings: LandingAssessment[];   // one per landing (touchdowns that are not bounces), in time order
  other_findings: number[];  // indices in `Debrief.events` outside every scored landing (no pilot reports)
  legacy_points: number;     // the points method's, for one release
  legacy_findings: number;
  parts: AssessmentPart[];   // points method: most points first, then by rule name; advisories (0 points) last
  bands: GradeBand[];        // points method's scale, low to high: good up to 1, fair up to 5, poor above
  grades: { good: number; fair: number };   // landing-3's edges: Good from 85, Fair from 60
}

interface CapNote {
  kind: "material" | "unstable" | "inspection" | "undershoot" | "excursion" | "flight";
  score: number;             // the most the score can be
  title: string;             // the headline, e.g. "Unstable approach continued to a landing"
  reason: string;            // what caused it, e.g. "level 2 or more: A2 above, high-sink-light"
  source: string;
  landing?: number;          // index in `landings`
}

interface LandingAssessment {
  t_s: number;               // its first touchdown
  status: "scored" | "insufficient";   // insufficient: a runway fix, peak g, sink or rollout position missing
  score: number | null;      // after the caps
  raw: number | null;        // before them
  grade: "good" | "fair" | "poor" | null;
  confidence: "full" | "partial" | null;
  class: "light" | "turboprop" | "jet" | null;   // the scoring class whose bands apply
  partial: string[];         // expected data that was missing, e.g. "No Vref for B738"
  partial_note: string | null;   // the same in plain words (which items were not graded); null when nothing is missing
  note: string | null;       // why an insufficient landing is not scored, in plain words
  facts: string[];           // class, reference speeds and stable height in words, for the line under the title
  missing: string[];         // required data that was missing (insufficient)
  full_stop: boolean;        // false: a touch-and-go, no rollout scored, does not set the flight grade
  caps: CapNote[];
  stable_from_ft: number | null;   // lowest height above which every approach criterion stayed within tolerance
  split_ft: number | null;   // the approach window's split height for its class (light 300, else 500); null when insufficient
  speeds: { vref_kt: number; vapp_kt: number; source: "profile" | "selected"; approximate: boolean; basis: string } | null;
  domains: DomainAssessment[];     // stability, energy, point, lateral, contact, rollout; empty when insufficient
  profile?: ApproachProfile;       // the approach against its reference ([reference-approach.md](reference-approach.md)); absent without a runway fix or an approach window
}

// The approach window's points as parallel arrays; NaN goes out as null.
interface ApproachProfile {
  t_s: number[];          // track time, oldest first
  dist_nm: number[];      // to the threshold along the extended centreline, positive before it
  height_ft: number[];    // above the runway, as graded
  gs_dot: number[];       // glideslope deviation, fly-up positive; null without a signal
  loc_dot: number[];      // localizer deviation, right positive; null without a signal
  path_deg: number;       // 3.0 (ICAO PANS-OPS standard path)
  tch_ft: number;         // 50
  top_ft: number;         // the window used for grading
  bottom_ft: number;
}

interface DomainAssessment {
  id: "stability" | "energy" | "point" | "lateral" | "contact" | "rollout";
  title: string;             // "Approach stability", …
  short: string;             // the title in a word or two, for small bars
  summary: string;           // what the domain looks at, in one line
  weight: number;            // 25, 10, 20, 15, 20, 10
  loss: number | null;       // 0-1 of the weight, its worst item's; null: n/a, its weight shared out
  points: number;            // points it cost, to 0.1
  level: number;             // 0-3, of its worst item
  criteria: CriterionAssessment[];
  findings: FindingAssessment[];   // its events inside the landing's window
  sets_by: string | null;    // the item that set the loss: "A2 above" or a rule id
  set_by_text: string | null;   // that item as the app and the EFB show it: "Descent rate 1,562 fpm at or above 300 ft", or its event's title
}

interface CriterionAssessment {
  id: string;                // A1…F2 (scoring/landing-3.yaml)
  title: string;
  height: "all" | "above" | "below";   // the half of the approach window (split 500 ft, light 300 ft)
  value: number;             // its worst value, in `unit`; offsets (A1, B1, B2, C1) signed
  value_text: string;        // `value` as shown: "+12 kt" (offsets signed), "1.48 g", "0.8 dot", "1,562 fpm", "L2"; C1 names its side: "46 ft before the target", "558 ft past the target"
  height_text: string | null;   // "at or above 500 ft" / "below 500 ft"; null for "all"
  unit: string | null;       // null: a level 0-3 (A6, E3)
  ell: number;               // continuous level 0-3: 0 ideal, 0.5 tolerance edge, 1-3 the FDM levels
  level: number;             // floor(ell)
  loss: number;              // 0-1 of the domain
  // [ideal, tolerance, L1, L2, L3] resolved for this landing; `above` grades
  // larger values, `below` smaller; null where the anchor does not exist
  bands: { above: (number | null)[]; below: (number | null)[] };
  bands_text: { above: (string | null)[]; below: (string | null)[] };   // each anchor as shown, like `value_text`
  standard: string;          // the published source, with "est" for FOQA-Sim choices
  sets: boolean;             // it set its domain's loss
  first_out_s?: number;      // a series: when it first left the tolerance band (ell past 0.5, persistence held)
  worst_s?: number;          // a series: when it was worst
}

interface FindingAssessment {
  event: number | null;      // index in `Debrief.events`
  rule: string;
  level: Level;
  ell: number;               // low 1, medium 2, high 3, advisory 0
  loss: number;
  sets: boolean;             // it set its domain's loss
}

interface AssessmentPart {
  rule: string;
  title: string;
  level: Level;              // its worst level in the flight
  points: number;            // high 4, medium 2, low 1, advisory 0
  count: number;             // how many times it fired
}

interface GradeBand {
  grade: "good" | "fair" | "poor";
  up_to?: number;            // inclusive; absent on the last, which takes any more
}

// Flying skills ([skills.md](skills.md)). A measure is `null` when its parameters are missing or
// the flight has no such phase, never 0. Times in s, to 0.1; `vertical_rms` and `lateral_rms` in m/s².
type RideBand = "not uncomfortable" | "a little uncomfortable" | "fairly uncomfortable"
              | "uncomfortable" | "very uncomfortable" | "extremely uncomfortable";

interface Skills {
  automation: {
    airborne_s: number;
    ap_s: number | null; at_s: number | null;   // engaged time within airborne_s
    by_phase: { phase: string; s: number; ap_s: number | null; at_s: number | null }[];   // phases with airborne time
    ap_off_ft: number | null;        // last autopilot disconnect of the final approach; null for an autoland
    manual_final_s: number | null;   // hand-flown, from that disconnect to touchdown
    autoland: boolean;               // still engaged at contact
  };
  ride: {
    vertical_rms: number | null; ride_band: RideBand | null;
    lateral_rms: number | null; sideslip_rms_deg: number | null;
    max_bank_deg: Record<string, number>;     // by phase name
    rotation_rate_dps: number | null;         // transport class only
  };
  fuel: {
    fuel_used_kg: number | null;     // null if fuel rose in between
    fuel_kg_h: number | null; fuel_kg_nm: number | null; taxi_fuel_kg: number | null;
    descent_level_s: number | null; descent_levels: number | null;
  };
  control: {
    manual_s: number | null;         // autopilot off, airborne
    reversals_per_min: { pitch: number | null; roll: number | null };
  };
}

// What the flight list keeps of them; `ap_share` and `at_share` are of the airborne time (0-1).
interface SkillsSummary {
  ap_share: number | null; at_share: number | null; ap_off_ft: number | null; autoland: boolean;
  vertical_rms: number | null; ride_band: RideBand | null;
  fuel_kg_h: number | null; fuel_kg_nm: number | null; descent_level_s: number | null;
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
    // The phase boundaries in effect (docs/phases.md) by name: roll_kt,
    // climb_end_ft, approach_ft, flare_ft, go_around_rise_ft,
    // go_around_drop_ft. `source` is "<ICAO> profile, <document>" where the
    // type profile sets it, else e.g. "light class default"
    phase_values: Record<string, { value: number; source: string; approximate?: boolean; note?: string }>;
    assessment_detail: AssessmentDetail;   // how `assessment` was reached, per landing

    damaged_at?: number;    // byte offset if the journal was cut off
  };
  // CICTT, contiguous. `trigger` says what opened the span with the value
  // there, e.g. "on the ground above 30 kt (34 kt)", "flare, below 20 ft
  // above the runway (19 ft RA)", "top of climb, under 1000 ft above the
  // aerodrome (820 ft AAL)"
  phases: { phase: string; start_s: number; end_s: number; trigger: string }[];
  touchdowns: Touchdown[];
  events: Event[];          // time order, most severe first at equal times
  series: Series;
  airports: Airport[];      // runway geometry for the map
  skills: Skills;           // flying skills, measures not a grade (docs/skills.md)
}

// A landing is a touchdown with `bounce: false` plus the `bounce: true`
// touchdowns that follow it, in time order: one landing, graded on its first
// contact, with further contacts. Ground flicker (a hop of a few samples) is
// not a touchdown.
interface Touchdown {
  t_s: number;
  bounce: boolean;        // a further contact of the preceding landing, not a landing of its own
  sink_fpm: number;
  sim_sink_fpm?: number;
  peak_g: number;
  pitch_deg?: number;
  roll_deg?: number;
  ias_kt?: number;
  wind?: {                                 // at contact, on the matched runway; absent without a runway fix or wind data
    from_deg: number; speed_kt: number;    // true direction the wind blows from
    headwind_kt: number;                   // along the runway; negative = tailwind
    crosswind_kt: number;                  // across it; + = from the right, - = from the left
  };
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
  aal_ft: (number | null)[];      // height above the aerodrome, derived
  ias_kt: (number | null)[];
  gs_kt: (number | null)[];
  vs_fpm: (number | null)[];      // inertial vertical speed, derived from pressure altitude and load
  nz_g: (number | null)[];
  pitch_deg: (number | null)[];
  roll_deg: (number | null)[];
  hdg_deg: (number | null)[];     // magnetic heading
  ra_ft: (number | null)[];       // radio height, on-ground bias removed; null above 2500 ft
  flap_deg: (number | null)[];
  vfe_kt: (number | null)[];      // profile's flap-speed limit at flap_deg; null without a profile band or with flaps up
  gear_ext: (number | null)[];    // gear.extended, 0 = up to 100 = down (%)
  ap_engaged: (0 | 1 | null)[];   // autopilot engaged
  at_engaged: (0 | 1 | null)[];   // autothrottle engaged
  lat: (number | null)[];
  lon: (number | null)[];
}
// Values go out rounded to what the app shows: t_s and nz_g 3 decimals,
// pitch and roll 2, lat/lon 6 (about 0.1 m), vs_fpm and the engaged flags
// whole, the rest 1.

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
