# FOQA-Sim

**Airline-style flight data monitoring for Microsoft Flight Simulator 2024.**

Airlines do not wait for an accident to learn. Under **FOQA** (Flight Operations Quality
Assurance, also called flight data monitoring), the recorder data of every routine flight is
downloaded and run through event checks: unstable approaches, hard landings, flap overspeeds,
tail-strike margins. Problems surface as trends long before they become incidents, and pilots
are debriefed without blame. ICAO requires such a programme for large aeroplanes, and it is one
of the most effective safety tools in commercial aviation.

FOQA-Sim brings that discipline to the simulator. It records every flight you fly the way an
aircraft's recorder does, analyses it the way an airline safety department would, and gives you
a debrief that says what happened, why it matters and what to do differently.

## Built like the real thing

- **Aircraft segment and ground segment.** A recorder samples a fixed parameter list (a
  Logical Frame Layout of 63 parameters at flight-data-recorder rates, 14 CFR 121.344) into a
  crash-safe journal; all analysis runs afterwards on that recording, never on live sim state.
  Everything stays on your PC.
- **Rules from aviation sources.** 26 event definitions in a small, unit-checked rule language
  (`rules/core.yaml`). Each threshold names its source: EASA EOFDM, Flight Safety Foundation
  ALAR, UK CAA CAP 739, manufacturer manuals. Estimates are marked as such.
- **Aircraft-aware limits.** 22 type profiles, from the C172 and Vision Jet to the 737, A320neo
  family and 787, carry VFE by flap angle, VMO, gear speeds, tail-strike pitch, hard-landing
  loads and crosswind limits. Every value cites its document, and approximations say how they
  were derived.
- **Measured, not guessed.** Touchdowns from radio-height rate and full-rate normal load, bounce
  detection, runway matching against the sim's own airport database (distance past the
  threshold, touchdown zone, centreline offset), wind resolved into crosswind and tailwind on
  the runway heading, flight phases after the ICAO CICTT definitions.
- **Engineered as a product.** A Rust workspace (hand-written, statically linked SimConnect FFI;
  a compressed columnar flight format; unit tests, a recorded test flight and synthetic
  golden flights whose answers are known by construction), a Tauri + React desktop app, an in-sim toolbar panel and EFB tablet app talking to it
  over the SimConnect Comm Bus, and a one-step Windows installer.

## Status

Version 0.4. Works end to end with MSFS 2024 on Windows: installer, automatic recording,
debrief, in-sim panel and EFB app, local landing challenges. It is being tested with a small
group of pilots before a first public release. In progress: an FDR-style replay, challenges
with the same weather for every pilot, and the first website running the open challenge
protocol.

## What you get

- **Recording.** Automatic, starting and ending with the sim's flights; about 1.8 MB per hour.
- **Debrief.** Phase timeline, charts with reference lines, a landing card for every touchdown
  (bounces grouped with their landing), runway diagram, a map synced to the charts, and events
  in plain language with what to do about them.
- **Landing assessment.** A score out of 100 for every landing across six areas (approach
  stability, threshold and flare, touchdown point, lateral control, touchdown contact,
  rollout), each read against checkride tolerances (FAA ACS) and airline FDM thresholds (EASA
  Data4Safety, FSF ALAR), with safety caps and the item behind every deduction
  ([scoring.md](docs/scoring.md)). Missing data is reported as such, never as full marks.
- **Flight phases** after the ICAO CICTT definitions, following the aircraft (a light aircraft
  flares lower, turboprops fly a higher pattern), each with the reason it started.
- **Landing challenges.** Import a challenge, check the pre-flight card, place the aircraft on
  final and fly again from the sim. Optionally connect a website that runs the open
  [challenge protocol](docs/challenge-protocol.md) to rank attempts; FOQA-Sim works fully without one.
- **In-sim panel and EFB app.** Live approach monitor with the 1000 ft and 500 ft stabilised
  gates, event feed, recent landings with charts and scores, past flights, a Challenges tab
  with the attempt status, and controls to start, end, discard or mark a recording.
- **What's new.** After an update the app shows the release notes once
  ([devlog/](devlog/)).

## Install

Run the `FOQA-Sim_<version>_x64-setup.exe` installer. It needs Windows 10/11 and MSFS 2024,
and brings everything else (the VC++ runtime DLLs sit next to the exe). The app installs the
in-sim packages into your Community folder and keeps them up to date. Flights are kept in
`Documents\FOQA-Sim\flights` across updates.

## Build from source

Needs Visual Studio Build Tools with the C++ workload, the
[MSFS 2024 SDK](https://docs.flightsimulator.com/msfs2024/html/1_Introduction/SDK_Overview.htm)
(its static `SimConnect.lib` is linked in; looked up in `MSFS2024_SDK`, else
`C:\MSFS 2024 SDK`), Rust (stable, MSVC toolchain) and Node.js 22+.

```powershell
cd app
npm install
npm run tauri dev
```

The app sits in the tray, waits for the sim and records. Without the sim, `npm run dev` in
`app/` serves the UI against the mock flights in `app/public/mock`. For the in-sim packages see
[msfs-panel/](msfs-panel/README.md); for the installer, `npm run release -- <version>` in
`app/` (see [app/README.md](app/README.md)).

## CLI

```powershell
cargo run -p cli --bin fdm -- debrief <flight.fqz>         # events, landings, runway fix
cargo run -p cli --bin fdm -- debrief <flight.fqz> --json  # the app's Debrief JSON
cargo run -p cli --bin fdm -- analyze <flight> [--rules my.yaml]
cargo run -p cli --bin fdm -- export <flight> [out_dir]    # Parquet
cargo run -p cli --bin fdm -- compact <file|dir>           # .fqj -> .fqz
cargo run -p cli --bin fdm -- list <dir>                   # the app's flight list
cargo run -p cli --bin fdm -- challenge <challenge.json> <flight>  # a landing-challenge attempt
cargo run -p capture --bin foqa-rec                        # headless recorder
```

`fdm` also builds for Linux without the sim link (`fdm.Dockerfile`).

## Layout

```
crates/
  fdm-core    parameter list (LFL), units, signals, geo
  sim-link    SimConnect bindings (hand-written FFI, statically linked)
  recorder    acquisition, journal (.fqj) and compact (.fqz) formats
  analyzer    phases, derived VS, touchdowns, runway matching, live tracking
  rules       rule language, engine, aircraft profiles
  debrief     the analysis the app and CLI show, flight assessment
  challenge   landing challenges: definition, start on final, grading
  capture     live recording service, in-sim panel and EFB feed
  cli         fdm and foqa-airports
  testkit     synthetic flights for tests
app/          Tauri 2 + React desktop app
msfs-panel/   MSFS toolbar panel and EFB app packages
rules/        core rule set
profiles/     aircraft type limits
fixtures/     recorded test flight
docs/         formats, contracts and designs
```

Worth reading first: [format.md](docs/format.md) (recording formats),
[rules.md](docs/rules.md) (rule language and type profiles), [phases.md](docs/phases.md)
(flight phases), [debrief-api.md](docs/debrief-api.md) (backend and UI contract),
[ingame-panel.md](docs/ingame-panel.md) (app and in-sim panel),
[challenges.md](docs/challenges.md) (landing challenges).

## Development

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace
npm --prefix app run lint
npm --prefix app run build
```

Work goes to `dev`, then `dev` → `test` → `main`.

## License

Apache-2.0, see [LICENSE](LICENSE). Some UI components in `app/src/ui` are derived from
Arc UI (MIT), see [app/src/ui/NOTICE.md](app/src/ui/NOTICE.md).

Not affiliated with Microsoft, Asobo or any airline. For simulation only, not for real-world
flight operations.
