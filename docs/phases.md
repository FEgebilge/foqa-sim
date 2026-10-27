# Flight phases

How FOQA-Sim splits a flight into phases, why each boundary is where it is,
and how the boundaries follow the aircraft. Phases matter beyond the
timeline: every continuous rule searches only its phases (`phase:` in
`rules/core.yaml`), so a wrong boundary moves or hides events.

Basis: the ICAO/CAST Common Taxonomy Team phase-of-flight definitions
(CICTT v1.3), which every FDM programme maps its own detection onto; EASA
EOFDM on phase splitting; UK CAA CAP 739 event windows; EASA Data4Safety
unstable-approach guidance (window 1000 ft to touchdown). CICTT defines
phases by crew actions ("application of takeoff power", "beginning of the
flare"); a recorder sees parameters, so each boundary below is a measurable
proxy, as in airline FDM software.

Code: `crates/analyzer/src/phase.rs` (after the flight, may look ahead) and
`live.rs` (the in-sim panel, frame by frame), both from one `PhaseConfig`.

## Phases and boundaries

Heights "above the runway" are the calibrated radio height, or the height
above the aerodrome (AAL, pressure altitude) where RA is invalid.

| Phase | CICTT | Starts when (proxy) | Ends when |
|---|---|---|---|
| Standing | STD | On the ground under 2 kt | Moving |
| Taxi | TXI | Moving on the ground under `roll_kt` | A takeoff roll starts, or stopped |
| Takeoff | TOF | On the ground above `roll_kt` and continuing to liftoff; after a touch-and-go, at the lowest ground speed of the roll (power applied) | 35 ft above the runway |
| Initial climb | ICL | 35 ft above the runway | `climb_end_ft` above the aerodrome, or the top of a lower climb |
| En route | ENR | End of the initial climb | Descending through `approach_ft`; for a leg topping under it, through `climb_end_ft` |
| Approach | APR | Descending through `approach_ft` above the aerodrome, past the top of a climb that stayed under `climb_end_ft`, or descending through `climb_end_ft` from a top between the two | Flare: below `flare_ft` above the runway |
| Landing | LDG | Below `flare_ft` | Under `roll_kt` after touchdown, or the lowest ground speed of a touch-and-go |
| Go-around | APR subphase | The lowest point of an approach from which the aircraft climbs `go_around_rise_ft` without touching down | The climb ends (`go_around_drop_ft` below its peak), or back above `approach_ft` |

An approach is entered from above only: a climb through `approach_ft`
stays en route when `approach_ft` is above `climb_end_ft`, so it cannot be
taken for an approach or a go-around. A leg that tops out between the two
(a low cruise under a 1500 ft turbine pattern) never entered the pattern
from its height, so its approach starts at CICTT's other VFR mark,
descending through `climb_end_ft` (1000 ft); until then a dip and climb in
that cruise is en route, not an approach and a go-around.

A touch-and-go splits the ground run between the two legs at its lowest
ground speed: CICTT ends the landing when takeoff power is applied, and the
speed starts rising there. The second leg so has its own takeoff roll and
the takeoff rules (e.g. tailstrike risk at rotation) check it. Live, the
switch comes once the speed is 5 kt above that lowest, a few seconds later.

## Values by aircraft

Defaults by class; a type profile may override them (`phases:` in
`profiles/<ICAO>.yaml`, `docs/rules.md`), each value with its source. A
flight of unknown class takes the transport values. Merged with the
defaults, `climb_end_ft` is at most 1000 ft, `approach_ft` at least
`climb_end_ft`, and `flare_ft` below `approach_ft`.

| Value | Light | Transport | Why |
|---|---|---|---|
| `roll_kt` | 30 | 30 | Above any taxi speed, below every liftoff speed |
| `climb_end_ft`, `approach_ft` | 1000 | 1000 | CICTT: 1000 ft or the VFR pattern; FAA AIM 4-3-3 pattern 1000 ft AGL (piston), 1500 ft (turbine, by profile) |
| `flare_ft` | 20 | 50 | Light flare about 10–20 ft; transport 30–50 ft, 50 ft being also the threshold crossing height |
| `go_around_rise_ft` | 200 | 200 | A smaller rise is not told apart from a balloon or a levelling-off at these sample rates |
| `go_around_drop_ft` | 50 | 50 | Smaller dips in a go-around climb are noise |

Shipped overrides: PC12, TBM9 and C208 start the approach at 1500 ft (FAA
AIM 4-3-3, turbine-powered aircraft enter the pattern at 1500 ft AGL).

The rule thresholds stay independent of the boundaries: approach-gate
rules (1000 ft, 500 ft) use heights, not phase boundaries. Moving
`flare_ft` lengthens the approach rules' window down to the flare; the
stall-warning rule keeps its own 50 ft exclusion (below 50 ft only while
climbing), so the light flare's horn is not a finding.

## Explaining them

- Each phase span carries the cause that opened it (`Span.cause`: trigger,
  value, threshold, basis GroundSpeed, Ra or Aal), which the debrief words
  as `PhaseSpan.trigger`: "on the ground above 30 kt (34 kt)",
  "35 ft above the runway (36 ft RA)", "descending through 1000 ft above
  the aerodrome (999 ft AAL)", "top of climb, under 1000 ft above the
  aerodrome (820 ft AAL)", "lowest speed of the touch-and-go roll, takeoff
  power applied (48 kt)".
- The debrief header lists the values in effect and where each came from
  (`phase_values`: "light class default", "PC12 profile, FAA AIM 4-3-3 …").
- `fdm analyze` prints the trigger beside each span.

## Not yet (known gaps)

- Climb, cruise and descent subphases; pushback.
- Rejected takeoff (a takeoff roll that slows without a liftoff).
- Runway exit (not groundspeed alone) as the end of the landing roll, which
  EOFDM recommends.
- Go-around by thrust or TOGA rather than by height.
- Live AAL at a destination with a different elevation: the panel measures
  from the last aerodrome on the ground (the departure) until touchdown,
  so its approach and flare boundaries are off by the elevation difference;
  the debrief uses the arrival aerodrome after the leg's top.
