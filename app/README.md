# FOQA-Sim app

The desktop app: Tauri 2 (Rust, `src-tauri/`) + React 19, Vite, Tailwind 4, uPlot and MapLibre.
It runs the recorder from the tray and shows the debrief of each flight.

```powershell
npm install
npm run tauri dev    # the real app, needs the MSFS 2024 SDK for SimConnect
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
