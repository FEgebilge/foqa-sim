# FOQA-Sim

Airlines go through the data of every flight to see how it was really flown. FOQA-Sim does the same for your Microsoft Flight Simulator 2024 flights, because why should they have all the fun.

Land, open the debrief and follow your flight on the map, moment by moment. Everything that went wrong is explained in plain words, along with what to do about it next time. Your landing gets a score out of 100, and it shows its work.

Think you can beat it? Fly the challenges: same approach, as many tries as you like, and a ranking if you want one.

Free and open source. Go easy on yourself after the first score.

## What it does

FOQA (Flight Operations Quality Assurance) is how airlines check their routine flights: the recorder data goes through event checks like unstable approaches, hard landings and flap overspeeds, trends show up, and pilots get debriefed without blame. Same idea here, for the sim.

- **It records by itself.** Recording starts and ends with your flights.
- **It's in the sim with you.** A toolbar panel and EFB app show a live approach monitor with the 1000 ft and 500 ft gates, events as they happen and your recent landings with their scores. You can start, end, discard or mark a recording from there too ([in-game panel](docs/ingame-panel.md)).
- **Replay any flight.** A map, a rebuilt primary flight display and charts all follow one time slider. Events are written in plain language with what to do about them, and every touchdown gets a landing card and runway diagram.
- **Your landings get scored.** Out of 100 across six areas, from approach stability to rollout, against FAA ACS tolerances and airline FDM thresholds, with the reason for every point lost ([scoring](docs/scoring.md)). Missing data is called missing, never full marks. You can see your approach against the profile it was graded by ([reference approach](docs/reference-approach.md)).
- **See how you fly.** Autopilot use by phase, cabin ride, level-offs in the descent, fuel, control activity while hand-flying. Numbers with their sources, not a grade ([skills](docs/skills.md)).
- **Watch the trend.** Landing scores over time, where you touch down on the runway and event rates per 100 flights, by aircraft and airport ([trends](docs/trends.md)).
- **Challenges.** Import one, start on final with its weather and time, and fly it again from the sim. Sign in to [Pilot Aptitude](https://pilotaptitude.egebilge.com), built in, if you want it ranked, over the open [challenge protocol](docs/challenge-protocol.md). FOQA-Sim works fully without it.
- **Take your data with you.** Any flight as CSV, KML, GPX or Parquet, your flight list as a Little Navmap logbook, a landing card as an image ([exports](docs/exports.md)). Written on your PC only.

## How it works

The work is split the way an airline splits it, into an aircraft segment and a ground segment. A recorder samples a fixed parameter list at flight-data-recorder rates (14 CFR 121.344) into a crash-safe journal, and the debrief is computed afterwards from that recording, never from live sim state.

45 event definitions in a small, unit-checked rule language (`rules/core.yaml`) name their sources: EASA EOFDM, Flight Safety Foundation ALAR, UK CAA CAP 739, manufacturer manuals. 28 aircraft type profiles, from the C172 and Vision Jet to the 737, A320neo family and 787, carry limits such as VFE by flap angle, VMO and tail-strike pitch, each citing its document. Touchdowns come from radio-height rate and full-rate normal load, runways are matched against the sim's own airport database, and flight phases follow the ICAO CICTT definitions.

It is a Rust workspace with hand-written, statically linked SimConnect FFI and a compressed columnar flight format, tested against recorded flights and synthetic flights whose answers are known by construction. A Tauri + React desktop app sits on top, and the in-sim views talk to it over the SimConnect Comm Bus.

## Install and use

**[Download for Windows](https://github.com/FEgebilge/foqa-sim/releases/latest/download/FOQA-Sim_x64-setup.exe)** (or pick a version on the [releases page](https://github.com/FEgebilge/foqa-sim/releases)).

It needs Windows 10/11 and MSFS 2024, and brings everything else. The app installs the in-sim packages into your Community folder and keeps them up to date. It sits in the tray, waits for the sim and records. Open the panel from the sim's toolbar once per session and leave it open. Flights are kept in `Documents\FOQA-Sim\flights` across updates. When a new version is out the app offers it with its release notes, never while a flight is recording.

The installer is not code-signed yet, so Windows SmartScreen may say "Windows protected your PC": choose **More info**, then **Run anyway**. To check the download, compare `Get-FileHash .\FOQA-Sim_x64-setup.exe` (PowerShell) with the SHA-256 in the release notes. Updates are signed with the project's own key and refused if they do not match it.

## Status and limits

Version 0.12. It works end to end with MSFS 2024 on Windows.

- A simulator is not an aircraft recorder. Some values have sampling limits and are documented as proxies, such as g read at 16 samples/s from frame-rate data and 1 Hz heading, reverser and brake.
- Thresholds without a published source are marked as estimates, and approximate profile values say how they were derived. The landing score is this project's reading of published tolerances, not an airline's or a regulator's assessment.
- Helicopter and glider landings are not assessed for a score.
- Recorded test flights are C172 flights. The challenge start's speed and pitch hold has only been flown in the C172.
- The challenge protocol is a design still in draft.
- For simulation only, not for real-world flight operations.

## Build from source

Needs Visual Studio Build Tools with the C++ workload, the [MSFS 2024 SDK](https://docs.flightsimulator.com/msfs2024/html/1_Introduction/SDK_Overview.htm) (its static `SimConnect.lib` is linked in; looked up in `MSFS2024_SDK`, else `C:\MSFS 2024 SDK`), Rust (stable, MSVC toolchain) and Node.js 22+.

```powershell
cd app
npm install
npm run tauri dev
```

Without the sim, `npm run dev` in `app/` serves the UI against the mock flights in `app/public/mock`. For the in-sim packages see [msfs-panel/](msfs-panel/README.md); for the installer, `npm run release -- <version>` in `app/` (see [app/README.md](app/README.md)).

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace
npm --prefix app run lint
npm --prefix app run build
```

Work goes to `dev`, then `dev` → `test` → `main`.

### CLI

```powershell
cargo run -p cli --bin fdm -- debrief <flight.fqz>         # flying skills (docs/skills.md)
cargo run -p cli --bin fdm -- debrief <flight.fqz> --json  # the app's Debrief JSON
cargo run -p cli --bin fdm -- analyze <flight> [--rules my.yaml]
cargo run -p cli --bin fdm -- export <flight> [--format parquet|csv|kml|gpx] [-o file]
cargo run -p cli --bin fdm -- logbook <dir>                # Little Navmap logbook CSV
cargo run -p cli --bin fdm -- compact <file|dir>           # .fqj -> .fqz
cargo run -p cli --bin fdm -- list <dir>                   # the app's flight list
cargo run -p cli --bin fdm -- trends <dir> [--aircraft ICAO] [--airport ICAO] [--days N] [--json]  # event rates, score and touchdown trends
cargo run -p cli --bin fdm -- challenge <challenge.json> <flight>  # a landing-challenge attempt
cargo run -p capture --bin foqa-rec                        # headless recorder
```

`fdm` also builds for Linux without the sim link (`fdm.Dockerfile`).

### Layout

```
crates/
  fdm-core    parameter list (LFL), units, signals, geo
  sim-link    SimConnect bindings (hand-written FFI, statically linked)
  recorder    acquisition, journal (.fqj) and compact (.fqz) formats
  analyzer    phases, derived VS, touchdowns, runway matching, live tracking
  rules       rule language, engine, aircraft profiles
  assess      landing assessment
  debrief     the analysis the app and CLI show, flight assessment
  challenge   landing challenges: definition, start on final, grading
  connect     optional challenge protocol client
  capture     live recording service, in-sim panel and EFB feed
  cli         fdm and foqa-airports
  testkit     synthetic flights for tests
app/          Tauri 2 + React desktop app
msfs-panel/   MSFS toolbar panel and EFB app packages
rules/        core rule set
profiles/     aircraft type limits
fixtures/     recorded test flights
docs/         formats, contracts and designs
```

Worth reading first: [format](docs/format.md), [rules](docs/rules.md), [phases](docs/phases.md), [debrief API](docs/debrief-api.md), [challenges](docs/challenges.md). Version notes are in [devlog/](devlog/).

## Licence

Free and open source under [Apache-2.0](LICENSE). Some UI components in `app/src/ui` are derived from Arc UI (MIT), see [app/src/ui/NOTICE.md](app/src/ui/NOTICE.md). Not affiliated with Microsoft, Asobo or any airline.
