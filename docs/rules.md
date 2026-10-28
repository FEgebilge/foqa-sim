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
domain: stability               # stability | energy | point | lateral | contact | rollout | other
aircraft: transport             # optional: light | transport
when: height < 500 ft           # optional precondition
measure: abs(roll)              # what is measured
detail: "{value} bank, limit {threshold}"   # optional, the event in words
sustain: 2 s                    # how long a level must hold
above: { low: 15 deg, medium: 20 deg, high: 30 deg }
```

Every rule names the `domain` of a landing assessment its events count
toward (`docs/scoring.md` §3): approach `stability`, threshold and flare
`energy`, touchdown `point`, `lateral` control, touchdown `contact`,
`rollout` and stopping, or `other` (never part of a landing score). A rule
without one fails to load.

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
  `sustain`; a run lasts from its first to its last point (`sustain: 2 s`
  needs 2 s between them). Points at a level up to 1 s apart belong to one
  run, whatever lies between (a dip under the threshold, unknown data), so
  a value hovering at a threshold is one event, not several or none; a
  longer gap ends the run and the episode. Its value is the worst one among
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
| `td.crosswind`, `td.tailwind` | kt | Wind at contact resolved on the runway heading: crosswind from either side; tailwind, negative for a headwind |
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
| `profile.vmo` | kt | Maximum operating speed (VNE for types without one), IAS |
| `profile.vle`, `profile.vlo` | kt | Maximum speed with the gear extended; while it moves (the lower of extension and retraction) |
| `profile.max_crosswind` | kt | Maximum demonstrated crosswind for landing (not a limit) |
| `profile.max_tailwind` | kt | Certified maximum tailwind for takeoff and landing |
| `profile.vso` | kt | Stalling speed in the landing configuration at maximum weight, IAS |
| `profile.main_gear_track` | ft | Distance between the main gear centrelines |
| `profile.vref` | kt | Vref at that point's gross weight and flap angle, from the profile's Vref table, approach speeds or VSO (below) |
| `profile.vapp` | kt | The approach speed the profile gives there (light types: the middle of the published range, or 1.3 VSO) |

## Type profiles

One YAML file per aircraft type, `profiles/<ICAO>.yaml`, named by its ICAO
Doc 8643 type designator; every file there is built into `fdm`, so adding a
type takes only its file. A flight is matched by the designator in its
recorded aircraft identity (`ATC MODEL`). Its class is then the profile's;
without a profile the class comes from the gross weight, and class rules
(`aircraft:`) still apply; a flight of unknown class is graded with the
transport rules, as with the transport phase boundaries. The debrief header
says which: `C172 profile`,
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
vmo: { value: 163 kt, source: "...", note: "VNE" }          # optional, as the next four
vle: { value: 200 kt, source: "..." }
vlo: { value: 180 kt, source: "...", approximate: true, note: "sister variant's value" }
max_crosswind: { value: 15 kt, source: "..." }
max_tailwind: { value: 10 kt, source: "..." }
flaps:                          # VFE bands by flap angle, increasing
  - { up_to: 10 deg, vfe: { value: 110 kt, source: "..." } }
  - { vfe: { value: 85 kt, source: "..." } }   # last band: no up_to = any larger
phases:                         # optional; moves the class's phase boundaries
  approach_ft: { value: 1500 ft, source: "FAA AIM 4-3-3 ..." }
scoring_class: turboprop        # optional: light | turboprop | jet (docs/scoring.md)
vso: { value: 40 kt, source: "..." }                       # optional, as the next three
main_gear_track: { value: 18.8 ft, source: "..." }
approach_speed:                 # published final-approach ranges (POH)
  source: "..."
  flaps:                        # increasing flap angle
    - { flap: 0 deg, ias: [65 kt, 75 kt] }
    - { flap: 30 deg, ias: [60 kt, 70 kt] }
vref:                           # Vref by gross weight per landing flap (FCOM/QRH)
  source: "..."
  flaps:
    - { flap: 30 deg, weights_kg: [40000, 60000], ias: [120 kt, 140 kt] }
```

`scoring_class` defaults to `light` for a light class and `jet` for a
transport one. A landing's Vref and approach speed come from the first of
(Data4Safety's order, `docs/scoring.md` §5): the `vref` table at the gross
weight (linear, held at its ends) on the flap angle nearest the landing
flap; the `approach_speed` range on the nearest flap angle (Vref its lowest
speed, the approach speed its middle); 1.3 `vso`. A value read beyond the
table's weights or on a flap angle more than 0.5 deg from its entry is
marked approximate. `weights_kg` are plain kilograms, as the tables are
published, strictly increasing with one `ias` each; a table has one
`source` (and optional `approximate`, `note`) for all its entries.

`phases:` overrides the class defaults of `docs/phases.md` where the type
differs, each value sourced like the others: `roll_kt`, `rto_kt` (kt),
`climb_end_ft`, `approach_ft`, `flare_ft`, `go_around_rise_ft`,
`go_around_drop_ft` (ft). Merged with the defaults, `rto_kt` may not be
below `roll_kt`, `climb_end_ft` may not exceed 1000 ft (where CICTT ends
the initial climb), `approach_ft` may not be below `climb_end_ft`, and
`flare_ft` must be below `approach_ft`. A flight of unknown class takes the
transport defaults. The debrief lists the values in effect and where each
comes from (`phase_values`). The shipped turbine singles (PC12, TBM9, C208)
start the approach at their 1500 ft pattern (FAA AIM 4-3-3).

VFE is looked up by the flap surface angle (`flap.te_angle`), as a flight data
recorder samples it and as VFE limits are defined, not by the handle index,
whose detent order the sim does not guarantee. At 0.5 deg or less the flaps
count as retracted and there is no VFE. Otherwise the first band whose `up_to`
plus 0.5 deg (flap travel and rounding) reaches the angle applies; beyond a
closed last band the VFE is unknown. The shipped profiles are listed by the
`shipped_profiles_load` test in `crates/rules/src/profile.rs`.

Every value carries its unit, checked at load like a rule threshold, and a
non-empty `source` (document or URL); `note` is optional. A wrong unit, a
missing source, a `type` that does not match the file name, `up_to` not
strictly increasing or missing on a band that is not the last, phase
boundaries out of order, or an unknown field fails the load.

A value is the type's own published one where that was found (AFM/POH, type
certificate data sheet, FCOM/FCTM, maintenance manual). Otherwise the
closest one found is used, marked `approximate: true`, and its `note` must
say how it was derived (a sister variant, a secondary source, a TCDS value in
KEAS); the debrief shows it as `≈`. A value with no basis at all is left out.
Speeds from a TCDS in KCAS are used as IAS (within a few knots at these
speeds).

## Events

Each event records the rule id and version, level, start and end time, the
worst measured value at that level, when it was measured and the threshold
it crossed. A debrief can therefore show why an event fired, and against
which version of its definition, and in plain language what it means
(`why_it_matters`) and how to avoid it (`advice`); every core rule carries
both.

`detail` says the event in words, e.g. `{ias} at flaps {flap.te_angle},
limit {profile.vfe}` reads "111 kt at flaps 12°, limit 85 kt". Each `{…}` is
`value` (the measured value), `threshold` (the one it crossed) or a numeric
expression of the rule's names, read where the value was measured; all are
checked at load and shown with their unit. A value that would read as its
threshold keeps one more decimal. If a placeholder is unknown there the event
has no detail. Without `detail` a numeric rule reads `{value}, limit
{threshold}` and a condition has none.
