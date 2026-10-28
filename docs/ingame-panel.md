# In-game panel

The FOQA-Sim toolbar panel shows the recorder's state and each landing
inside MSFS 2024, where Windows notifications are not visible in fullscreen
or VR. The EFB app shows the same board on the tablet, plus the saved
flights. The desktop app sends; the views only render.

```
recorder service ─ Happening ─► panel feed (crates/capture/src/panel.rs)
                                  │ own SimConnect connection
                                  ▼ SimConnect_CallCommBusEvent(…, BROADCAST_TO_JS, json)
                                MSFS Comm Bus ─► RegisterCommBusListener().on(name, cb)
                                                  msfs-panel: foqa-board.js, foqa-history.js
```

The feed opens a SimConnect connection of its own ("FOQA-Sim panel feed")
while the recorder is connected, so publishing never touches the recording
connection. It needs MSFS 2024 SU5 or later (the Comm Bus; SDK 1.8.1).
`foqa-rec` (the CLI) has no feed.

A closed panel runs no JavaScript and cannot open itself: the pilot opens
it once per session and leaves it open. It is a fixed-size window (25 % of
the screen wide, 40 % tall by default, resizable down to 15 % × 18 %); the
content scrolls inside when the window is made smaller.

## Board

A header and four tabs. The header shows the recorder state (`REC 12:34`,
"Ready · no flight", "FOQA-Sim app not connected" after 4 s without a
status), aircraft and departure, the live phase (cyan while in Approach or
Landing) and a **Mark** button while recording.

| Tab | Shows |
|---|---|
| Now | Live values. In Approach/Landing: height AAL, IAS with the VFE at the current flap (amber within 5 kt, red above), V/S large; flaps and bank small; the 1000 ft and 500 ft gates as ahead ("in 260 ft"), stable or unstable with the failing checks ("Sink 1467 fpm, limit 1000 fpm"). Otherwise the five values compact, the latest 3 events and the last landing. Entering the approach switches to this tab. |
| Events | Counts per level and every event of the flight, newest first, with its time and value in words ("111 kt at flaps 12°, limit 85 kt", "1568 fpm sink, limit 1500 fpm"). |
| Landings | The newest landing as a card (sink, peak g, IAS, TDZ/long/short, bounce, distance past the threshold and off the centreline, top 3 events of its approach), the two before it as rows. A landing at most 120 s old opens this tab by itself; `×` goes back to the tab before. |
| Flight | Aircraft, departure, time, phase, landings, event counts, flight id; **Mark event**, **End flight & save**, **Discard flight…** (a second click within 6 s confirms). |

Under the tabs, the **attempt strip** (`foqa-strip.js`) shows the current
challenge attempt in the app's words ("Attempt 2 · Landed 142 fpm · Graded
87.4 · Scored 87.4, #3") with **Finish** while it flies and **Fly again**
(over an open attempt a second click within 6 s confirms, since it abandons
it); it goes 5 s after the app stops sending it. See Challenges below.

A new event flashes in a banner under the header for 8 s (click opens
Events). Events already there when the panel opens do not flash. The app's
`ack` shows as a green or red line at the bottom for 6 s; no ack within 8 s
shows "No reply from the FOQA-Sim app".

## Package

Source in `msfs-panel/`:

| Path | |
|---|---|
| `foqa-sim-panel.xml` | Package tool project; output in `msfs-panel/out` (git-ignored) |
| `PackageDefinitions/foqa-sim-panel.xml` | Package `foqa-sim-panel`: an SPB group (panel definition) and a Copy group (`html_ui`) |
| `PackageSources/InGamePanels/InGamePanel_FOQASim.xml` | Panel definition, compiled to `InGamePanel_FOQASim.spb`: id `PANEL_FOQA_SIM`, name "FOQA-Sim", toolbar icon `ICON_TOOLBAR_FOQA_SIM` |
| `PackageSources/html_ui/InGamePanels/FOQASim/` | `FOQASim.html` and `FOQASim.js` (sim glue: `ingame-ui` frame, Comm Bus listener), `foqa-board.js` (rendering, no sim APIs), `foqa-strip.js` (the attempt strip, `node efb/test/foqa-strip.test.js`), `FOQASim.css` |
| `PackageSources/html_ui/icons/toolbar/ICON_TOOLBAR_FOQA_SIM.svg` | Toolbar button |
| `efb/` | EFB app: `FoqaSimApp.js` (EFB glue, Live, Challenges and Flights tabs), `foqa-challenges.js` (Challenges, `node efb/test/foqa-challenges.test.js`), `foqa-history.js` (Flights, `node efb/test/foqa-history.test.js`), `foqa-weather.js` (weather setter, `node efb/test/foqa-weather.test.js`), `build.ps1` |
| `dev/index.html`, `dev/efb.html` | The board and the EFB app in a browser with a stubbed Comm Bus |

`foqa-board.js` is plain ES2015 with no sim dependency, so the EFB app
reuses it: `FoqaSim.attach(element, { on(name, cb), send(name, json) })`
(`FOQASim.js` wraps `RegisterCommBusListener`'s `on` and `callSimConnect`),
or `FoqaSim.landingView(notice)` for the data alone.

### Build

MSFS 2024 must be closed: `fspackagetool` builds in a sim instance of its
own, and beside a running one (MS Store build) it returns at once without
building.

```powershell
powershell -ExecutionPolicy Bypass -File msfs-panel\build.ps1
```

It runs `fspackagetool.exe msfs-panel\foqa-sim-panel.xml -nopause` from
`$env:MSFS2024_SDK` (default `C:\MSFS 2024 SDK`) and waits for
`msfs-panel\out\Packages\foqa-sim-panel\InGamePanels\InGamePanel_FOQASim.spb`.
The tool writes `manifest.json` and the `.spb`, then keeps waiting on a sim
that never starts (seen with SDK 1.8.1, sim 1.9.21), so the script stops it
and writes `layout.json` itself.

The EFB app is plain files, so `& msfs-panel\efb\build.ps1` needs no SDK and
the sim may stay running. It writes `msfs-panel\out\Packages\foqa-sim-efb`.
`msfs-panel\assemble.ps1`, which the app's cargo build runs, writes both
packages without the SDK, the panel definition from `prebuilt\` (which
`build.ps1` refreshes).

### Install

The app installs both packages from its Installation screen. By hand:

1. Build the packages (above).
2. Copy `msfs-panel\out\Packages\foqa-sim-panel` (and `foqa-sim-efb`) into
   the Community folder. For the MS Store build it is
   `%LOCALAPPDATA%\Packages\Microsoft.Limitless_8wekyb3d8bbwe\LocalCache\Packages\Community`
   (the root is `InstalledPackagesPath` in `UserCfg.opt`; that folder also
   has `Community2024`, which the SDK documents for native 2024 packages; if
   the panel does not appear from `Community`, move it there).
   DevMode → Tools → Virtual File System shows which folders the sim reads.
3. Start MSFS and the FOQA-Sim app, start a flight, open the toolbar (mouse
   to the top of the screen) and click the FOQA-Sim button; if it is not
   there, enable it under the toolbar's settings cog. Leave the panel open.
   FOQA-Sim also appears among the EFB apps.

### Develop without the sim

Serve the repository root over HTTP and open `/msfs-panel/dev/`:

```powershell
python -m http.server 8765   # in the repository root
```

The page stubs the Comm Bus and plays the app from
`app/public/mock/debrief-flight-1793037932.json`: it replays the series as
Live from en route (a flap overspeed), through the approach (both gates
unstable on sink, the 1000 ft gate also on speed) to the touchdown and the
landing notice 12 s later, sends the events as they peak and answers
commands with an ack ("Commands fail" makes it a failure). Two boards, the
default size (resizable) and a small one; buttons jump to each state, "App
gone" stops the status, "Reopen panels" opens them late. The two earlier
landings in the list are synthetic (the mock has one).

`/msfs-panel/dev/efb.html` runs the EFB app under a stand-in for the EFB OS
and answers the history queries from `app/public/mock` in parts; it can
drop messages, go silent or lower the size cap. In the sim, the Coherent GT
debugger shows the views' console.

## Messages

Comm Bus messages between the app (`crates/capture/src/panel.rs`, live
values in `monitor.rs`) and the views. JSON, UTF-8. Every message carries
`v: 2`. Types reuse [debrief-api.md](debrief-api.md) (`RecorderStatus`,
`Event`, `LandingNotice`, `Touchdown`, `Level`, `FlightSummary`).

- A view only hears what is sent while it is open, so every app → view
  message is repeated.
- A landing is identified by `notice.flight_id` and `notice.touchdown.t_s`.
- No status for 4 s: not connected. No Live for 2 s: the Now tab shows the
  state instead of values.
- **Size cap.** Each message is at most 6 KB (6144 bytes) as UTF-8 JSON,
  escaping and the terminating NUL included. Provisional until the real
  Comm Bus limit is measured.

### App → view

| Event | Payload | Cadence |
|---|---|---|
| `FOQA_SIM_STATUS` | `Status` | 1 Hz while the sim is connected; at once with an `ack` |
| `FOQA_SIM_LIVE` | `Live` | 4 Hz while recording |
| `FOQA_SIM_EVENTS` | `Events` | on change, else every 5 s while recording |
| `FOQA_SIM_LANDING` | `{ v, age_s, notice: LandingNotice }` | at the landing, then every 5 s |
| `FOQA_SIM_LANDINGS` | `{ v, landings: LandingNotice[] }` | every 5 s while recording; the last 3 landings of this flight, newest first |
| `FOQA_SIM_FLIGHTS` | `Flights` | reply to `list_flights`, every 1 s for ~5 s |
| `FOQA_SIM_DEBRIEF` | `DebriefPart` | reply to `debrief`, all parts every 1 s for ~5 s |
| `FOQA_SIM_WEATHER` | `{ v: 2, cmd: "set_weather", req, spec: WeatherSpec }` | EFB app only; see Weather. Not sent by any flow yet |
| `FOQA_SIM_ATTEMPT` | `AttemptStrip` | 1 Hz while an attempt is open and for 10 min after it reached a final state; at once on change. See Challenges |
| `FOQA_SIM_CHALLENGES` | `{ v, req, challenges: ChallengeCard[] }` | reply to `list_challenges`, every 1 s for ~5 s |
| `FOQA_SIM_BRIEFING` | `{ v, req, key, title, lines, preflight, error? }` | reply to `briefing`, every 1 s for ~5 s |

`EVENTS` and `LANDINGS` come from the app's own analysis of the open
journal, run every 5 s in the background (one at a time), so they trail the
flight by up to about 5 s plus the analysis time; the first arrives a few
seconds into the flight.

```ts
interface Status {
  v: 2;
  status: RecorderStatus;
  flight?: {                 // while recording only
    id: string;
    aircraft?: string;       // ICAO type, else sim title (from the analysis)
    departure?: string;      // ICAO
    elapsed_s: number;       // wall clock
    phase: Phase;            // the latest Live.phase, "Standing" before any
    landings: number;        // from the analysis
  };
  ack?: { cmd: string; ok: boolean; message?: string };  // reply to a command, sent once; message on failure
}

type Phase = "Standing" | "Taxi" | "Takeoff" | "InitialClimb" | "EnRoute"
           | "Approach" | "Landing" | "GoAround";

interface Live {
  v: 2;
  t_s: number;               // sim time since the flight's first frame
  phase: Phase;              // live CICTT phase, see below
  on_ground: boolean;
  aal_ft: number | null;     // above the aerodrome of the last ground contact; 0 on the ground; null for a flight started airborne
  ias_kt: number | null;
  vs_fpm: number | null;     // inertial, + up
  flap_deg: number | null;   // trailing-edge flap angle
  vfe_kt: number | null;     // profile flap limit at this flap angle; null with flaps up or no profile band
  bank_deg: number | null;   // + right wing down
  gates: Gate[];             // [1000 ft, 500 ft] throughout Approach and Landing, else empty
}

interface Gate {
  at_ft: 1000 | 500;
  state: "ahead" | "stable" | "unstable";   // "ahead": not yet passed, checks are the current values
  checks: Check[];           // frozen when the gate is passed
}

interface Check {
  name: "sink" | "speed" | "bank" | "flaps";
  ok: boolean;
  value?: number;
  limit?: number;            // only where one exists
  unit: "fpm" | "kt" | "deg";
}

interface Events {
  v: 2;
  flight_id: string;
  events: Event[];           // all events of this flight so far, time order, pilot reports included
  counts: Record<Level, number>;
}
```

**Phase.** The analyzer's CICTT phases (`analyzer::phase`) computed frame by
frame by `analyzer::LiveTrack`, with the same boundaries (`docs/phases.md`:
the class defaults or the type profile's, as known so far, plus 35 ft
takeoff end and 3 s bounce). The ground segment decides some boundaries from
what follows (the top of a climb, a go-around, a bounce, the lowest speed of
a touch-and-go); live, they are decided
once they have happened, so the debrief's phases can differ by a few
seconds. `aal_ft` uses the analyzer's aerodrome height (pressure altitude on
the ground, where the airport snapshots are captured): the departure, or
the last landing after a touch-and-go, as the analyzer has it for a flight
not yet landed.

**Gates.** FSF ALAR's 1000 ft (IMC) and 500 ft (VMC) gates, the same
criteria at both. A gate is passed at the first airborne frame at or below
its height in Approach/Landing; both reset when the phase leaves Approach /
Landing (a go-around). No new thresholds: each check uses a rule of
`rules/core.yaml` and the aircraft profile.

| Check | Value | Limit and `ok` |
|---|---|---|
| `sink` | `-vs`, fpm | rule `unstable-sink-500`: `ok` below its lowest level, `limit` 1000 |
| `speed` | IAS, kt | rule `flap-overspeed`: `limit` = profile VFE at the flap angle + its lowest level (0 kt); no `limit` and `ok` with flaps up or no profile |
| `bank` | `abs(roll)`, deg | rule `unstable-bank-500`, `limit` 7 |
| `flaps` | flap angle, deg | `ok` when extended (beyond the profile tolerance, 0.5 deg); no `limit` |

A check is left out when its value is unknown or its rule does not apply to
the aircraft class.

### View → app

Sent with `listener.callSimConnect("FOQA_SIM_CMD", json)`; the app
subscribes with `SimConnect_SubscribeToCommBusEvent` on the panel feed's
connection and ignores trailing NULs.

| Event | Payload | |
|---|---|---|
| `FOQA_SIM_CMD` | `{ v: 2, cmd: "start_flight" }` | record a new flight from the next frame, while none is open |
| | `{ v: 2, cmd: "end_flight" }` | close and save the flight now, as on return to the menu |
| | `{ v: 2, cmd: "discard_flight" }` | close the flight and delete its files (the view asks to confirm first) |
| | `{ v: 2, cmd: "mark", note?: string }` | mark this moment in the journal (pilot report) |
| | `{ v: 2, cmd: "list_flights", req: string }` | the saved flights, answered in `FOQA_SIM_FLIGHTS` |
| | `{ v: 2, cmd: "debrief", req: string, id: string }` | one flight's compact debrief, answered in `FOQA_SIM_DEBRIEF` |
| | `{ v: 2, cmd: "list_challenges", req }` | the app's challenge library, answered in `FOQA_SIM_CHALLENGES` |
| | `{ v: 2, cmd: "briefing", req, key }` | one challenge's briefing and pre-flight card, answered in `FOQA_SIM_BRIEFING` |
| | `{ v: 2, cmd: "place", key, practice?: boolean }` | create an attempt and place the aircraft, as the app's Place; `practice`: never uploaded |
| | `{ v: 2, cmd: "fly_again", id }` | abandon the current or last attempt (`replaced`) and place a new one on its challenge |
| | `{ v: 2, cmd: "finish", id }` / `{ v: 2, cmd: "abandon", id }` | the open attempt, as the app's buttons; `id` is the strip's, and another than the attempt shown now is refused |

Commands are acked once in `Status.ack`; the two queries are not (see
Flight history). The ack has `cmd`, `ok` and, on failure, `message`, e.g.
`"no flight open"`, `"unknown command"`, `"unsupported version"`,
`"not JSON: …"`; `cmd` is `"?"` when unreadable.

- `end_flight`: answered once the flight is saved; it is then analysed and
  compacted as usual (`flight-saved` in the app). The next flight opens at
  the sim's next flight start, as after the main menu, or on `start_flight`.
  If the recording is not a flight (under 10 s, or never left the menu
  position) it is deleted instead, not saved: the ack is `ok: false` with
  `message: "nothing to save: …"` and no `flight-saved` follows. A start
  works as usual after it.
- `discard_flight`: answered once the flight is closed and its `.fqj`,
  `.fqz` and `.raw.jsonl.gz` are deleted, nothing else.
- After either, `Status` has `state: "connected"` and no `flight`.
- `mark`: answered at once; the note (cut to 1000 bytes) is recorded as a
  pilot report and appears in the next `Events` as an advisory event
  `rule: "pilot-report"`, `title: "Pilot report"`, `description`: the note
  (see `debrief-api.md`).
- `start_flight`: answered at once; the flight opens at the next sim frame.
  It fails with `"a flight is already recording"` while one is open.
- Any other command while no flight is recorded fails with `"no flight open"`.

### Challenges (toolbar panel and EFB app)

The challenge part of the views (docs/challenge-protocol.md B9); types and
caps in `crates/capture/src/views.rs`, the words in
`app/src-tauri/src/insim.rs`. The app carries out `place`, `fly_again`,
`finish` and `abandon` exactly as its own buttons (one attempt open at a
time) and acks them in `Status.ack`; the two queries are answered like the
history's, with `req` echoed back, from the app's own library (no network
call from the sim). A key is `[a-z0-9-]`, at most 80 bytes; `req` plus `key`
at most 256 bytes. Each message fits the 6 KB cap: the strip's text and
reasons are cut to 160 characters, a card list to 12 cards and then to as
many as fit, a briefing to 12 lines of 120 characters and 6 pre-flight
lines, then briefing lines and pre-flight lines dropped from the end to
fit; a briefing that still does not fit is sent as its `error` ("This
briefing is too long for the sim; open it in the app.").

```ts
interface AttemptStrip {
  v: 2;
  id: string;                  // local attempt id
  key: string;                 // challenge's library key
  n: number;                   // attempt number on this challenge on this PC
  stage: "placing" | "flying" | "gate" | "rollout" | "finished" | "graded" | "abandoned" | "failed_start";
  landed?: { sink_fpm: number; peak_g: number };
  local?: { valid: boolean; score: number | null };
  site?: { state: "practice" | "uploading" | "waiting" | "scoring" | "scored" | "rejected" | "unverified"
                 | "expired" | "signed_out" | "unknown"; score?: number; rank?: number; reason?: string };
  text: string;                // "Attempt 2 · Landed 142 fpm · Graded 87.4 · Scored 87.4, #3"
  can_fly_again: boolean;      // not while placing
  can_finish: boolean;         // placed and not yet graded or abandoned
}
interface ChallengeCard { key: string; title: string; airport: string; runway: string;
  source: string;              // "File" or "<name> (<ascii host>)"
  state: "open" | "upcoming" | "closed" | "needs_update"; last?: string; rank?: number }
interface PreflightLine { name: "sim" | "aircraft" | "weather" | "ranking";
  state: "ok" | "warn" | "fail" | "unknown"; text: string; fix?: string }
```

The strip follows the open attempt, else the newest. The **EFB Challenges
tab** lists the cards (the challenge flown last first, with its last
attempt), opens a briefing (its facts, the pre-flight card, asked again
every 5 s while shown) with **Place** (off while a check fails) and **Fly as
practice**, and shows the strip with Fly again above. With no reply within
8 s it says so and asks again on a tap.

### Weather (EFB app)

`set_weather` sets the sim's weather through its own weather service
(`msfs-panel/efb/foqa-weather.js`, `FoqaWeather`; how and why in
[challenges.md](challenges.md#setting-the-weather-experiment-2026-10-27)).
It goes app to EFB on `FOQA_SIM_WEATHER` (repeated like every app message;
the EFB handles each `req` once, 6 KB cap as above) and is answered view to
app on `FOQA_SIM_CMD`. Types: `WeatherSpec`, `WeatherResult` in
`crates/capture/src/panel.rs` (`WeatherSpec::message`, `WeatherResult::parse`).
Status: built and unit-tested, not wired into the app's flows, not yet
verified in flight.

```ts
interface WeatherSpec {
  base: "clear" | number;    // "clear": the preset whose name contains CLEAR_SKY; else a preset index
  wind: { alt_ft: number; from_deg: number; kt: number }[];  // 1 to 8 layers, no gusts (the base's gusts are turned off)
  qnh_hpa?: number;          // 850..1085
  temp_c?: number;           // sea-level ground temperature, -80..60
  clouds?: [];               // only empty: the base preset's clouds stay
}                            // no visibility: the preset has no direct field

// EFB -> app, FOQA_SIM_CMD
interface WeatherResult {
  v: 2; cmd: "weather_result"; req: string;   // sent 3 times, 1 s apart
  ok: boolean;               // no mismatches
  applied: { wind: WindLayer[]; qnh_hpa?: number; temp_c?: number } | null;   // read back from the echoed preset, in spec units
  measured: { wind_dir_deg: number; wind_kt: number; qnh_hpa: number; temp_c: number; alt_ft: number } | null;   // SimVars after 4 s
  mismatches: string[];      // preset read-back and SimVars vs the spec, in words
  error?: string;            // instead of a report: bad spec, "no weather list", "busy", unknown unit, ...
}
```

The preset's wind layers past the spec's repeat its last layer (altitudes
stay the preset's); more spec layers than the preset has fails. Values are
converted from each field's `unit` (kt, m/s, km/h; ft, m; hPa, mbar, inHg;
°C, °F; rad, deg); an unknown unit fails with an error. The SimVar check
compares the spec's wind at the aircraft's altitude (interpolated between
layers) within 10 deg and 2 kt, QNH within 1.5 hPa, and temperature within
2.5 C after an ISA lapse from sea level.

### Flight history (EFB app)

`list_flights` and `debrief` are queries: they work with or without a flight
open and are answered on their own event with the view's `req` echoed back,
so a view can tell its reply from a repeat, an older reply or another
view's. A view ignores replies whose `req` is not the one it waits for, and
a new query replaces a waiting one. With nothing complete after about 8 s
it shows an error and **Try again**, which asks again with a new `req`.

The debrief view opens with the flight's landing-3 score and grade
([scoring.md](scoring.md)), six domain mini-bars of the grading landing
(track as long as the weight, the share of it lost in the level's colour,
labelled with the points lost) and
its largest deduction in one line; "How this was graded" still lists the
points method's parts. A flight without a scored landing shows its points.


```ts
interface Flights {
  v: 2;
  req: string;
  flights?: FlightItem[];    // saved (compacted) flights, newest first: at most 30, fewer to fit the size cap
  error?: string;            // instead of flights, e.g. "unsupported version"
}

// FlightSummary trimmed: `aircraft` as the ICAO type, else the sim title,
// `duration_s` in whole seconds, `event_counts` with levels that have events
// only, and of the assessment its grade only.
type FlightItem = Omit<FlightSummary, "aircraft" | "worst_level" | "assessment"> & {
  aircraft?: string;
  grade: "good" | "fair" | "poor";
};

interface DebriefPart {
  v: 2;
  req: string;
  id: string;                // the flight asked for
  seq?: number;              // 0-based
  total?: number;            // parts of this document
  part?: string;             // a fragment of the JSON text of one CompactDebrief, split on character boundaries to fit the size cap
  error?: string;            // instead of seq/total/part, e.g. "no such flight"
}

// What the parts join to (by req, in seq order): the debrief without airports,
// its series cut to the EFB's charts.
interface CompactDebrief {
  flight: FlightItem & {     // the assessment (debrief-api.md) and how it came about:
    profile: string;
    method: "landing-3" | "points";
    score?: number;          // landing-3, 0-100
    confidence?: "full" | "partial";
    // The grading landing's six domains in order (stability, energy, point,
    // lateral, contact, rollout) as mini-bars: [weight, points lost or null
    // when n/a, level 0-3, share of the weight lost 0-1 (0 when n/a)];
    // absent without a scored landing. The bar draws the share: the points
    // are rescaled when a domain is n/a.
    bars?: [number, number | null, number, number][];
    top?: string;            // its largest deduction in one line, the domain's `set_by_text`, e.g. "Approach stability −17.3: Descent rate 1,562 fpm at or above 300 ft"
    points: number; findings: number;   // the points method's, for one release

    parts: { title: string; level: Level; points: number; count: number }[];   // only rules with points, most first
    bands: GradeBand[];   // the grade scale (debrief-api.md)
  };
  touchdowns: Touchdown[];
  phases: { phase: string; start_s: number; end_s: number; trigger: string }[];   // as in the debrief (debrief-api.md); the Phases list
  events: {                  // time order
    rule: string; level: Level; title: string;
    start_s: number; peak_s: number;
    measure: string;         // for the value-vs-limit text
    value?: number; threshold?: number; unit?: string;
    detail?: string;         // the event in words (Event.detail)
    why_it_matters?: string; advice?: string;
    note?: string;           // pilot reports: the note (Event.description)
  }[];
  approaches: Grid;          // one per touchdown, same order: 180 s before to 10 s after it, every 2 s;
                             // values aal_ft, vs_fpm (to 10), ias_kt
  profile: Grid;             // the whole flight in ~150 steps of whole seconds; values alt_ft (to 10), ias_kt
}

// A uniform time grid: values[k] is at start_s + k * step_s, the series'
// nearest point within a step, rounded; null without one. No values when
// the flight has no series.
interface Grid {
  start_s: number; step_s: number;
  values?: Record<string, (number | null)[]>;
}
```
