# Reference approach

The landing score says *how much* an approach cost; the reference approach shows *where*. Each
landing's approach is drawn against the profile it was graded by, as an instructor's debrief
overlays the flown path on the ideal one: the 3° path to the threshold, the approach speed, the
sink rate that path needs at your ground speed, and the stabilised gate, with the bands of
`scoring/landing-3.yaml` around each. There is no second set of numbers: the bands on the chart
are the criteria's own (`CriterionAssessment.bands`), so the picture and the score always agree.

## What the reference is

| Line | Reference | Source |
|---|---|---|
| Vertical path | 3.0° to a 50 ft threshold crossing height: `50 + dist_ft · tan 3°` | ICAO PANS-OPS Vol II (standard glide path 3°, RDH 15 m / 50 ft); FAA AIM 1-1-9 |
| Glideslope, localizer | The recorded ILS deviation in dots, where the landing runway's ILS was received ([format.md](format.md#ils-deviations-and-the-localizer-course)); that is the runway's real path, steep approaches included | criteria A5G, A5L |
| Speed | `vapp_kt` (Vref plus the wind additive, `LandingAssessment.speeds`), bands of A1 | docs/scoring.md 2.4 |
| Sink | The sink the reference path needs at the recorded ground speed: `gs_kt · 6076 / 60 · tan 3°` (about 5.3 × GS), bands of A2 | 1-in-60 rule; FSF ALAR 7.1 (1,000 fpm) |
| Gate | `stable_from_ft` and `split_ft` (class gates, e.g. 1,000 / 500 ft transport), landing configuration by the gate (A6) | docs/scoring.md 2.2 |

The 3° path is the standard, not every runway's: where an ILS was received, the glideslope trace
is the truth and the 3° line is shown only as a reference; a non-precision approach with a
published steeper angle is not known to FOQA-Sim (no procedure database) and the chart says so.

## Wire

`LandingAssessment.profile` (absent without a runway fix or an approach window):

```ts
interface ApproachProfile {
  t_s: number[];          // the approach window's points (track time), oldest first
  dist_nm: number[];      // to the threshold along the extended centreline, positive before it
  height_ft: number[];    // above the runway (radio height, else above the aerodrome), as graded
  gs_dot: number[];       // glideslope deviation, dots, fly-up positive; NaN without a signal
  loc_dot: number[];      // localizer deviation, dots, right of course positive; NaN without a signal
  path_deg: number;       // 3.0
  tch_ft: number;         // 50
  top_ft: number;         // the window's top and bottom heights (scoring/landing-3.yaml windows)
  bottom_ft: number;
}
```

Every other line (IAS, ground speed, vertical speed, flap, gear) is read from `Debrief.series` at
`t_s`, and the bands from the landing's criteria A1, A2, A5G, A5L. `NaN` goes out as `null`.
Each series criterion also carries `first_out_s` and `worst_s` (`CriterionAssessment`): where
the score found it first left its tolerance band, persistence included, and where it was worst.
The list below the chart reads these, so it never disagrees with the grade.

## Screen

On each scored landing, an "Approach profile" with distance to the threshold on the x axis
(furthest left, threshold right, so it reads like the approach): vertical path, speed against
Vapp, sink against the path's need, and a configuration strip, each with the criterion's bands
shaded and the gate heights marked. Under it, "where you left the profile": for each criterion
that reached level 1 or more, the first point it did (`first_out_s`), as a distance and a height,
linking the replay cursor there.
