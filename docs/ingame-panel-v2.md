# In-game panel v2: message contract

Comm Bus messages between the FOQA-Sim app (Rust, `crates/capture/src/panel.rs`,
live values in `monitor.rs`) and the in-sim views (toolbar panel now, EFB app
later). JSON, UTF-8. Every message carries `v: 2`. Types reuse
`docs/debrief-api.md` (`RecorderStatus`, `Event`, `LandingNotice`,
`Touchdown`, `Level`). A view only hears what is sent while it is open, so
every app → view message is repeated.

## App → view

| Event | Payload | Cadence |
|---|---|---|
| `FOQA_SIM_STATUS` | `Status` | 1 Hz while the sim is connected; at once with an `ack` |
| `FOQA_SIM_LIVE` | `Live` | 4 Hz while recording |
| `FOQA_SIM_EVENTS` | `Events` | on change, else every 5 s while recording |
| `FOQA_SIM_LANDING` | `{ v, age_s, notice: LandingNotice }` | at the landing, then every 5 s (v1, unchanged but `v: 2`) |
| `FOQA_SIM_LANDINGS` | `{ v, landings: LandingNotice[] }` | every 5 s while recording; the last 3 landings of this flight, newest first |

`EVENTS` and `LANDINGS` come from the app's own analysis of the open
journal, run every 5 s in the background (one at a time), so they trail the
flight by up to about 5 s plus the analysis time; the first arrives a few
seconds into the flight.

```ts
interface Status {
  v: 2;
  status: RecorderStatus;
  flight?: {                 // while recording only
    id: string;
    aircraft?: string;       // ICAO type, else sim title (from the analysis)
    departure?: string;      // ICAO
    elapsed_s: number;       // wall clock
    phase: Phase;            // the latest Live.phase, "Standing" before any
    landings: number;        // from the analysis
  };
  ack?: { cmd: string; ok: boolean; message?: string };  // reply to a command, sent once; message on failure
}

type Phase = "Standing" | "Taxi" | "Takeoff" | "InitialClimb" | "EnRoute"
           | "Approach" | "Landing" | "GoAround";

interface Live {
  v: 2;
  t_s: number;               // sim time since the flight's first frame
  phase: Phase;              // live CICTT phase, see below
  on_ground: boolean;
  aal_ft: number | null;     // above the aerodrome of the last ground contact; 0 on the ground; null for a flight started airborne
  ias_kt: number | null;
  vs_fpm: number | null;     // inertial, + up
  flap_deg: number | null;   // trailing-edge flap angle
  vfe_kt: number | null;     // profile flap limit at this flap angle; null with flaps up or no profile band
  bank_deg: number | null;   // + right wing down
  gates: Gate[];             // [1000 ft, 500 ft] throughout Approach and Landing, else empty
}

interface Gate {
  at_ft: 1000 | 500;
  state: "ahead" | "stable" | "unstable";   // "ahead": not yet passed, checks are the current values
  checks: Check[];           // frozen when the gate is passed
}

interface Check {
  name: "sink" | "speed" | "bank" | "flaps";
  ok: boolean;
  value?: number;
  limit?: number;            // only where one exists
  unit: "fpm" | "kt" | "deg";
}

interface Events {
  v: 2;
  flight_id: string;
  events: Event[];           // all events of this flight so far, time order, pilot reports included
  counts: Record<Level, number>;
}
```

**Phase.** The analyzer's CICTT phases (`analyzer::phase`) computed frame by
frame by `analyzer::LiveTrack`, with the same thresholds (35 ft takeoff end,
1000 ft initial climb / approach, 50 ft flare, 200 ft go-around rise, 30 kt
roll, 3 s bounce). The ground segment decides some boundaries from what
follows (the top of a climb, a go-around, a bounce); live, they are decided
once they have happened, so the debrief's phases can differ by a few
seconds. `aal_ft` uses the analyzer's aerodrome height (pressure altitude on
the ground, where the airport snapshots are captured): the departure, or
the last landing after a touch-and-go, as the analyzer has it for a flight
not yet landed.

**Gates.** FSF ALAR's 1000 ft (IMC) and 500 ft (VMC) gates, the same
criteria at both. A gate is passed at the first airborne frame at or below
its height in Approach/Landing; both reset when the phase leaves Approach /
Landing (a go-around). No new thresholds: each check uses a rule of
`rules/core.yaml` and the aircraft profile.

| Check | Value | Limit and `ok` |
|---|---|---|
| `sink` | `-vs`, fpm | rule `unstable-sink-500`: `ok` below its lowest level, `limit` 1000 |
| `speed` | IAS, kt | rule `flap-overspeed`: `limit` = profile VFE at the flap angle + its lowest level (0 kt); no `limit` and `ok` with flaps up or no profile |
| `bank` | `abs(roll)`, deg | rule `unstable-bank-500`, `limit` 7 |
| `flaps` | flap angle, deg | `ok` when extended (beyond the profile tolerance, 0.5 deg); no `limit` |

A check is left out when its value is unknown or its rule does not apply to
the aircraft class.

## View → app

Sent with `listener.callSimConnect("FOQA_SIM_CMD", json)`; the app
subscribes with `SimConnect_SubscribeToCommBusEvent` on the panel feed's
connection and ignores trailing NULs.

| Event | Payload | |
|---|---|---|
| `FOQA_SIM_CMD` | `{ v: 2, cmd: "end_flight" }` | close and save the flight now, as on return to the menu |
| | `{ v: 2, cmd: "discard_flight" }` | close the flight and delete its files (the view asks to confirm first) |
| | `{ v: 2, cmd: "mark", note?: string }` | mark this moment in the journal (pilot report) |

The app answers each command once in `Status.ack` (`cmd`, `ok`, `message`
on failure, e.g. `"no flight open"`, `"unknown command"`, `"unsupported
version"`, `"not JSON: …"`; `cmd` is `"?"` when unreadable).

- `end_flight`: answered once the flight is saved; it is then analysed and
  compacted as usual (`flight-saved` in the app). The next flight opens at
  the sim's next flight start, as after the main menu.
- `discard_flight`: answered once the flight is closed and its `.fqj`,
  `.fqz` and `.raw.jsonl.gz` are deleted, nothing else.
- After either, `Status` has `state: "connected"` and no `flight`.
- `mark`: answered at once; the note (cut to 1000 bytes) is recorded as a
  pilot report and appears in the next `Events` as an advisory event
  `rule: "pilot-report"`, `title: "Pilot report"`, `description`: the note
  (see `debrief-api.md`).
- A command while no flight is recorded fails with `"no flight open"`.
