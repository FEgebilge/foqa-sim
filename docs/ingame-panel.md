# In-game panel

The FOQA-Sim toolbar panel shows the recorder's state and each landing
inside MSFS 2024, where Windows notifications are not visible in fullscreen
or VR. The desktop app sends; the panel only renders.

```
recorder service ─ Happening ─► panel feed (crates/capture/src/panel.rs)
                                  │ own SimConnect connection
                                  ▼ SimConnect_CallCommBusEvent(…, BROADCAST_TO_JS, json)
                                MSFS Comm Bus ─► RegisterCommBusListener().on(name, cb)
                                                  msfs-panel: foqa-board.js
```

The feed opens a SimConnect connection of its own ("FOQA-Sim panel feed")
while the recorder is connected, so publishing never touches the recording
connection. It needs a SimConnect with `SimConnect_CallCommBusEvent` (MSFS
2024 SU5 or later, SDK 1.8.1 has it); without it recording works and the
feed logs the missing symbol. `foqa-rec` (the CLI) has no feed.

A closed panel runs no JavaScript and cannot open itself: the pilot opens
it once per session and leaves it open. It is a fixed-size window (25 % of
the screen wide, 40 % tall by default, resizable down to 15 % × 18 %); the
content scrolls inside when the window is made smaller.

## Board (v2)

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
shows "No reply from the FOQA-Sim app". Values behind a flap overspeed
(IAS, flap, VFE) come from the Live samples the board heard at the event's
peak; a panel opened after the event shows the excess instead ("26 kt over
the flap limit").

## Messages

The app now sends v2: live values, events, landings and commands back,
[ingame-panel-v2.md](ingame-panel-v2.md). Version 1 below is kept for
reference.

The contract is [ingame-panel-v2.md](ingame-panel-v2.md): app → view
`FOQA_SIM_STATUS`, `FOQA_SIM_LIVE`, `FOQA_SIM_EVENTS`, `FOQA_SIM_LANDING`,
`FOQA_SIM_LANDINGS`; view → app `FOQA_SIM_CMD`, sent with the Comm Bus
listener's `callSimConnect(name, json)` (the app subscribes with
`SimConnect_SubscribeToCommBusEvent`). Types are those of
[debrief-api.md](debrief-api.md).

- A view hears only what is sent while it is open, so the app repeats
  everything.
- A landing is identified by `notice.flight_id` and `notice.touchdown.t_s`.
- No status for 4 s: not connected. No Live for 2 s: the Now tab shows the
  state instead of values.

## Package

Source in `msfs-panel/`:

| Path | |
|---|---|
| `foqa-sim-panel.xml` | Package tool project; output in `msfs-panel/out` (git-ignored) |
| `PackageDefinitions/foqa-sim-panel.xml` | Package `foqa-sim-panel`: an SPB group (panel definition) and a Copy group (`html_ui`) |
| `PackageSources/InGamePanels/InGamePanel_FOQASim.xml` | Panel definition, compiled to `InGamePanel_FOQASim.spb`: id `PANEL_FOQA_SIM`, name "FOQA-Sim", toolbar icon `ICON_TOOLBAR_FOQA_SIM` |
| `PackageSources/html_ui/InGamePanels/FOQASim/` | `FOQASim.html` and `FOQASim.js` (sim glue: `ingame-ui` frame, Comm Bus listener), `foqa-board.js` (rendering, no sim APIs), `FOQASim.css` |
| `PackageSources/html_ui/icons/toolbar/ICON_TOOLBAR_FOQA_SIM.svg` | Toolbar button |
| `dev/index.html` | The board in a browser with a stubbed Comm Bus |

`foqa-board.js` is plain ES2015 with no sim dependency, so an EFB app can
reuse it: `FoqaSim.attach(element, { on(name, cb), send(name, json) })`
(`FOQASim.js` wraps `RegisterCommBusListener`'s `on` and `callSimConnect`),
or `FoqaSim.landingView(notice)` and `FoqaSim.eventDetail(event, sample)`
for the data alone.

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

### Install

1. Build the package (above).
2. Copy `msfs-panel\out\Packages\foqa-sim-panel` into the Community folder.
   For the MS Store build it is
   `%LOCALAPPDATA%\Packages\Microsoft.Limitless_8wekyb3d8bbwe\LocalCache\Packages\Community`
   (the root is `InstalledPackagesPath` in `UserCfg.opt`; that folder also
   has `Community2024`, which the SDK documents for native 2024 packages; if
   the panel does not appear from `Community`, move it there).
   DevMode → Tools → Virtual File System shows which folders the sim reads.
3. Start MSFS and the FOQA-Sim app, start a flight, open the toolbar (mouse
   to the top of the screen) and click the FOQA-Sim button; if it is not
   there, enable it under the toolbar's settings cog. Leave the panel open.

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
landings in the list are synthetic (the mock has one). In the sim, the
Coherent GT debugger shows the panel's console.
