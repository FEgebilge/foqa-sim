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
- `src/ui/` is vendored from egebilge/ui. Edit it here if needed and keep the manifest in sync
  (`npm run ui:check` expects the library checked out next to this repo).

## Installer and start with Windows

- `npm run tauri build` writes `target/release/bundle/nsis/FOQA-Sim_<version>_x64-setup.exe`:
  per-user install, unsigned.
- Every cargo build of the app crate (`src-tauri/build.rs`, also `cargo test`/`clippy` and a fresh clone) runs
  `msfs-panel/assemble.ps1` when its inputs are newer than its output (no MSFS needed): it assembles the panel and EFB
  Community packages from `msfs-panel/prebuilt/InGamePanel_FOQASim.spb` + sources, `package_version` = app version,
  which the bundle ships as resources `packages/foqa-sim-panel` and `packages/foqa-sim-efb` (installed at
  `<install dir>\packages\...`, i.e. Tauri `resource_dir()/packages/...`). After changing the panel XML, run
  `msfs-panel/build.ps1` (MSFS closed) to recompile and refresh `prebuilt/`, then commit it. The app copies the
  packages into Community.
- `src-tauri/windows/hooks.nsh`, skipped for updates (`/UPDATE`): before install, warns if the VC++ 2015-2022 x64
  runtime is missing (offers the download page) or MSFS 2024 is not found (Store/Steam `UserCfg.opt`); after a
  completed uninstall removes the Community folders listed in `%APPDATA%\com.egebilge.foqa-sim\community-packages.txt`
  (written by the app, see [installation](../docs/debrief-api.md#installation); only folders named
  `foqa-sim-panel`/`foqa-sim-efb` whose `manifest.json` names `FOQA-Sim` as the manufacturer). So uninstalling and
  then installing by hand removes the packages until the app installs them again: its Installation screen shows them
  missing, with Install. Re-running the setup repairs the app's files.
- Start with Windows is off by default; the power button in the header toggles it (a login launch starts in the tray).
- CSP (`tauri.conf.json`): self, plus `https://tiles.openfreemap.org` for the map (connect/img), blob workers
  for MapLibre, Tauri IPC. A new remote host needs adding there.
