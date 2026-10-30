# Landing challenges (design, draft)

A challenge is a JSON file: an airport, runway, aircraft class and
conditions that everyone flies the same way. FOQA-Sim opens the file, sets
the start, records the attempt and grades it locally; that works with no
site at all. A site is optional: through the
[FOQA Challenge Protocol](challenge-protocol.md) it can publish challenges,
receive attempts, re-score them and keep a leaderboard (the first one is
Pilot Aptitude). Status: the definition, the start, the grading, the
attempt marker and the recording slice are implemented
(`crates/challenge`, `fdm challenge`, `fdm slice`, the app's Challenges
screen) and the site client (`crates/connect`); deep links are not.

Research behind it (2026-10-27): SimConnect, MSFS 2024 SDK, DevSupport
threads, BeatMyLanding, OnAir and others; sources are linked inline.

## What MSFS 2024 allows

| Need | Mechanism | Reliability |
|---|---|---|
| Start on final, airborne | `SIMCONNECT_DATA_INITPOSITION` (lat, lon, alt, pitch, bank, heading, `OnGround = 0`, airspeed) on the user aircraft | Works; BeatMyLanding ships a teleport to the threshold. Complex add-ons may need their own configuration |
| Gear, flaps, AP off | Key events (`GEAR_DOWN`, `FLAPS_SET`, `AP_MASTER`) | Default aircraft yes; add-ons with L-vars vary |
| NAV1 on the runway's ILS | Facility database (runway `*_ILS_ICAO`, then the VOR's `FREQUENCY`), key event `NAV1_RADIO_SET_HZ` | Not yet verified live; add-on radios may ignore it |
| Weather | No documented SimConnect setter in 2024; the old weather API is deprecated; custom wind had bugs (SU4) | **Cannot be relied on** |
| Load a whole flight (`.FLT`) | `SimConnect_FlightLoad` | Reported to reset airborne starts to the ground ([DevSupport](https://devsupport.flightsimulator.com/t/loading-a-saved-flight-via-simconnect-is-not-working/11274)) |
| Time of day | `ZULU_*_SET` events | Changes live weather; measure instead |
| Aircraft | No call to load one | Verify only: `ATC MODEL`, weight, class |
| Native mission package | Only a temporary Mission Tool, XML-heavy | Feasible but fragile; later experiment |

Hence the principle: **set what can be set (position, configuration),
measure everything else, and accept an attempt only if the measured
conditions are inside the challenge's bands.** Fairness then becomes a
verified property of the attempt, not a hope about the user's settings.

## Setting the weather (experiment, 2026-10-27)

Ranked attempts must start with the same weather every time (Ege). The sim's
own weather service is reachable from any Coherent page that has
`RegisterViewListenerT` (our EFB app's page does), as the sim's weather
toolbar does it (`fs-base-ui/html_ui/JS/Services/Weather.js`):

```js
const l = RegisterViewListenerT("JS_LISTENER_WEATHER", ready, ViewListener.ViewListener);
l.on("SetWeatherList", list => ...);           // presets: index, name, wind summary
l.on("UpdatePreset", preset => ...);           // the selected preset, full data
l.trigger("ASK_WEATHER_LIST");
l.trigger("CUSTOM_WEATHER_PRESET_CHANGED", 1); // select by index
l.call("UpdateTempWeatherPreset", preset);     // edited copy, resolves with the result
```

Verified on the running sim (MS Store build, paused before Ready to fly):
the listener answers with 11 presets (live, clear sky, ..., last custom);
selecting index 1 pushes the full preset (`tWindLayers` with `dvAltitude`,
`dvAngleRad`, `dvSpeed`, `gustWaveData`; up to 3 `tCloudLayers`;
`oSettings` with `dvMSLPressure`, `dvMSLGLTemperature`, `dvPrecipitation`,
`dvPollution`, ...); editing wind layer 0 to 270 deg / 18 kt with no gust
and calling `UpdateTempWeatherPreset` is accepted and echoed back exactly.
Values carry the user's display units (`inHg`, `°F` here): convert from each
value's `unit`, never assume.

Not yet verified: that the aircraft then flies in that wind. While frozen
before Ready to fly the ambient wind SimVars kept the previous weather.
Next: in flight, select a preset, set wind layer 0, then compare
`AMBIENT WIND DIRECTION` / `VELOCITY`, `SEA LEVEL PRESSURE` and temperature
with the challenge before the attempt starts. Gusts stay off for ranked
challenges: their phase is random, so identical gust settings do not give
identical wind.

### Implementation status

Built, verified in flight: pending. `FoqaWeather` (`msfs-panel/efb/foqa-weather.js`)
selects the base preset ("clear" is found by the name containing
`CLEAR_SKY`), edits a copy (wind layers, QNH, temperature, converted from
each field's `unit`; unknown unit fails), calls `UpdateTempWeatherPreset`,
reads back the echoed preset and, after 4 s, the four SimVars, and reports
`{ ok, applied, measured, mismatches }`. The app asks over the Comm Bus
(`set_weather`, [ingame-panel.md](ingame-panel.md#weather-efb-app)); no
challenge flow uses it yet. Assumptions to check in flight: that
`gustWaveData.dvSpeedMultiplier = 1` means no gust, that `dvAngleRad` is the
direction the wind blows from, and that the SimVars follow within 4 s.
Visibility has no direct preset field and is not set; clouds stay the
base preset's.

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
unknown scoring version. A `landing-3` (or `landing-2`) challenge must list
`aircraft.types`, each with a Vref source in its profile (Vref table, POH
approach speeds or VSO): the pilot's selected speed never counts for a
ranked attempt ([scoring.md](scoring.md) §5, Decision 1).

## Attempt lifecycle

Local (always available):

1. The app lists challenges from opened files (and from connected sites),
   with a briefing and a **pre-flight card**: simulator connected, loaded
   aircraft allowed, current wind inside the bands, ranked or practice,
   each with a one-line fix (protocol B8).
2. **Start:** the app positions and configures the aircraft and writes an
   attempt marker into the recording; the attempt runs from the start to
   the stop on the runway. **Fly again** abandons it and starts over
   with a pristine aircraft ("Fly again: a clean start" below).
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

`scoring.version` names the method, and the server re-scores an upload
with the one its challenge names, so app updates never move a board.

**`landing-2`** (0.10) is retired: a challenge naming it is graded with
landing-3, and its attempts say `landing-3` ([scoring.md](scoring.md) §6).

**Every attempt is graded with the debrief's current method**
(`challenge::CURRENT`, today `landing-3`; decided 2026-10-30), whatever
the definition's `scoring.version` names: a challenge and the debrief
never grade one landing two ways. `scoring.version` must still be a known
name (`landing-1`, `landing-2`, `landing-3`; an unknown one needs a newer
app), and the attempt's `score.version` says which method graded it. A
definition naming `landing-2` or `landing-3` must allow only types with a
Vref source; one naming `landing-1` still loads without, and its attempts
then fail the `data` check where no Vref is known. The app's briefing and
the in-sim briefing show the method used.

**`landing-3`** is that method, specified in
[scoring.md](scoring.md): the same assessment the debrief shows (six
domains, ideal and tolerance from the checkride standard, levels from the
FDM thresholds, safety caps), run as a ranked attempt from the gate.
`challenge::evaluate` takes it from `crates/assess`; the attempt's
`score.total` is the landing's score and its `parts` are the six domains
(weight shared out where a domain is n/a; a safety cap holds the total
below their sum). It adds two checks: `data` (every expected value
recorded, `confidence: full`; a landing without a runway fix, peak g or
sink is not scored at all) and `go_around` (an attempt that went around
after the gate is not ranked: fly again). Boards should be split by
scoring class (light, turboprop, jet); the attempt does not carry the
class yet, the site reads it from the type. The attempt also carries
`assessment { method, score, grade, confidence, caps[{ kind, score,
reason, source }] }`, the debrief's grade bands, `confidence` and safety
caps for that landing, so a site does not rederive them from `parts`.

**`landing-1`** (retired 0.11.3) was the first challenge score: seven
weighted parts on smooth curves per class (touchdown point, sink rate,
centreline, stability, peak g, bounce, rollout). Its code is gone; old
attempts keep the result they were graded with, and a definition naming
it is graded with the current method.

The method is versioned: old attempts keep their score when it changes. Since conditions are fixed per challenge, no difficulty
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
2. **Scoring:** `landing-1` at first; `landing-2` ([scoring.md](scoring.md))
   for new challenges since 2026-10-27, `landing-3` since 0.11. Since
   2026-10-30 every challenge is graded with the debrief's current method
   whatever it names (`challenge::CURRENT`).

3. **Server analysis:** the Rust analysis on the home server:
   `fdm challenge <challenge.json> <flight.fqz>` prints the `Attempt`
   (checks, validity, score, the `aircraft` type and class graded, the
   `marker` it graded after) as JSON, the same `challenge::evaluate` the
   app runs. Only a touchdown after the latest `begin` marker is graded.
   `fdm slice <flight> --ref <ref> [-o out.fqz]` cuts the attempt's slice
   the app uploads ([format.md](format.md), Recording slice); the golden
   example a site's CI checks is in `fixtures/challenge/`. It takes a compact flight only, whose decompressed body is capped
   at 32 MiB for `fdm challenge` and `fdm slice` (256 MiB for the app's own
   flights); a journal or a raw capture is refused by its extension, since
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
let airport = sim.airport("LTFJ");                   // fdm_core::Airport with runways
let ils = sim.ils("LTFJ", "24");                     // Option<capture::Ils>
let placed = sim.place(Start { pos, gear_down: true, flaps: Some(0.875), nav1_hz });
placed.recv_timeout(Duration::from_secs(5))??;       // Result<(), String>
```

- `place` sets `InitPosition` (lat, lon, altitude ft MSL, pitch, bank, true
  heading, `on_ground`, airspeed kt; `sim_link::InitPosition`) on the user
  aircraft, then sends `AUTOPILOT_OFF`, `GEAR_DOWN`/`GEAR_UP`, if
  `flaps` is set `FLAPS_SET` (`flaps_set_data`: handle fraction × 16383,
  the sim snaps to the closest detent) and, if `nav1_hz` is set,
  `NAV1_RADIO_SET_HZ` (active NAV1, Hz; standby untouched). It runs at the
  next sim frame; a rejection by the sim is only logged.
- **The placement splits the recording** like any teleport
  (`Session::place`, 2026-10-30): the flight open closes where the
  aircraft was (kept as a flight of its own, or dropped under
  `MIN_FLIGHT_S` like any short recording), frames still at the old
  position are dropped, and a new flight opens at the first frame after
  the sim's `TELEPORT_DONE`, or `PLACE_SETTLE` (1 s) after the placement
  when the sim reports no teleport (`InitPosition` applies at the next sim
  frame; a second costs at most a second of the approach, never a frame of
  where the aircraft was). The sim's teleport flows within `OWN_TELEPORT`
  (10 s) are the app's own and split nothing more. `place` answers with
  that new flight's id, or an error when none opened within 10 s; the app
  binds the attempt to it (`Attempts::place` takes the flight from the
  placement), waits until it records and writes the begin marker there.
  So the attempt's flight, its debrief and its analysis begin exactly at
  the challenge start: no phantom takeoff, no teleport in it.
- `ils` (`sim_link::ils::IlsFetch`) asks the airport's runways for the
  end's ILS: `PRIMARY_ILS_ICAO`/`_REGION`/`_TYPE` when the runway's primary
  end (`PRIMARY_NUMBER`/`_DESIGNATOR`) is the asked one, else the
  `SECONDARY_*` ones; a type other than `'V'` or an empty ICAO is no ILS.
  Then it requests that VOR-type facility by ICAO and region for its
  `FREQUENCY`, refused outside 108–118 MHz. Both stages share one 10 s
  deadline. Fields: MSFS 2024 SDK, [`SimConnect_AddToFacilityDefinition`](https://docs.flightsimulator.com/msfs2024/retail/programming-apis/simconnect/api-reference/facilities/simconnect_addtofacilitydefinition/)
  (RUNWAY, VOR); event: [Aircraft Radio Navigation Events](https://docs.flightsimulator.com/msfs2024/retail/programming-apis/key-events/aircraft-radio-navigation-events/).
- The app (`place_challenge`) looks the ILS up before placing; without one,
  or if the lookup fails, it places anyway. The attempt log's `placed`
  start carries `nav1`: `NAV1 set to 110.30 (IZBA ILS 16L)`, `no ILS on
  34R: NAV1 left as is`, or why the lookup failed.
- `airport` answers from the facility database (thresholds:
  `analyzer::runway::thresholds`); it is also recorded like the airports
  captured for landings.
- While the sim is not connected both answer `Err` at once; if the sim
  quits first, the receiver reports disconnection.

Still to check live for the split: whether MSFS 2024 sends
`TELEPORT_START`/`DONE` for `InitPosition` (the log line `flight started`
right after the placement, and the old flight saved or discarded), that the
new flight's first frame is at the start, and that the attempt's debrief
starts on final.

Not yet verified against a live sim: the placement itself, `Pitch` sign
(SimConnect's usual positive nose-down assumed), whether key events sent
right after the teleport stick, and add-on aircraft that ignore key events.
For NAV1: that the runway `*_ILS_*` fields are filled (a [DevSupport thread](https://devsupport.flightsimulator.com/t/facility-data-airport-runway-missing-ils-info/13056)
reports them empty in MSFS 2024; the fallback would be the ILS approach's
recommended navaid), that an airport block with no values is answered,
that the VOR `FREQUENCY` is in Hz, and that the tuned NAV1 holds after the
teleport.

## Fly again: a clean start (implemented, 2026-10-30)

After a crash or damage, placing the same aircraft again would fly the
next attempt damaged. Fly again therefore starts every attempt from a
pristine aircraft, with no click in the sim:

1. **Every placement not reloaded from a clean start** (the first Place,
   and a Fly again with none saved for the loaded type) places as before,
   then saves the flight with
   `SimConnect_FlightSave` as the challenge's clean start:
   `<app data>/challenge-snapshots/<challenge key>.FLT`, beside
   `<challenge key>.json` with the ICAO type it was saved for. Each such
   placement replaces it (the old one is deleted first, so a failed save
   leaves none). The service saves it `SAVE_AFTER` (2 s) after the
   placement, so it holds the start itself: at the start, gear, flaps
   and NAV1 set. Log: `Clean start saved for Fly again (C172)`, or why
   not (`the loaded aircraft's type is not known yet`).
2. **Fly again** abandons the attempt (`replaced`), creates the next one
   and, if a clean start is saved for the type loaded now, reloads it
   (`SimRequests::reload`, `SimConnect_FlightLoad`), waits for the sim to
   settle, places the start as usual (position, autopilot off, gear,
   flaps, NAV1) and only then writes the begin marker. A clean start of
   another type is never reloaded: a reload brings back the saved
   aircraft, not the loaded one. Log: `Reloading the clean start (C172)`,
   `Reloaded in 12.3 s`; or `No clean start (none saved yet): placed as
   is`, `No clean start (the one saved is for a C172, a B738 is loaded):
   placed as is`, `Clean start not reloaded: <why>; placed as is`.
3. **The recording:** the load ends the recorded flight (flow
   `FLIGHT_END`/`FLT_LOAD`, `Session`), and the sim's own sequence after it
   opens and closes short flights of its own. The reload answers once the
   sim settled and the recorder opened a flight after it; the placement
   that follows splits that one too (a second or so on the runway, dropped
   as too short), and the new attempt binds to the flight the placement
   opened, as a first Place does: one split per attempt, never a flight
   that starts on the runway. The attempt replaced keeps its own flight.

**Bench, 2026-10-30** (MSFS 2024, C172, `foqa-reset-lab`): the sim showed
its crash message after a ground impact but SimConnect sent nothing: no
`Crashed` system event, no `PLANE_CRASH` flow, `CRASH FLAG` and `FLAP
DAMAGE BY SPEED` stayed 0. Damage cannot be detected, so the clean start
is reloaded at every Fly again. `SimConnect_FlightSave` works airborne
(`FlightSaved` at once); `SimConnect_FlightLoad` reloads in about 1.7 s,
airborne at the saved state with the saved fuel, visually intact. The
sim's own Restart then also reloads the last flight saved this way.

Flow after the load (`SIMCONNECT_FLOW_EVENT_ID`, SimConnect.h): `FLIGHT_END`
(15), `FLT_LOAD`/`FLT_LOADED` (1/2) with the file, `TELEPORT_START`/`DONE`
(3/4), `SimStop`/`SimStart`; about 10 s later the sim moves the aircraft
to the runway: `FLIGHT_START` (14), `taxi.flt` loaded, `RTC_START` (10),
about 2.5 s later a teleport, `runway.flt`, `RTC_END` (11). After
`RTC_END` the aircraft sits on the runway (flaps back at the takeoff
detent) and `InitPosition` puts it at the start correctly; placed before,
the move undoes it. The reload is therefore settled at `RTC_END` after the
load (`ReloadWait`, `crates/capture/src/place.rs`).

**Fallback:** without that move (MSFS 2020 sends no flow events; another
flow may not move the aircraft) the reload is settled 15 s after the last
`SimStart` after the load (`QUIET_AFTER`; the move starts about 10 s
after it), and the log says `Reloaded in … s, the sim did not move it to
the runway`. Nothing loaded within `RELOAD_TIMEOUT` (30 s), a move that
never ends, or a return to the main menu is a failed reload: the start is
placed as is in the flight recording then, or fails if there is none.

Still to check live: a full Fly again after a crash (the ~13 s wait, the
start placed after `RTC_END`, the attempt graded from the new flight);
that a save 2 s after the placement holds the configured start (flaps
moving, NAV1); the sim's crash dialog left open during the reload; a
clean start reloaded after the sim was restarted; the fallback on MSFS
2020; add-on aircraft.

**A crash flies again by itself.** MSFS 2024 showed its crash screen but
sent SimConnect nothing (2026-10-30 bench: no `Crashed`, no flow
`PLANE_CRASH`, `CRASH FLAG` 0), so the app reads the impact instead: a
touchdown during an attempt at 4 g or more, or 2,000 fpm or more (that
crash: 8.3 g, 10,400 fpm), ends the attempt (`crashed`, graded as it
stands) and starts Fly again at once, about 12 s after the impact when the
landing notice comes. Not yet verified: that `FlightLoad` works while the
crash screen is shown.

**Every attempt from the clean start, alone.** The first Place (and a Fly
again with no clean start for the loaded type) places the aircraft, saves
the clean start there and reloads it; Fly again reloads it. So every
attempt starts from the same saved flight, undamaged, and without AI air
traffic: the saved file gets `[AirTraffic] Status=0`, as the sim's own live
events and mission templates have (not their `[TrafficManager] disable=True`:
with it the sim never ended its move to the runway after a load, bench
2026-10-31) (2026-10-30: an airliner crossed a final and its wake upset the
approach). The first attempt takes the reload's 15 s too. A reload that
does not settle within 60 s places nothing (the sim would move the aircraft
back to the runway when it ends): the attempt asks for Fly again. Not yet verified
in the sim: that these sections turn the traffic off.

**Finished when stopped.** After a landing, the live frames tell when the
aircraft has stopped: ground speed below 2 kt held 3 s (`Monitor`,
`Happening::Stopped`). The attempt then finishes and is graded at once,
instead of `ROLLOUT` (120 s) after the landing or a press of Finish; the
120 s stay as the fallback. A stop that comes before the landing's analysis
(about 12 s after the last touchdown) waits for it, up to 30 s.

**Faster detection.** The crash is told from the live frames a second
after contact (`Impact`: descent rate on the last airborne frame, peak load
over the next second), not from the landing's analysis some 12 s later.

**A wing, propeller or nose strike is a crash too** (2026-10-30). A C172
landed fine on LTBJ 34L (247 fpm, 1.29 g), then ground-looped at 45 kt:
roll -25° and pitch -17° for 5 s, then +25°, a wingtip and the propeller
on the runway. MSFS's own challenges call that a crash; ours scored it
70.5 and it counted. The attitude is the evidence (`rules::strike`, one
measure for everything): after a touchdown, bank beyond the class's roll
limit (light 12°, transport 8°) or pitch below -5° on the ground, held
0.4 s (a single noisy frame never counts), or a touchdown frame already
banked beyond the roll limit. The limits and their sources are in
`crates/rules/src/strike.rs` (FOQA-Sim estimates from gear geometry and a
737 wingtip incident at 8.5°; a tailwheel type gets the nosewheel
value, the profiles cannot tell the gear layout).

- **Live:** `Monitor` follows it frame by frame and the service reports
  `Happening::Strike`; the app ends the attempts flown in that flight
  `crashed`, logs `Ground strike: wing strike (roll -25°, pitch -17°),
  ended as a crash` and starts Fly again, like a crash impact.
- **Grade:** the check `ground_strike` fails (`struck the ground with a
  wing or the nose: …`), so the attempt never counts; the landing-3
  assessment is held at 0 by the excursion cap, titled "Crash or ground
  strike"; the debrief shows the High event `ground-strike`.

Regression: `fixtures/c172-ltbj-34l-ground-strike.fqz` (that landing, `fdm
slice --from 150 --to 240`), `crates/challenge/tests/ground_strike.rs`:
the strike at 201.5 s, the check failed for a definition naming
landing-1 or landing-3, the score held at 0. Still to check live: that a real ground loop or
nose-over is told within half a second, and that hard but normal crosswind
landings (wing low at contact, up to 10° in a light aircraft) never are.

**The reload still takes about 15 s** (bench 2026-10-30: 15.4 s), most of
it the sim's own move to the runway: a saved free flight says
`FirstFlightState=PREFLIGHT_HOLDSHORT`. Tried the same day by editing the
saved file: `FLIGHT_DESCENT` (what the short-landing tutorial uses) loads
the aircraft where it was saved, but about 9 s later starts the sim's
`cruise.flt` intro (`FLIGHT_START`, `RTC_START`, no `RTC_END`), which waits
for Ready to fly and resets the flaps; `FREEFLIGHT` loads in under a second
with `RTC_END` at once and no later flow, but the aircraft stayed frozen
(the sim may have been held on a screen the bench could not see). To
verify with the screen in view before shortening the wait.
