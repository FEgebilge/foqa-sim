# FOQA-Sim toolbar panel

An MSFS 2024 in-game panel that shows what the app sees: live approach monitor, events,
last landings, and buttons to end, discard or mark the recording. Data comes from the app
over the SimConnect Comm Bus, so the panel does nothing without the app running.

## Build

`assemble.ps1` writes both Community packages, this panel and the [EFB app](#efb-app),
to `msfs-panel\out\Packages` without the SDK or the sim: the compiled panel definition
comes from `prebuilt\InGamePanel_FOQASim.spb`, the rest from the sources. The app's
cargo build runs it whenever its inputs change and bundles the result, so it rarely
needs running by hand:

```powershell
powershell -ExecutionPolicy Bypass -File msfs-panel\assemble.ps1
```

`build.ps1` recompiles the panel definition with the SDK's `fspackagetool` and refreshes
`prebuilt\`; run it after changing `foqa-sim-panel.xml`, `PackageDefinitions` or
`PackageSources\InGamePanels`, then commit the new `.spb`. Close MSFS first: the tool
compiles in a sim instance of its own and does nothing while one is running.

```powershell
powershell -ExecutionPolicy Bypass -File msfs-panel\build.ps1
```

## Install

The app installs, updates and repairs both packages from its Installation screen. By
hand, only as a fallback: copy `out\Packages\foqa-sim-panel` into your Community folder
(MS Store build:
`%LOCALAPPDATA%\Packages\Microsoft.Limitless_8wekyb3d8bbwe\LocalCache\Packages\Community`).
Start a flight, open the toolbar and click the FOQA-Sim button (enable it under the toolbar
cog if it's not there).

## Work on the UI without the sim

Serve the repo root and open `/msfs-panel/dev/`; it replays a recorded flight through a
stubbed Comm Bus.

```powershell
python -m http.server 8765
```

`foqa-board.js` has no sim dependencies so an EFB app can reuse it. Messages:
[docs/ingame-panel.md](../docs/ingame-panel.md). Debugging in the sim: enable the
Coherent debugger and open `http://localhost:19999`.

## EFB app

The same board on the EFB tablet (Live), plus the saved flights and a compact
debrief of each (Flights). Sources in `efb/`; the board and its stylesheet
come from the toolbar panel's sources at build time. An EFB app is plain
files, so the build needs no SDK and the sim may stay running; `assemble.ps1`
runs it, or alone:

```powershell
& msfs-panel\efb\build.ps1
```

Output: `msfs-panel\out\Packages\foqa-sim-efb`; FOQA-Sim appears among the EFB
apps once it is in the Community folder. UI without the sim:
`/msfs-panel/dev/efb.html`, which answers the history queries from
`app/public/mock` in chunks and can drop messages.
