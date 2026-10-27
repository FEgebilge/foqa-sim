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
| `PackageSources/html_ui/InGamePanels/FOQASim/` | `FOQASim.html` and `FOQASim.js` (sim glue: `ingame-ui` frame, Comm Bus listener), `foqa-board.js` (rendering, no sim APIs), `FOQASim.css` |
| `PackageSources/html_ui/icons/toolbar/ICON_TOOLBAR_FOQA_SIM.svg` | Toolbar button |
| `efb/` | EFB app: `FoqaSimApp.js` (EFB glue, Live and Flights tabs), `foqa-history.js` (Flights), `build.ps1` |
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
frame by `analyzer::LiveTrack`, with the same thresholds (35 ft takeoff end,
1000 ft initial climb / approach, 50 ft flare, 200 ft go-around rise, 30 kt
roll, 3 s bounce). The ground segment decides some boundaries from what
follows (the top of a climb, a go-around, a bounce); live, they are decided
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
| `FOQA_SIM_CMD` | `{ v: 2, cmd: "end_flight" }` | close and save the flight now, as on return to the menu |
| | `{ v: 2, cmd: "discard_flight" }` | close the flight and delete its files (the view asks to confirm first) |
| | `{ v: 2, cmd: "mark", note?: string }` | mark this moment in the journal (pilot report) |
| | `{ v: 2, cmd: "list_flights", req: string }` | the saved flights, answered in `FOQA_SIM_FLIGHTS` |
| | `{ v: 2, cmd: "debrief", req: string, id: string }` | one flight's compact debrief, answered in `FOQA_SIM_DEBRIEF` |

Commands are acked once in `Status.ack`; the two queries are not (see
Flight history). The ack has `cmd`, `ok` and, on failure, `message`, e.g.
`"no flight open"`, `"unknown command"`, `"unsupported version"`,
`"not JSON: …"`; `cmd` is `"?"` when unreadable.

- `end_flight`: answered once the flight is saved; it is then analysed and
  compacted as usual (`flight-saved` in the app). The next flight opens at
  the sim's next flight start, as after the main menu.
- `discard_flight`: answered once the flight is closed and its `.fqj`,
  `.fqz` and `.raw.jsonl.gz` are deleted, nothing else.
- After either, `Status` has `state: "connected"` and no `flight`.
- `mark`: answered at once; the note (cut to 1000 bytes) is recorded as a
  pilot report and appears in the next `Events` as an advisory event
  `rule: "pilot-report"`, `title: "Pilot report"`, `description`: the note
  (see `debrief-api.md`).
- A command while no flight is recorded fails with `"no flight open"`.

### Flight history (EFB app)

`list_flights` and `debrief` are queries: they work with or without a flight
open and are answered on their own event with the view's `req` echoed back,
so a view can tell its reply from a repeat, an older reply or another
view's. A view ignores replies whose `req` is not the one it waits for, and
a new query replaces a waiting one. With nothing complete after about 8 s
it shows an error and **Try again**, which asks again with a new `req`.

```ts
interface Flights {
  v: 2;
  req: string;
  flights?: FlightItem[];    // saved (compacted) flights, newest first: at most 30, fewer to fit the size cap
  error?: string;            // instead of flights, e.g. "unsupported version"
}

// FlightSummary with `aircraft` as the ICAO type, else the sim title, and
// `duration_s` in whole seconds.
type FlightItem = Omit<FlightSummary, "aircraft"> & { aircraft?: string };

interface DebriefPart {
  v: 2;
  req: string;
  id: string;                // the flight asked for
  seq?: number;              // 0-based
  total?: number;            // parts of this document
  part?: string;             // a fragment of the JSON text of one CompactDebrief, split on character boundaries to fit the size cap
  error?: string;            // instead of seq/total/part, e.g. "no such flight"
}

// What the parts join to (by req, in seq order): the debrief without series,
// phases or airports.
interface CompactDebrief {
  flight: FlightItem & { profile: string };
  touchdowns: Touchdown[];
  events: {                  // time order
    rule: string; level: Level; title: string;
    start_s: number; peak_s: number;
    measure: string;         // for the value-vs-limit text
    value?: number; threshold?: number; unit?: string;
    detail?: string;         // the event in words (Event.detail)
    why_it_matters?: string; advice?: string;
    note?: string;           // pilot reports: the note (Event.description)
  }[];
}
```
