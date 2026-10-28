# Flying skills

Beyond the landing score, FOQA programmes look at how a flight was flown: how much of it was
hand-flown, how smooth it was for the cabin, how efficiently it descended. FOQA-Sim reports
these as **measures, not a grade**. Turbulence, the aircraft and ATC move every one of them, and
more hand-flying is not by itself better flying (FAA SAFO 13002), so there is no composite
"skill score". Each measure names its source; a reference line without a firm source is marked
*estimate*.

`debrief::skills(&Analysis) -> Skills` computes them once; the debrief (`Debrief.skills`), the
flight summary (`FlightSummary.skills`, the figures trends need) and the CLI (`fdm debrief`
prints a "Skills" block) all take them from there.

## Automation

EASA's FDM precursor guidance (EOFDM, LOC29) recommends fleet and phase baselines for autopilot
and autothrottle engagement, the height of the autopilot disconnect on approach, and time from
disengagement to manual input.

| Field | Meaning |
|---|---|
| `airborne_s` | Time in the air, lift-off to touchdown, summed over the legs |
| `ap_s`, `at_s` | Of it, with the autopilot / autothrottle engaged (`ap.engaged`, `at.engaged`) |
| `by_phase` | `[{phase, s, ap_s, at_s}]` for each phase with airborne time, in phase order (take-off and landing only their airborne part) |
| `ap_off_ft` | Radio height (height above the aerodrome where the radio altimeter is invalid) of the last autopilot disconnect in the final landing's approach phase; `null` if the autopilot was never on in it or for an autoland |
| `autoland` | The autopilot was still engaged at contact: no disconnect, no hand-flown final |
| `manual_final_s` | Hand-flown time from that disconnect to touchdown |

## Ride quality

A **band-limited comfort proxy**, not ISO 2631 compliance: the recorder samples normal load at
8 Hz and lateral load at 4 Hz, while ISO 2631-1 weights 0.5-80 Hz at the seat.

| Field | Meaning | Reference |
|---|---|---|
| `vertical_rms` | RMS of normal load minus its 2 s moving mean, airborne, in m/s² | ISO 2631-1 Annex C bands: < 0.315 not uncomfortable, 0.315-0.63 a little, 0.5-1 fairly, 0.8-1.6 uncomfortable, 1.25-2.5 very, > 2 extremely |
| `ride_band` | The band `vertical_rms` falls in (the lowest that contains it, upper bounds exclusive): `not uncomfortable`, `a little uncomfortable`, `fairly uncomfortable`, `uncomfortable`, `very uncomfortable`, `extremely uncomfortable` | as above |
| `lateral_rms` | RMS of the side load the ball shows, airborne, in m/s²: the recorded body acceleration minus gravity's share along the wing, sin(bank)·cos(pitch), which a coordinated turn carries (a 25° turn would otherwise read 0.4 g) | descriptive; "ball centred" is the reference (FAA PHAK ch. 8) |
| `sideslip_rms_deg` | RMS sideslip, airborne below 10,000 ft above the aerodrome and at 40 kt or more (so out of the take-off and landing roll, where the vane means nothing) | descriptive |
| `max_bank_deg` | `{phase: peak bank}`, airborne, for each phase with airborne time | 30° (*estimate*: common airline SOP; Boeing FCTM 3.35 limits 15° below V2+15) |
| `rotation_rate_dps` | Peak pitch rate (over 0.5 s) from rotation, the pitch 1° above its value at the start of the take-off roll, to 35 ft; transport class only | about 3°/s (Airbus, "What about rotation technique", 2020) |

## Fuel and descent

No flight plan is recorded, so nothing is compared with planned fuel (ICAO A39-WP/431 §4.6
lists the recorded figures a fuel programme starts from).

| Field | Meaning |
|---|---|
| `fuel_used_kg` | `fuel.total` (recorded in lb, shown in kg) at the start of the first take-off roll minus at the end of the last landing roll; `null` if fuel rose by more than 1 kg in between (refuelled in flight or a slew) |
| `fuel_kg_h` | Airborne burn (each leg, lift-off to touchdown, so no rolls or taxi between circuits) over the airborne time |
| `fuel_kg_nm` | Airborne burn over the ground distance flown airborne |
| `taxi_fuel_kg` | Fuel burnt in the taxi phases (out and in) |
| `descent_level_s`, `descent_levels` | Time and number of level segments in the descent: vertical speed within ±300 fpm for at least 10 s between the top of descent and 1,000 ft above the landing runway (the top is the last point of the final leg within 100 ft of its highest, there being no descent phase; the end where the last stretch below 1,000 ft above the aerodrome begins) (EUROCONTROL, Vertical Flight Efficiency technical note §3.1) |

## Control activity

Counted only while hand-flying (autopilot off, airborne).

| Field | Meaning | Reference |
|---|---|---|
| `manual_s` | Hand-flown airborne time (autopilot off); `null` without `ap.engaged` | |
| `reversals_per_min` | `{pitch, roll}`: direction changes of `ctl.pitch` / `ctl.roll` larger than 0.5 % of full travel (the controls run -100 to 100 %, so 1.0), per hand-flown minute | NASA NTRS 20140005730 ("Aggressiveness and Duty Cycle Metrics", eq. 2), detection setting only, not a limit |

## Summary and trends

`FlightSummary.skills` carries `ap_share`, `at_share`, `ap_off_ft`, `autoland`, `vertical_rms`,
`ride_band`, `fuel_kg_h`, `fuel_kg_nm`, `descent_level_s`, all optional. `Trends` gains
`skills: {ap_share (the mean of the flights' shares), ap_off_ft_median (hand-flown landings only), autolands (flights that landed on the autopilot), vertical_rms_median, descent_level_s_median, fuel_kg_h:
[{icao, median}]}` over the matching flights (fuel per type only: types are not comparable).

A measure is `null` when its parameters are missing or the flight has no such phase; it is never
filled with zero. Under 60 s airborne (a hop, a slew) ride, fuel and control are all `null`: an
RMS or a rate over a few seconds would read as smooth flying.

## Command line

`fdm debrief <flight>` prints a short "Skills" block (`-` for a measure the flight cannot give);
`--json` prints the whole debrief, `skills` included.
