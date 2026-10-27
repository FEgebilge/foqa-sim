# Releasing FOQA-Sim: publish, self-update, devlog

Goal: people download the installer from GitHub, the app updates itself, and the existing devlog ("What's new",
`devlog/<version>.md`) shows after an update. Research date 2026-10-27. Items marked **(unverified)** are not
confirmed by a source.

## TL;DR

- **Publish locally, one command** (`release.mjs` builds, signs the updater artifact, `gh release create`). No CI.
- **Self-update** with `tauri-plugin-updater` against `releases/latest/download/latest.json`. Needs a public repo
  (anonymous download), so wire it in at going public, and ship it in the first public release.
- **CI**: not needed now. The MSFS SDK EULA does not clearly allow or forbid it (section 4); the clean way to make CI
  possible later is to load `SimConnect.dll` at runtime instead of linking `SimConnect.lib`.
- **Signing**: nothing free fits today (section 5). Ship unsigned, document SmartScreen, publish SHA-256.

---

## 1. Updater mechanics (sources: Tauri updater docs, Tauri NSIS template, plugin source)

Sources: <https://v2.tauri.app/plugin/updater/>,
<https://raw.githubusercontent.com/tauri-apps/tauri/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi>,
<https://raw.githubusercontent.com/tauri-apps/plugins-workspace/v2/plugins/updater/src/updater.rs>.

**Keys.** `npm run tauri signer generate -- -w ~/.tauri/foqa-sim.key` (in `app/`). Gives a private key (+ password)
and a public key string. Losing the private key means existing installs can never update again: back it up in the
password manager, never commit it. The pubkey in config is the key *content*, not a path.

**Config** (`tauri.conf.json`), added at going public:

```jsonc
"bundle": { "createUpdaterArtifacts": true, ... },          // "will be removed in v3" per docs; fine for v2
"plugins": { "updater": {
  "pubkey": "<content of foqa-sim.key.pub>",
  "endpoints": ["https://github.com/FEgebilge/foqa-sim/releases/latest/download/latest.json"],
  "windows": { "installMode": "passive" }                   // default; shows a small progress window, no input
} }
```

`installMode`: `passive` (recommended, default), `basicUi` (needs clicks), `quiet` (no feedback; works without admin
only for user-wide installs, which ours is: `installMode: currentUser`). Keep `passive`.

**Build env** (PowerShell; `.env` files do NOT work): `$env:TAURI_SIGNING_PRIVATE_KEY = "<path or content>"`,
`$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "..."`. With `createUpdaterArtifacts: true` the build writes
`FOQA-Sim_<ver>_x64-setup.exe` and `FOQA-Sim_<ver>_x64-setup.exe.sig` in `target/release/bundle/nsis/`.
A build without the key fails once `createUpdaterArtifacts` is on, so `release.mjs` must check the env first.

**latest.json** (static; Tauri validates the whole file, so every platform entry must be complete):

```json
{
  "version": "0.2.0",
  "notes": "<body of devlog/0.2.0.md>",
  "pub_date": "2026-11-01T12:00:00Z",
  "platforms": { "windows-x86_64": {
    "signature": "<CONTENT of the .sig file>",
    "url": "https://github.com/FEgebilge/foqa-sim/releases/download/v0.2.0/FOQA-Sim_0.2.0_x64-setup.exe"
  } }
}
```

`version` is SemVer, `v` prefix optional. `tauri-action` can generate it, but `release.mjs` can write it in ~10 lines.

**What the NSIS update does.** The plugin runs the downloaded installer via `ShellExecuteW` with `/P` (passive mode
args) and `/UPDATE`, then calls `std::process::exit(0)` itself (docs: the app exits automatically on Windows; an
`on_before_exit` hook runs first). In the Tauri template `/UPDATE` sets `$UpdateMode = 1`, and
`PageLeaveReinstall` then jumps straight to `reinst_done`: **the old uninstaller is not run** (only a WiX-migration
would force it). So `hooks.nsh` PREUNINSTALL/POSTUNINSTALL never fire during an update, and every one of our four
hook blocks is additionally guarded by `${If} $UpdateMode <> 1`, so the MSFS prompt and the Community-package
removal are skipped either way. Community packages therefore stay; `setup.rs::refresh()` (called from `main.rs` at
startup) then installs a newer bundled package if the installed one is older and the sim is not running; otherwise
the status screen offers it. Manual reinstall (double-click the new setup.exe) is the other path: the old
uninstaller runs without `/UPDATE`, removes the packages, and `refresh()` restores them (the case its doc comment
describes). Both are covered; test both once (section 7).

**Restart after update.** The plugin appends restart args (`/R`, `/ARGS`) only if its `restart_after_install`
context is set; I could not confirm from the sources read whether JS `downloadAndInstall()` sets it **(unverified)**.
Test it on the first real update. If the app does not relaunch (the process has already exited, so `relaunch()`
cannot help), pass `/R` via the plugin's installer args or start the app from `NSIS_HOOK_POSTINSTALL` when
`$UpdateMode = 1` (the template only auto-runs with `/R` in passive/silent mode). Single-instance plugin is
already used, so a double start is harmless.

**App side** (small): add `tauri-plugin-updater` (Rust) and `@tauri-apps/plugin-updater`; capability
`updater:default`; check on start (after the tray is up, once per day) and from a "Check for updates" button;
show `update.body` (the notes) plus "Install & restart"; never auto-install while a flight is recording
(`sim-link` state) or the sim is running. After the update the existing devlog dialog shows the new files (it
bundles all `devlog/*.md` at build time). Network calls come from Rust, so CSP `connect-src` is unaffected.

**Private-repo limitation.** `releases/latest/download/...` is a redirect to a release asset and returns 404 to
anonymous clients while the repo is private; the updater cannot send a token (Tauri discussion #7553; the only
workarounds are a proxy server). Hence: enable only when public; do not build a proxy.

**Channels.** `releases/latest` ignores drafts and prereleases, so a `--prerelease` release is invisible to
normal users: use it for tester builds that are downloaded by hand. A real beta auto-update channel would need a
second endpoint at a stable URL (e.g. a moving `beta` tag whose `latest.json` is replaced each time) and a setting
in the app that switches `updater_builder().endpoints(...)`. Not worth it until there are testers.

## 2. Publishing (`release.mjs` extension)

Today it bumps versions and runs `npm run tauri build`, and already requires `devlog/<ver>.md`. Add, after the build:

1. Fail early if `TAURI_SIGNING_PRIVATE_KEY` is unset (once the updater is on) or `gh auth status` fails.
2. Copy the installer to `FOQA-Sim_x64-setup.exe` in the same dir (stable download name).
3. Write `latest.json` there from the devlog body and the `.sig` contents (format above).
4. Create a **draft** release, check it, then publish:

```
gh release create v0.2.0 --draft --title "FOQA-Sim 0.2.0" --notes-file devlog/0.2.0.md `
  target\release\bundle\nsis\FOQA-Sim_0.2.0_x64-setup.exe `
  target\release\bundle\nsis\FOQA-Sim_0.2.0_x64-setup.exe.sig `
  target\release\bundle\nsis\FOQA-Sim_x64-setup.exe `
  target\release\bundle\nsis\latest.json
gh release edit v0.2.0 --draft=false      # only now does /latest/ (and so the updater) see it
```

Notes: strip the YAML front matter from the devlog for `--notes-file` (write a temp file). `gh release create`
creates the tag from the default branch unless `--target`/an existing tag is given: tag the release commit first
(`git tag v0.2.0 && git push origin v0.2.0`, owner does the commit/push per his git rules). Tester build: add
`--prerelease` (never becomes `latest`). README link: `https://github.com/FEgebilge/foqa-sim/releases/latest/download/FOQA-Sim_x64-setup.exe`
("Download for Windows"). Add SHA-256 of the installer to the release notes (`Get-FileHash`).

## 3. Signing for SmartScreen (summary; details in section 5)

An unsigned installer shows SmartScreen "Windows protected your PC" until reputation builds; signing is the only
real fix. No free route fits right now.

## 4. CI: can we, and how others do it

### What the licence says

I read the actual text installed with SDK 1.8.1 (`C:\MSFS 2024 SDK\Licenses\MSFS SDK EULA.pdf`, dated 11/2019,
"Microsoft Software License Terms"):

- 1(a): "You may install and use the Software solely for the Purpose stated above." Purpose: "to develop MSFS
  Add-Ons".
- 2(e): you will not "share, publish, distribute, or lend the Software (except for any distributable code, subject
  to the terms above), or provide the Software as a stand-alone hosted solution for others to use, or transfer the
  Software or this agreement to any third party".
- The EULA never defines or lists "distributable code" (nothing "above" does), and has no clause on CI, build
  servers or copies. Two threads on the MSFS dev forum ask whether the 2024 `SimConnect.dll`/`SimConnect.msi` are
  distributable; neither has an answer from Asobo/Microsoft as of this research
  (<https://devsupport.flightsimulator.com/t/redistribution-rights-for-msfs-2024-native-simconnect-dll/18233>,
  <https://devsupport.flightsimulator.com/t/redistribution-permission-for-msfs-2024-simconnect-dlls/18295>;
  the fetch of the page bodies failed on a TLS error, so this rests on the search summaries).
- SDK docs (<https://docs.flightsimulator.com/msfs2024/html/1_Introduction/SDK_Contents.htm>) point to the licence
  folder only; nothing on redistribution.

Conclusions (my reading, not legal advice):

| Action | Verdict |
|---|---|
| Commit SDK files (`SimConnect.lib/.dll/.h`, the SDK) to the public repo | Publishing the Software: **do not**. |
| Use the SDK on your own machine, build, publish the exe | Clearly the intended use. |
| Upload an SDK zip to a private/secret location and download it in your own GitHub-hosted CI job | **Grey**: not distribution to third parties, but a copy on a third-party host and undefined by the EULA. Common practice (below). |
| The released exe contains `SimConnect.lib` code (our static link) | **Same grey area as shipping the DLL**: the "distributable code" question applies to it too. Already the case for every build today. |
| Ship `SimConnect.dll` next to the exe | Unclear until Microsoft confirms or the DLL is found on user PCs. |
| Load `SimConnect.dll` from the user's own MSFS/SDK install at runtime | **No redistribution at all.** Cleanest. |

### How others do it

- **FlyByWire** (aircraft, A32NX/A380X; WASM/Rust): CI runs on `ubuntu-latest` and executes everything inside a
  public Docker image, `ghcr.io/flybywiresim/dev-env` (pinned by digest in `scripts/dev-env/run.sh`); the CMake
  toolchain sets `MSFS_SDK "/workdir/MSFS_SDK"`, i.e. the WASM SDK parts live in the image. `msfs-rs`
  (`.github/workflows/pr.yaml`) runs `docker run ghcr.io/flybywiresim/dev-env:latest scripts/ci.sh`. Their image
  source repo is not public as far as I could find, so what exactly is inside is **unverified**. The SDK pieces
  they need are the WASM sysroot/headers, not SimConnect.lib. Public, and tolerated for years, but not an
  authorization.
- **koesie10/setup-msfs-sdk** (GitHub Action, used by community add-ons): downloads "a ZIP file" from a URL given as
  a **secret** (`url: ${{ secrets.MSFS_SDK_URL }}`) and sets `MSFS_SDK`. That is the "private copy" pattern.
- **MobiFlight Connector** (C#, public): `ci.yml` on `windows-latest` has no SDK step; the repo commits a
  `lib/SimConnect.dll` (managed client assembly, I did not inspect it) and builds from that.
- **Little Navmap / atools** (Qt): SimConnect is optional, env `ATOOLS_SIMCONNECT_PATH` pointing to a local SDK; I
  found no workflow files for littlenavmap, releases appear to be built locally by the author.
- **SkyDolly** (C++): `cmake/FindSimConnect.cmake` takes `MSFS_SDK` and imports `SimConnect.dll` as a **SHARED**
  (dynamic) library, SDK never in the repo.
- Many C# add-ons reference `Microsoft.FlightSimulator.SimConnect.dll` from their own SDK install. I found no
  official NuGet package **(unverified)**. SPAD.neXt and FSUIPC7 are closed source, nothing to read.

Pattern: nobody puts the SDK in the repo; open-source MSFS projects either build locally, pull a private copy in
CI, or (FlyByWire) bake it into a public image.

### Runtime loading instead of static linking (the real fix)

`sim-link` is already hand-written FFI and the repo contains no SDK header, only `build.rs` linking
`SimConnect.lib`. Replace that with `LoadLibraryW` + `GetProcAddress` (or the `libloading` crate) for the ~15
SimConnect functions used, behind the existing `sim-link` types. Then:

- CI needs **nothing** from the SDK: a plain `windows-latest` job can build, test and publish.
- No SimConnect code is in our exe, so the EULA question disappears for the exe.
- The `/MD` workaround for `SimConnect.lib` goes away: `-C target-feature=+crt-static` can replace the bundled
  `msvcp140/vcruntime140*.dll` resources (`tauri.conf.json`), the `.cargo/config.toml` `/IGNORE:4099` and the
  `build.rs` VC++ copy logic.
- Cost: the DLL must exist on the player's PC. **Unverified:** whether MSFS 2024 (Store/Steam) installs a native
  `SimConnect.dll` outside the SDK (a third-party guide says SimConnect comes with the sim, not official:
  <https://flyawaysimulation.com/ask/answers/install-simconnect-msfs-fsx-prepar3d/>). Check on the owner's PC first
  (search the MSFS install dir and `%LOCALAPPDATA%\Packages\Microsoft.Limitless_*` for `SimConnect.dll`). If it is
  there: search order = beside the exe, then the sim's folder, else a clear error. If it is not there, only
  players with the SDK would work, which is unacceptable: then keep static linking (status quo) and ask Asobo on
  the dev forum for a written answer before relying on CI.

### Self-hosted runner (only if CI is wanted while the DLL stays linked)

GitHub's own guidance: self-hosted runners "should almost never be used for public repositories" because anyone can
open PRs; they are not ephemeral; environment approvals do not isolate the runner
(<https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions>).
If done anyway, all of:

1. Workflow trigger **only** `on: push: tags: ['v*']` (no `pull_request`, `pull_request_target`, `workflow_run`,
   `issue_comment`). Repo settings: Actions > "Require approval for all outside collaborators" for fork PRs.
2. Job guard `if: github.actor == 'FEgebilge' && github.repository == 'FEgebilge/foqa-sim'`; protect tags
   (`v*` ruleset: only the owner can create), restrict who has write access (nobody else).
3. `environment: release` with required reviewer (you) holding the signing key/password as environment secrets
   (not repo secrets); runner group limited to this one repo.
4. Runner as a separate low-privilege Windows user (not admin, no access to your profile, browser, `.ssh`, vault),
   `--ephemeral` or a clean workspace per run, outbound network only what is needed; no long-lived tokens on disk
   (`GITHUB_TOKEN` with `contents: write` only in the release job, `permissions: read` elsewhere).
5. Pin third-party actions by SHA; the workflow file itself changes only by your own pushes.
6. Machine must be on when tagging (owner PC): defeats part of the point of CI.

### Recommendation

1. **Now and at going public: keep local publishing** (section 2). Same machine, same SDK, one command, no secrets
   leave the PC, no EULA exposure beyond today's. CI adds nothing the owner needs yet except provenance.
2. **Medium term, if the DLL is on player PCs: runtime-load SimConnect**, drop the VC++ DLL bundling, then add a
   plain GitHub-hosted tag workflow (`windows-latest`, `npm ci`, `npm run release`-equivalent, signing via a
   cloud signer, `gh release create`). This is also what SignPath requires (section 5).
3. **Never**: SDK files in the repo, or a self-hosted runner reachable by PR-triggered workflows.

## 5. Code signing

- **SignPath Foundation** (free for OSS; <https://signpath.org/terms>): OSI-approved licence with no commercial
  dual licensing (we are Apache-2.0: ok); "No proprietary code" except system libraries (a statically linked
  `SimConnect.lib` is proprietary: likely disqualifying while linked; fine after runtime loading); project active,
  already released, documented download page; MFA for the team; a "Code signing policy" section in the README with
  attribution and privacy policy; roles (author/reviewer/approver) and **manual approval of every release**;
  **each signed release must be an automated build from the source repo that SignPath verifies** (so local builds
  cannot be signed); for executables "a certain verifiable reputation" (no numeric threshold). Acceptance at their
  discretion. Realistic only after: public repo, some history, CI release workflow, runtime-loaded SimConnect.
- **Azure Artifact Signing** (formerly Trusted Signing; <https://azure.microsoft.com/en-us/products/artifact-signing>,
  <https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options>): Basic $9.99/month (5,000
  signatures), Premium $99.99/month. Public-trust identity validation: individuals **USA and Canada only**;
  organizations USA, Canada, EU, UK. **Türkiye is not eligible as an individual**, and a Q&A post reports individual
  onboarding paused (unconfirmed). Not an option.
- Paid OV/EV certificates from a CA: not researched; EV gives instant SmartScreen trust, OV does not.
- Tauri hook (from Tauri docs, not re-fetched; verify): `bundle.windows.signCommand` = a command line with `%1` for the
  file (e.g. `signtool sign ... %1` or the Artifact Signing CLI); SignPath works differently (CI uploads the
  artifact, SignPath returns it signed), so it needs a workflow, not `signCommand`.
- Interim: ship unsigned, say in the README that SmartScreen may warn ("More info > Run anyway"), publish SHA-256 in
  each release, keep the repo public and the build reproducible. Reputation accrues per file hash/publisher.

## 6. Owner step list

### Now (repo private)

1. Verify one thing on your PC: does the MSFS 2024 install have its own `SimConnect.dll` (section 4)? Decides
   whether runtime loading is viable.
2. Optionally post on the MSFS dev forum asking Asobo to confirm SimConnect.dll redistribution (the two existing
   threads are unanswered).
3. Generate the updater key: `cd app; npm run tauri signer generate -- -w $HOME\.tauri\foqa-sim.key`; store key,
   password and pubkey in the password manager (and a second offline copy).
4. Wire the app side in code but leave `plugins.updater` out of `tauri.conf.json` (no pubkey, no endpoint, no
   `createUpdaterArtifacts`): plugin, capability, "Check for updates" UI, notes display. Keep the check disabled when
   the config block is absent.
5. Extend `release.mjs` per section 2 (stable-name copy, `latest.json`, `gh release create --draft`); dry-run it
   privately: private repo release works for manual downloads by collaborators.
6. Write `devlog/<ver>.md` for each release (already enforced).

### At going public

1. Add to `tauri.conf.json`: `createUpdaterArtifacts: true` and the `plugins.updater` block (pubkey, endpoint,
   `passive`). Set `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)` in the shell.
2. **First public version must already contain the updater** (a build without it cannot self-update); bump to e.g.
   0.2.0 and tell private testers to reinstall manually once.
3. Branch/commit/tag yourself (git rules), then `npm run release -- 0.2.0`; check the draft on GitHub: assets are
   installer, `.sig`, stable-name setup, `latest.json`; publish.
4. README: Download link to `releases/latest/download/FOQA-Sim_x64-setup.exe`, SmartScreen note, SHA-256.
5. Do not add `pull_request` CI that needs the SDK; leave `.github/` empty or CI without secrets (lint/tests only,
   on GitHub-hosted runners, per the Git rules CI only on test/main).

## 7. Test plan before the first public release

1. Install 0.2.0 (public build), publish 0.2.1 draft-then-live, confirm the app finds it, shows notes, installs
   passively, and **relaunches**; confirm Community packages are present and `refresh()` upgraded them
   (sim off), plus the "sim running" path (status screen offers the update).
2. Update while recording or with the sim running: the app must refuse to install, not corrupt a flight.
3. Manual path: run the new setup.exe over the old one; packages removed then restored by `refresh()`.
4. Fresh uninstall still removes the packages (hooks unchanged, not in update mode).
5. Wrong signature / tampered `latest.json`: update must be rejected (Tauri verifies the minisign signature).

## Checked: no public SimConnect.dll for players (2026-10-27)

On a Microsoft Store install of MSFS 2024 the game folder
(`C:\XboxGames\Microsoft Flight Simulator 2024\Content`) holds only
`SimConnect_internal.dll` (85 KB), the sim's own internal library, not the
public SimConnect client API. Players without the SDK therefore have no
`SimConnect.dll` to load at runtime: the app has to carry SimConnect itself,
as it does today by linking the static `SimConnect.lib` (MobiFlight and
Little Navmap ship the DLL instead). Loading the DLL from the game install
is not an option, and builds keep needing the SDK. Decision: releases are
built and published locally (`npm run release`) until that changes.
