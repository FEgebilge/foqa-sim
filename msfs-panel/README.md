# FOQA-Sim toolbar panel

An MSFS 2024 in-game panel that shows what the app sees: live approach monitor, events,
last landings, and buttons to end, discard or mark the recording. Data comes from the app
over the SimConnect Comm Bus, so the panel does nothing without the app running.

## Build

Close MSFS first: `fspackagetool` compiles the panel definition in a sim instance of its own
and does nothing while one is running.

```powershell
powershell -ExecutionPolicy Bypass -File msfs-panel\build.ps1
```

Output: `msfs-panel\out\Packages\foqa-sim-panel`.

## Install

Copy `foqa-sim-panel` into your Community folder. MS Store build:
`%LOCALAPPDATA%\Packages\Microsoft.Limitless_8wekyb3d8bbwe\LocalCache\Packages\Community`.
Start a flight, open the toolbar and click the FOQA-Sim button (enable it under the toolbar
cog if it's not there).

## Work on the UI without the sim

Serve the repo root and open `/msfs-panel/dev/`; it replays a recorded flight through a
stubbed Comm Bus.

```powershell
python -m http.server 8765
```

`foqa-board.js` has no sim dependencies so an EFB app can reuse it. Messages:
[docs/ingame-panel-v2.md](../docs/ingame-panel-v2.md). Debugging in the sim: enable the
Coherent debugger and open `http://localhost:19999`.
