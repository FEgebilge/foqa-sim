# FOQA-Sim app

The desktop app: Tauri 2 (Rust, `src-tauri/`) + React 19, Vite, Tailwind 4, uPlot and MapLibre.
It runs the recorder from the tray and shows the debrief of each flight.

```powershell
npm install
npm run tauri dev    # the real app; building needs the MSFS 2024 SDK (static SimConnect)
npm run dev          # UI only, in the browser, against public/mock
npm run build
npm run lint
```

- Backend ↔ UI contract: [docs/debrief-api.md](../docs/debrief-api.md). `src/api.ts` falls back
  to the mocks when it isn't running inside Tauri.
- Regenerate a mock from a real flight:
  `cargo run -p cli --bin fdm -- debrief <flight.fqz> --json > public/mock/debrief-<id>.json`
- `src/ui/` is vendored from egebilge/ui and is part of this repo: nothing else is needed to build. Edit it here if
  needed; `npm run ui:check` (maintainers) compares it with the library when that is checked out next to this repo
  or `EGEBILGE_UI` points to it, and skips otherwise.

## Devlog

Release notes are plain Markdown, one file per version: `../devlog/<version>.md` (frontmatter `version`, `date`, `title`;
format in `../devlog/README.md`). Vite bundles them at build time (`src/lib/devlog.ts`): after an update the app shows a
"What's new" dialog with the versions since the last one seen (kept in `localStorage`; a first install only stores the
version), and the header's newspaper button opens the full list. `npm run release -- <version>` refuses to run without a
matching `devlog/<version>.md`, whose body is the release notes (in `latest.json` and on GitHub).

## Installer and start with Windows

- `npm run tauri build` writes `target/release/bundle/nsis/FOQA-Sim_<version>_x64-setup.exe`:
  per-user install, unsigned.
- Updates: `tauri-plugin-updater` reads `releases/latest/download/latest.json` (`plugins.updater` in `tauri.conf.json`;
  the `pubkey` there must be the content of `~\.tauri\foqa-sim.key.pub`). The app checks once at start (at most once a
  day) and from "Check for updates" on the What's new screen; a corner notice offers "Install and restart" (refused
  while a flight records). Installs are passive and the installer relaunches the app. Details: `../docs/releasing.md`.
- Releasing: `npm run release -- <version> --publish` (in `app/`). It needs `gh auth login`, the updater key
  (`~\.tauri\foqa-sim.key`, or `FOQA_UPDATER_KEY`) and asks for the key's password (hidden; or set
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). It builds and signs, writes `latest.json`, copies `FOQA-Sim_x64-setup.exe`,
  prints the SHA-256 and creates a **draft** GitHub release. Check the draft's assets, then publish it on GitHub.
  Without `--publish` it stops after the build and prints the `gh release create --draft` command.
- A new version: `npm run release -- 0.1.1` sets it in `tauri.conf.json`, `package.json`, `package-lock.json` and the
  workspace `Cargo.toml` (and `Cargo.lock`), then builds that installer; it refuses a version that is not newer.
  Run over an older install, the setup offers to uninstall it first (recommended); flights in
  `Documents\FOQA-Simflights` and the settings stay (unless "Delete app data" is ticked), and the new app puts the
  in-sim packages back on its first start (see [installation](../docs/debrief-api.md#installation)).
- Every cargo build of the app crate (`src-tauri/build.rs`, also `cargo test`/`clippy` and a fresh clone) runs
  `msfs-panel/assemble.ps1` when its inputs are newer than its output (no MSFS needed): it assembles the panel and EFB
  Community packages from `msfs-panel/prebuilt/InGamePanel_FOQASim.spb` + sources, `package_version` = app version,
  which the bundle ships as resources `packages/foqa-sim-panel` and `packages/foqa-sim-efb` (installed at
  `<install dir>\packages\...`, i.e. Tauri `resource_dir()/packages/...`). After changing the panel XML, run
  `msfs-panel/build.ps1` (MSFS closed) to recompile and refresh `prebuilt/`, then commit it. The app copies the
  packages into Community.
- The same build script copies the VC++ runtime DLLs the exe needs (`msvcp140.dll`, `vcruntime140.dll`,
  `vcruntime140_1.dll`) from the Visual Studio Build Tools redist folder into `src-tauri/vcrt` (not in git); the
  bundle installs them next to the exe (app-local deployment), so users need no VC++ redistributable.
- `src-tauri/windows/hooks.nsh`, skipped for updates (`/UPDATE`): before install, warns if MSFS 2024 is not found (Store/Steam `UserCfg.opt`); after a
  completed uninstall removes the Community folders listed in `%APPDATA%\com.egebilge.foqa-sim\community-packages.txt`
  (written by the app, see [installation](../docs/debrief-api.md#installation); only folders named
  `foqa-sim-panel`/`foqa-sim-efb` whose `manifest.json` names `FOQA-Sim` as the manufacturer). So uninstalling and
  then installing by hand removes the packages until the app installs them again: its Installation screen shows them
  missing, with Install. Re-running the setup repairs the app's files.
- Start with Windows is off by default; the power button in the header toggles it (a login launch starts in the tray).
- CSP (`tauri.conf.json`): self, plus `https://tiles.openfreemap.org` for the map (connect/img), blob workers
  for MapLibre, Tauri IPC. A new remote host needs adding there.
