# Rule language

A rule file is YAML, one event definition per document (`---` between them).
The built-in set is [`rules/core.yaml`](../rules/core.yaml); `fdm analyze
<flight> --rules my.yaml` uses another. Every rule is compiled and
unit-checked when loaded, so a broken rule fails before any flight is read.

```yaml
rule: bank-below-500            # id, unique in the file
version: 1                      # bump when the definition changes
title: Excessive bank below 500 ft
description: Source of the thresholds.
why_it_matters: Steep bank near the ground raises the stall speed.  # optional, for pilots
advice: Finish the turn to final above 500 ft.                     # optional, for pilots
phase: [Approach, Landing]      # CICTT phases the rule applies to
aircraft: transport             # optional: light | transport
when: height < 500 ft           # optional precondition
measure: abs(roll)              # what is measured
sustain: 2 s                    # how long a level must hold
above: { low: 15 deg, medium: 20 deg, high: 30 deg }
```

## Triggers

| Field | Evaluated | Notes |
|---|---|---|
| (none) | Every point of the phases (8 Hz) | `sustain` sets the minimum duration; 0 or absent: one point is enough |
| `gate: <condition>` | Once, where the condition becomes true | Fires at the first point where `when` and `measure` are known, exceeded or not. Re-arms when the condition is known false or the phases did not apply for 5 s (e.g. after a go-around); an unknown condition changes nothing |
| `at: touchdown` | Once per main-gear touchdown | `phase` not needed; `td.*` values available |

## Limits

- A numeric `measure` takes `above` and/or `below`, each with any of
  `advisory`, `low`, `medium`, `high`. Thresholds are exceeded strictly, and
  carry the measure's unit. Thresholds must grow with the level (shrink for
  `below`), and a map needs at least one level; otherwise the rule fails to
  load.
- A condition `measure` takes `level:` and raises it whenever it holds.
- In a continuous rule, an event's level is the highest one held for
  `sustain` without a break; a run lasts from its first to its last point
  (`sustain: 2 s` needs 2 s between them). Its value is the worst one among
  the points at exactly that level in the longest such run, so value and
  threshold always agree with the level. A brief spike over a higher
  threshold does not change either; the event still spans the whole episode.

## Expressions

Numbers carry units: `ft`, `kt`, `deg`, `g`, `fpm`, `dot`, `%`, `degC`,
`hPa`, `lb`, `s`. Operands of `+ - < <= > >= == !=` must have the same unit.
`* /` only scale by a plain number. A unit mistake is a load error that
points at the problem:

```
rule `bank-below-500`, `when`:
height < 500 fpm
         ^^^^^^^ compares ft with fpm
```

Operators: `and`, `or`, `not`, arithmetic, comparisons, parentheses.
Functions: `abs(x)`, `min(a, b)`, `max(a, b)`. Literals: `true`, `false`.

Missing or invalid data (e.g. no glideslope signal) makes an expression
**unknown**, not false. A rule is only evaluated where `when` is known to be
true. `and` / `or` still decide if one side settles it (`false and ?` is
false).

### Names

| Name | Unit | |
|---|---|---|
| Any LFL parameter (`ias`, `roll`, `alt.radio`, `sel.speed`, …) | its LFL unit | Discrete ones (`air_ground`, `ap.engaged`) are conditions |
| `vs` | fpm | Inertial vertical speed (derived) |
| `height` | ft | Above the departure aerodrome up to the highest point, above the arrival aerodrome after |
| `ra` | ft | Radio height, on-ground bias removed, valid below 2500 ft |
| `td.sink` | fpm | Touchdown rules: rate of descent at contact |
| `td.peak_g` | g | Highest normal load around contact |
| `td.pitch`, `td.roll` | deg | Attitude at contact |
| `td.ias` | kt | Airspeed at contact |
| `td.bounce` | condition | A touchdown after a short hop |
| `td.distance` | ft | Touchdown distance past the landing threshold; negative = before it |
| `td.cross` | ft | Offset from the runway centreline, right positive (`abs(td.cross)` for a limit) |
| `td.lda`, `td.tdz` | ft | Landing distance available; touchdown zone length min(3000 ft, LDA/3) |
| `td.in_tdz`, `td.long_landing`, `td.short_landing` | condition | Inside the touchdown zone; beyond it; before the threshold |

The runway values come from the airport snapshots recorded in the flight and
the ground track after touchdown. Without airport data, or when no runway
matches, they are unknown and rules using them do not fire.

Type-specific limits come from the aircraft's type profile (below), in any
rule. Without a profile, or where it lacks the value, they are unknown and
rules using them stay silent.

| Name | Unit | |
|---|---|---|
| `profile.vfe` | kt | VFE of the flap extension (`flap.te_angle`, see the bands below); unknown with the flaps retracted |
| `profile.tail_takeoff_pitch` | deg | Pitch at which the tail touches the runway, main gear struts extended (rotation) |
| `profile.tail_landing_pitch` | deg | The same with the struts compressed (touchdown) |
| `profile.hard_landing_g` | g | Manufacturer's hard-landing maintenance inspection load |

## Type profiles

One YAML file per aircraft type, `profiles/<ICAO>.yaml`, named by its ICAO
Doc 8643 type designator; every file there is built into `fdm`, so adding a
type takes only its file. A flight is matched by the designator in its
recorded aircraft identity (`ATC MODEL`). Its class is then the profile's;
without a profile the class comes from the gross weight, and class rules
(`aircraft:`) still apply. The debrief header says which: `C172 profile`,
`transport class default (no B744 profile)` or
`no aircraft identity, unknown class`.

```yaml
type: C172                      # must match the file name
name: Cessna 172S Skyhawk SP (G1000)
class: light                    # light | transport
tail_contact:                   # optional; body pitch at tail contact
  takeoff: { value: 11 deg, source: "...", note: "..." }   # struts extended
  landing: { value: 9 deg, source: "..." }                 # struts compressed
hard_landing_g: { value: 2.2 g, source: "..." }            # optional
flaps:                          # VFE bands by flap angle, increasing
  - { up_to: 10 deg, vfe: { value: 110 kt, source: "..." } }
  - { vfe: { value: 85 kt, source: "..." } }   # last band: no up_to = any larger
```

VFE is looked up by the flap surface angle (`flap.te_angle`), as a flight data
recorder samples it and as VFE limits are defined, not by the handle index,
whose detent order the sim does not guarantee. At 0.5 deg or less the flaps
count as retracted and there is no VFE. Otherwise the first band whose `up_to`
plus 0.5 deg (flap travel and rounding) reaches the angle applies; beyond a
closed last band the VFE is unknown. Shipped: C172, C25C, C700 (flaps),
B38M, B738 (tail contact; B738 also hard landing). B78X and SF50 have no
verified type values and use the class default.

Every value carries its unit, checked at load like a rule threshold, and a
non-empty `source` (document or URL); `note` is optional. A wrong unit, a
missing source, a `type` that does not match the file name, `up_to` not
strictly increasing or missing on a band that is not the last, or an unknown
field fails the load. Values without a published source are left out, never estimated.

## Events

Each event records the rule id and version, level, start and end time, the
worst measured value at that level, when it was measured and the threshold
it crossed. A debrief can therefore show why an event fired, and against
which version of its definition, and in plain language what it means
(`why_it_matters`) and how to avoid it (`advice`); every core rule carries
both.
