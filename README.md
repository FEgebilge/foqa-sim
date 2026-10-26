# FOQA-Sim

Flight data monitoring for Microsoft Flight Simulator 2024, modelled on how airlines do FOQA.

A tray app records every flight you fly, runs it through a rule engine afterwards and shows you
what happened: phases, landing numbers, runway position, unstable approaches, flap overspeeds,
hard landings. An in-sim toolbar panel shows the same thing live while you fly.

It follows the real split between the **aircraft segment** (a recorder that samples a fixed
parameter list, like a QAR) and the **ground segment** (analysis and rules that run on the
recording, never on live sim state). Everything runs locally; nothing leaves your PC.

> Status: early. Works end to end on my machine with MSFS 2024 (Windows, MS Store build).
> No installer yet.

## What you get

- **Recording**: automatic, starts and stops with the sim's flights. A 1 h flight is about 1.8 MB.
- **Debrief**: phase timeline, charts with reference lines, landing card with reference bands,
  runway diagram, map synced to the charts, events in plain language with what to do about them.
- **Rules**: 21 rules in YAML (`rules/core.yaml`), per-aircraft limits in `profiles/*.yaml`
  (C172, 737-8, 737-800, CJ4, Longitude so far).
- **In-sim panel**: live approach monitor with 1000/500 ft stabilised gates, event feed,
  last landings, and end/discard/mark controls for the recording.

## Requirements

- Windows 10/11, MSFS 2024
- [MSFS 2024 SDK](https://docs.flightsimulator.com/msfs2024/html/1_Introduction/SDK_Overview.htm)
  (for `SimConnect.dll`; looked up in `MSFS2024_SDK`, else `C:\MSFS 2024 SDK`)
- Rust (stable, MSVC toolchain), Node.js 22+

## Run it

```powershell
cd app
npm install
npm run tauri dev
```

The app sits in the tray, waits for the sim and records to `Documents\FOQA-Sim\flights`.
For the in-sim panel see [msfs-panel/](msfs-panel/README.md).

Without the sim, `npm run dev` in `app/` serves the UI against the mock flights in
`app/public/mock`.

## CLI

```powershell
cargo run -p cli --bin fdm -- debrief <flight.fqz>         # events, landings, runway fix
cargo run -p cli --bin fdm -- debrief <flight.fqz> --json  # the app's Debrief JSON
cargo run -p cli --bin fdm -- analyze <flight> [--rules my.yaml]
cargo run -p cli --bin fdm -- export <flight> [out_dir]    # Parquet
cargo run -p cli --bin fdm -- compact <file|dir>           # .fqj -> .fqz
cargo run -p capture --bin foqa-rec                        # headless recorder
```

## Layout

```
crates/
  fdm-core    parameter list (LFL), units, signals, geo
  sim-link    SimConnect bindings (hand-written FFI, DLL loaded at runtime)
  recorder    acquisition, journal (.fqj) and compact (.fqz) formats
  analyzer    phases, derived VS, touchdowns, runway matching, live tracking
  rules       rule language, engine, aircraft profiles
  debrief     the analysis the app and CLI show
  capture     live recording service and in-sim panel feed
  cli         fdm and foqa-airports
app/          Tauri 2 + React desktop app
msfs-panel/   MSFS toolbar panel package
rules/        core rule set
profiles/     aircraft limits
fixtures/     recorded test flight
docs/         formats and contracts
```

Docs worth reading first: [format.md](docs/format.md) (recording formats),
[rules.md](docs/rules.md) (rule language), [debrief-api.md](docs/debrief-api.md)
(backend ↔ UI contract), [ingame-panel-v2.md](docs/ingame-panel-v2.md) (app ↔ sim panel).

## Development

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm --prefix app run lint
npm --prefix app run build
```

CI runs the same on Windows.

Branches: work goes to `dev`, then `dev` → `test` → `main` through pull requests.

## License

Apache-2.0, see [LICENSE](LICENSE). Some UI components in `app/src/ui` are derived from
Arc UI (MIT), see [app/src/ui/NOTICE.md](app/src/ui/NOTICE.md).

Not affiliated with Microsoft or Asobo. Not for real-world flight operations.
