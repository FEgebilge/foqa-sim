# Landing challenges (design, draft)

A challenge is a JSON file: an airport, runway, aircraft class and
conditions that everyone flies the same way. FOQA-Sim opens the file, sets
the start, records the attempt and grades it locally; that works with no
site at all. A site is optional: through the
[FOQA Challenge Protocol](challenge-protocol.md) it can publish challenges,
receive attempts, re-score them and keep a leaderboard (the first one is
Pilot Aptitude). Status: the definition, the start and the grading are
implemented (`crates/challenge`, `fdm challenge`, the app's
`place_challenge`); the challenge screen, the marker and slice, and the
site client are not.

Research behind it (2026-10-27): SimConnect, MSFS 2024 SDK, DevSupport
threads, BeatMyLanding, OnAir and others; sources are linked inline.

## What MSFS 2024 allows

| Need | Mechanism | Reliability |
|---|---|---|
| Start on final, airborne | `SIMCONNECT_DATA_INITPOSITION` (lat, lon, alt, pitch, bank, heading, `OnGround = 0`, airspeed) on the user aircraft | Works; BeatMyLanding ships a teleport to the threshold. Complex add-ons may need their own configuration |
| Gear, flaps, AP off | Key events (`GEAR_DOWN`, `FLAPS_SET`, `AP_MASTER`) | Default aircraft yes; add-ons with L-vars vary |
| Weather | No documented SimConnect setter in 2024; the old weather API is deprecated; custom wind had bugs (SU4) | **Cannot be relied on** |
| Load a whole flight (`.FLT`) | `SimConnect_FlightLoad` | Reported to reset airborne starts to the ground ([DevSupport](https://devsupport.flightsimulator.com/t/loading-a-saved-flight-via-simconnect-is-not-working/11274)) |
| Time of day | `ZULU_*_SET` events | Changes live weather; measure instead |
| Aircraft | No call to load one | Verify only: `ATC MODEL`, weight, class |
| Native mission package | Only a temporary Mission Tool, XML-heavy | Feasible but fragile; later experiment |

Hence the principle: **set what can be set (position, configuration),
measure everything else, and accept an attempt only if the measured
conditions are inside the challenge's bands.** Fairness then becomes a
verified property of the attempt, not a hope about the user's settings.

## Challenge definition

A versioned JSON file, opened locally or served by a site; the app (and a
site, with `fdm challenge --check`) validates it with `Challenge::parse`
before offering it.

```json
{
  "id": "ltfj-05-gusty-2026w44",
  "version": 1,
  "title": "Sabiha Gökçen 06, gusty crosswind",
  "window": { "from": "2026-10-27T00:00Z", "to": "2026-11-03T00:00Z" },
  "airport": "LTFJ",
  "runway": "06",
  "aircraft": { "classes": ["transport"], "types": ["B38M", "A20N"] },
  "start": { "distance_nm": 8, "height_ft": 2500, "ias_kt": 160, "gear_down": true, "flaps": 1, "mode": "teleport" },
  "gate": { "distance_nm": 3 },
  "conditions": {
    "wind": { "from_deg": 330, "speed_kt": 18, "tolerance_deg": 20, "tolerance_kt": 4 },
    "crosswind_kt": { "min": 12, "max": 20 },
    "tailwind_kt": { "max": 0 },
    "preset": "ltfj-gusty.wpr"
  },
  "rules": { "ap_off_by_height_ft": 1000, "stop_on_runway": true },
  "scoring": { "version": "landing-1" }
}
```

Field reasons:

- **Airport and runway:** the touchdown is matched to the runway through the
  recorded airport snapshot (as in every debrief), so the touchdown zone and
  centreline values are the same for everyone.
- **Aircraft:** class and ICAO types, checked against `ATC MODEL` and the
  type profiles (`profiles/`). Liveries and variants are free.
- **Start:** where `INITPOSITION` puts the aircraft: on the extended
  centreline, `distance_nm` before the threshold, `height_ft` above it, gear
  and flaps (a fraction of full travel) set. `mode: self` lets the pilot fly
  there by any means.
- **Gate:** the last time before touchdown the aircraft is this far from the
  threshold. Grading starts here, so spawn settling never counts against
  the pilot; it must be closer than the start, and the aircraft airborne
  from the gate to touchdown.
- **Conditions:** targets with tolerances. Wind is measured over the attempt
  (`wind.speed`, `wind.dir`) and resolved on the runway, as `td.crosswind`
  and `td.tailwind` already are for the debrief rules. `preset` is a
  convenience: a weather preset shipped with the challenge for the pilot to
  pick.
- **Rules:** AP off below a height; slowed to taxi speed (30 kt) still on
  the runway. Sim rate 1, no pause, no slew and continuous data are always
  checked.

`Challenge::parse` rejects unknown fields, a gate not closer than the
start, flaps outside 0 to 1, a runway not written like `06` or `27L` and an
unknown scoring version.

## Attempt lifecycle

Local (always available):

1. The app lists challenges from opened files (and from connected sites),
   with a briefing and a **pre-flight card**: simulator connected, loaded
   aircraft allowed, current wind inside the bands, ranked or practice,
   each with a one-line fix (protocol B8).
2. **Start:** the app positions and configures the aircraft and writes an
   attempt marker into the recording; the attempt runs from the start to
   the stop on the runway. **Fly again** abandons it and starts over.
3. **Grade (local):** conditions, rules and score evaluated on the
   recording; the pilot sees at once whether it counts, in the app, the
   toolbar panel strip and the EFB tab.

With a site (optional, protocol A4–A5):

4. Before placing, the app asks the site to issue an attempt and puts its
   opaque `ref` in the marker. Offline or signed out, the attempt is
   practice: graded locally, never uploaded.
5. **Upload:** the attempt's recording slice (start to stop plus margins,
   the usual 1 to 10 Hz parameters, the airport snapshot, no tail number or
   aircraft title) and, for display only, the app's own grade. The site
   never ranks the client's score.
6. **Server:** recomputes conditions, rules and score from the slice with
   the same `fdm challenge`, runs plausibility checks, stores the attempt,
   ranks it.

## Integrity

An open-source client cannot be trusted: any key in it can be extracted and
telemetry can be fabricated. The design aims for verification and deterrence,
not prevention, which matches the stakes.

- **Re-scoring on the server** from raw telemetry; the client's number is
  display only.
- **Plausibility:** position continuity (no jumps beyond speed × dt),
  altitude vs vertical speed, normal g vs vertical speed change, groundspeed
  vs IAS and wind, sim time vs wall time.
- **Flags already recorded** with every flight (LFL v1): `sim.rate`
  (`SIMULATION RATE`), `sim.slew` (`IS SLEW ACTIVE`), `sim.paused` (pause events). Known
  holes: photo mode freezes without a pause event, and exiting to the menu
  sends no `SimStop` in 2024. Continuity checks cover both.
- **Issued attempt** per flight, owned by the account, with its own marker
  `ref`, a 30 min fly window and a 24 h upload window: no replaying an old
  or someone else's recording.
- **Tiers:** attempts passing every check are "verified"; only verified
  attempts rank; a report button and manual removal for the rest.

## Scoring

`landing-1`, 0 to 100, sum of smooth sub-scores (curves, not
bands), per aircraft class:

| Part | Weight | Full marks (transport / light) | Zero |
|---|---|---|---|
| Touchdown point | 25 | Within 8 % of the touchdown zone from the aiming point | Outside the zone |
| Sink rate | 25 | 100–250 fpm / 60–180 fpm; softer loses points too (float) | 600 / 500 fpm |
| Centreline | 15 | Within 1.5 m | 22.5 m |
| Stability from the gate | 10 | No rule event (low 0.7, medium 0.3) | A high event |
| Peak g | 10 | Up to 1.4 g / 1.6 g | 2.0 g / 2.5 g |
| Bounce | 10 | None | Bounced |
| Rollout | 5 | Within 3 m of the centreline | 22.5 m or off the runway |

Each part is piecewise linear between those points (`crates/challenge/src/score.rs`).

The formula is versioned (`scoring.version`): old attempts keep their score
when it changes. Since conditions are fixed per challenge, no difficulty
multiplier is needed inside one board. For comparison: BeatMyLanding is
penalty-based with a crosswind/tailwind multiplier capped at 1.45; OnAir
grades sink rate in bands (very smooth < 200 fpm … hard > 500 fpm).

## Site API (contract)

Superseded by the open, site-independent
[FOQA Challenge Protocol](challenge-protocol.md): discovery through
`/.well-known/foqa-challenges.json`, `foqa-sim://` deep links, sign-in per
site (browser loopback redirect with PKCE, device flow as fallback), the
attempt API, the recording slice and the app-side `connect` module.
Leaderboards are the site's business, not part of the protocol.

## Decisions (Ege, 2026-10-27)

1. **Start:** `teleport` by default, `self` as the fallback.
2. **Scoring:** `landing-1` above.
3. **Server analysis:** the Rust analysis on the home server:
   `fdm challenge <challenge.json> <flight.fqz>` prints the `Attempt`
   (checks, validity, score) as JSON, the same `challenge::evaluate` the app
   runs. It takes a compact flight only, whose decompressed body is capped
   (256 MiB); a journal or a raw capture is refused by its extension, since
   a raw capture's gzip lets a small upload expand. The raw replay is capped
   too (1 GiB decompressed, 64 KiB a line) for local use.
4. **Accounts:** site accounts; the app signs in through the browser
   (loopback redirect with PKCE), the device flow as fallback.
5. **Offline:** attempts without a site-issued attempt stay local practice;
   the upload window is 24 h.

## Placing a start (implemented)

The recorder service carries out the app's simulator requests on its own
SimConnect connection (`crates/capture/src/place.rs`). Take the handle
before `Service::run`; it is `Clone + Send`:

```rust
let sim = service.sim_requests();                    // capture::SimRequests
let placed = sim.place(Start { pos, gear_down: true, flaps: Some(0.875) });
let airport = sim.airport("LTFJ");                   // fdm_core::Airport with runways
placed.recv_timeout(Duration::from_secs(5))??;       // Result<(), String>
```

- `place` sets `InitPosition` (lat, lon, altitude ft MSL, pitch, bank, true
  heading, `on_ground`, airspeed kt; `sim_link::InitPosition`) on the user
  aircraft, then sends `AUTOPILOT_OFF`, `GEAR_DOWN`/`GEAR_UP` and, if
  `flaps` is set, `FLAPS_SET` (`flaps_set_data`: handle fraction × 16383,
  the sim snaps to the closest detent). It runs at the next sim frame; `Ok`
  means the calls went out, a rejection by the sim is only logged.
- `airport` answers from the facility database (thresholds:
  `analyzer::runway::thresholds`); it is also recorded like the airports
  captured for landings.
- While the sim is not connected both answer `Err` at once; if the sim
  quits first, the receiver reports disconnection.

Not yet verified against a live sim: the placement itself, `Pitch` sign
(SimConnect's usual positive nose-down assumed), whether key events sent
right after the teleport stick, and add-on aircraft that ignore key events.
