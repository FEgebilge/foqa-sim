# Landing assessment `landing-2` (design, draft)

One method grades every approach and landing. The same numbers drive the
debrief's Good / Fair / Poor, the 0–100 challenge score and the
leaderboard. The debrief and the leaderboard therefore tell the same story.
`landing-1` (`crates/challenge/src/score.rs`) stays frozen for the
challenges that name it.

Status: slices 1-4 of §7 are implemented (profile fields, analyzer
values, rule set v2, `crates/assess` with `scoring/landing-2.yaml`); the
debrief, challenges and UI do not use it yet. Thresholds marked
**est** are FOQA-Sim choices that no published source covers; they are
listed so they can be reviewed, not hidden.

## 1. Method in one paragraph

A landing is graded in six domains, the way an airline FDM programme and a
checkride examiner would look at it: approach stability, threshold and
flare energy, touchdown point, lateral control, touchdown contact, and
rollout and stopping. Each measured value is placed on one scale. Level 0
means within the checkride tolerance (FAA ACS for the class). Levels 1 to 3
use the FDM event thresholds, mainly the EASA Data4Safety unstable-approach
thresholds, the type's AFM/AMM limits from its profile, and the core rule
set. Each domain loses points by its worst level only, so one mistake is not
counted twice. Every deduction is explained by the measured value or the
event card that caused it. Safety outcomes then cap the score: continuing an
unstable approach to a landing, a hard landing that needs an inspection, or
leaving the runway. A soft landing earns no extra credit: a firm landing on
the aiming point scores better than a smooth one that floats long. This is a
standards-based assessment with a published scoring policy. No regulator
publishes a 0–100 landing score; the weights are ours.

### Sources

✔ = the primary document was fetched on 2026-10-27 and the cited figure was
read in it. ⚠ = the figure comes from the research brief or a secondary
source and was not re-read in the primary document.

| # | Source | Used for | |
|---|---|---|---|
| S1 | EASA Data4Safety, *Guidance for identifying unstable approach with flight data*, Rev 00, April 2022, pp. 14–15, 19–20 ([pdf](https://www.easa.europa.eu/en/downloads/136957/en)) | Speed > Vref+20 / +35 kt (3 s), < Vref−5 / −10 kt (3 s); descent > 1200 fpm (5 s) / > 1500 fpm (3 s); pitch > 10°/< −3° and > 15°/< −10°; roll > 15° / > 30° (3 s); window 1000–50 ft; glideslope > 1 / > 2 dots and localiser > 1 / > 1.5 dots (5 s, 1000–500 ft); late flap > 2° or 1 notch and late gear (1000–0 ft); low thrust (fleet 1st percentile, 10 s). Severity SL1 / SL2A (1st / 2nd threshold above 500 ft), SL2B / SL3 (below 500 ft). An approach is unstable with ≥ 3 criteria at SL1/SL2A or ≥ 1 at SL2B/SL3. Vref from a recorded value, from a gross-weight × flap table, or from gross weight alone | ✔ |
| S2 | FAA-S-ACS-11A, ATP and Type Rating, AA.III.B.S7, S10, S12 ([pdf](https://www.faa.gov/training_testing/testing/acs/atp_airplane_acs_11.pdf)) | Approach speed ±5 kt; touchdown at the aiming-point markings −250/+500 ft (no markings: 750–1500 ft from the threshold); runway centreline between the main gear; taxi speed (≤ 20 kt dry) within the landing distance + 25% | ✔ |
| S3 | FAA-S-ACS-6C, Private, PA.IV.B.S7, S10 ([pdf](https://www.faa.gov/training_testing/testing/acs/private_airplane_acs_6.pdf)) | Published approach speed, or 1.3 VSO without one, +10/−5 kt with the gust factor applied; touchdown on the specified point to 400 ft beyond it; no side drift; longitudinal axis aligned with and over the centreline | ✔ |
| S4 | FAA-S-ACS-7B, Commercial, CA.IV.B.S7, S10 ([pdf](https://www.faa.gov/training_testing/testing/acs/commercial_airplane_acs_7.pdf)) | Same, ±5 kt; specified point to 200 ft beyond it | ✔ |
| S5 | FSF ALAR Briefing Note 7.1, Stabilized Approach (Task Force V1.1, Nov 2000) ([pdf](https://flightsafety.org/wp-content/uploads/2016/09/alar_bn7-1stablizedappr.pdf)) | Stable by 1000 ft (IMC) / 500 ft (VMC); Vref to Vref+20; sink ≤ 1000 fpm; ILS within 1 dot; circling wings level by 300 ft; unstable below the gate → go around | ✔ |
| S6 | EAFDM, *Developing standardised FDM-based indicators* v2, Ed. 2017 ([pdf](https://www.easa.europa.eu/sites/default/files/dfu/EAFDM__standardised_FDM-based_indicators_v2_Ed2017.pdf)) | RE.4: window H1 = 50 ft to H2 ≥ 150 ft, 1000 fpm, roll 7° or 10°. RE.6: hard landing by the manufacturer's criteria, monitored below them. RE.7: lateral deviation × ground speed. RE.8: required deceleration a = GS²/(2d) and delay of the deceleration means | ✔ |
| S7 | EOFDM WGB, *Guidance for the Implementation of FDM Precursors*, Rev 02, 2019, RE28–RE30 ([pdf](https://www.easa.europa.eu/sites/default/files/dfu/Study_WGB_Precursors_20191021.pdf)) | Long flare = time from 50 ft AAL to touchdown; deep landing = threshold-to-touchdown air distance (thresholds configurable); bounce = main-gear air→ground count > 1 | ✔ |
| S8 | FAA AC 91-79B (28 Aug 2023), §3.9, §5.2.2–5.2.4, §5.2.8 ([pdf](https://www.faa.gov/documentLibrary/media/Advisory_Circular/AC_91-79B_FAA.pdf)) | AFM air distance 1500 ft or 7 s (1000 ft with mitigations); 50 ft threshold height, +200 ft landing distance per 10 ft above it; +10% approach speed gives +20% landing distance; 1 s between deceleration actions in AFM data, +200 ft per second beyond 2 s | ✔ |
| S9 | Boeing 737 NG FCTM Rev 21 (30 Jun 2022), ch. 6 ([pdf](https://p7aviation.com/LMS/wp-content/uploads/2023/03/B737-NG-FCTM_Rev_21.pdf)) | Minimum command speed Vref+5; wind additive ½ steady headwind + full gust, approach speed ≤ Vref+15; touchdown no lower than Vref−5, typically Vref+0, keep the gust correction to touchdown; Vref−10 seriously reduces tail clearance; touchdown at least 1000 ft down the runway; flare 4–8 s; "smooth touchdown is not the criterion for a safe landing"; reverse thrust immediately at main-gear touchdown; touchdown in a crab is an accepted technique | ✔ |
| S10 | 14 CFR 25.473 ([text](https://www.law.cornell.edu/cfr/text/14/25.473)) | Limit descent velocity 10 ft/s (600 fpm) at design landing weight, 6 ft/s (360 fpm) at design take-off weight. EASA CS 25.473 is quoted with the same values but was not re-read | ✔ / ⚠ CS |
| S11 | Cessna 172S POH, Section 4 (172SPHBUS-00) ([pdf](https://aerocadet.com/cbts/pdf/172S-POH.pdf)) | Normal approach 65–75 KIAS flaps up, 60–70 KIAS flaps FULL; short field 61 KIAS; main wheels first; maximum demonstrated crosswind 15 kt | ✔ |
| S12 | ATSB AO-2023-010, quoting the 737 AMM (already in `profiles/B738.yaml`) | Hard landing > 2.2 g, valid for roll ≤ 2° | ⚠ |
| S13 | EOFDM operator examples (high pitch A320 9°, bank below 20 ft 8°); EOFDM *Breaking the Silos* long landing 750/900/1050 m; AAIB A319 2.6 g / 9 ft/s; CAP 739 windows; EASA Part-FCL App. 9 half-scale deflection | Background only; nothing below depends on them unless marked ⚠ | ⚠ |
| S14 | Annex 14 aiming point (`runway.rs`), FAA P/CG touchdown zone 3000 ft (`long-landing` rule) | Aiming point and TDZ (already in the code) | ⚠ not re-read |

## 2. Domains, criteria and bands

### 2.1 Scale

Each criterion turns its measured value `x` into a **continuous level ℓ**
from 0 to 3, by linear interpolation through five anchors:

| Anchor | ℓ | Meaning |
|---|---|---|
| ideal | 0 | Inside the ideal band (no deduction) |
| tolerance | 0.5 | Checkride tolerance edge (ACS / SOP) |
| L1 | 1 | First FDM threshold, or the first `low` event |
| L2 | 2 | Material exceedance (`medium`) |
| L3 | 3 | Critical / limit (`high`); held at 3 beyond it |

Where an anchor does not exist (e.g. D4S has no L1 below 500 ft),
interpolation runs straight to the next one. The level shown to the pilot
is `floor(ℓ)`. Level 0 is split into "ideal" (ℓ = 0) and "within
tolerance" (ℓ ≤ 0.5); 0.5 < ℓ < 1 reads "outside checkride tolerance".
Values that are held over time (approach) use the **sustained worst**: the
largest value held for the criterion's persistence (S1: 3 s, or 5 s for the
first descent threshold and for path deviation). This is the rule engine's
`sustain` semantics.

The **loss** of a criterion is a fraction of its domain's weight:

| ℓ | 0 | 0.5 | 1 | 2 | 3 |
|---|---|---|---|---|---|
| loss | 0 | 0.10 | 0.35 | 0.65 | 1.00 |

(linear in between). A landing entirely within checkride tolerance scores
at least 90. A first FDM threshold costs about a third of the domain.

**Domain loss = max over its criteria and its findings** (§3). The worst
item counts and the others are shown, as in an FDM event review.

```
raw   = 100 − Σ_d W_d · loss_d
score = min(raw, caps)            (§4), to 0.1
grade = Good ≥ 85 · Fair 60–84.9 · Poor < 60
```

### 2.2 Classes

`rules::Aircraft` knows `light | transport`. Scoring needs three classes:

| Scoring class | Profiles today | Benchmark |
|---|---|---|
| **light** | C172, SR22, BE36, BE58, DA62, C208, PC12, TBM9 | POH + Private/Commercial ACS |
| **turboprop** (transport-category) | AT76, SF34 | Type AFM/FCTM + ATP ACS + D4S |
| **jet** | A20N, A21N, A310, A333, B38M, B738, B748, B78X, C25C, C700, PC24, SF50 | Type FCTM + ATP ACS + D4S |

A new optional profile field `scoring_class` is added. Without it, `light` →
light and `transport` → jet. In the tables below, **T/J** means turboprop
and jet share the band.

### 2.3 Weights

| Domain | W | Why this weight |
|---|---|---|
| A Approach stability (1000/500 → 50 ft) | 25 | Unstable approaches are the leading runway-excursion precursor (S5, S1) |
| B Threshold and flare energy | 10 | Sets landing distance (S8). Partly overlaps A and C, so it is kept small |
| C Touchdown point | 20 | ACS core tolerance (S2–S4), deep landing (S7) |
| D Lateral control | 15 | ACS centreline/drift (S2–S4), RE.7 (S6) |
| E Touchdown contact (g, sink, attitude, bounce) | 20 | RE.5/RE.6 (S6), structural limits (S10, S12) |
| F Rollout and stopping | 10 | RE.8 (S6), deceleration timing (S8, S9) |

Touchdown point, touchdown load and centreline are three of the six
domains, worth 55 points together. Each is visible as its own bar.

### 2.4 Inputs: Vref and Vapp

See §5. Here `Vref` is the type's reference speed and `Vapp` is the
target approach speed:

- jet/turboprop: `Vapp = Vref + clamp(½ headwind + gust, 5, 15)` (S9; with
  the autothrottle engaged, Vref+5).
- light: `Vapp` = the middle of the POH final-approach range, plus ½ gust
  (S3/S4 "gust factor applied"; the ½ convention is ⚠ FAA AFH, not
  re-read).

Headwind and gust come from `wind.speed`/`wind.dir` over the last 1000 ft,
resolved on the runway (`analyzer::wind_components`).

### 2.5 Criteria

Columns: *how* = the computation, with **new** marking a derived value
that does not exist today. The bands are `ideal | tol | L1 | L2 | L3`.

#### A. Approach stability (W 25)

Window: T/J from 1000 ft to 50 ft above the runway (S1). Light from 500 ft
(S5 VMC gate) to `flare_ft` (20 ft); roll from 300 ft (S5 circling). A
challenge whose gate lies below the window start uses the gate instead.
T/J bands are split at 500 ft, as S1 does.

| Id | Measured | How | Class | ideal \| tol \| L1 \| L2 \| L3 | Standard |
|---|---|---|---|---|---|
| A1 | IAS − target | `ias` vs Vapp (tol) and Vref (L1+), sustained 3 s | T/J ≥ 500 ft | Vapp±2.5 \| Vapp±5 \| >Vref+20 or <Vref−5 \| >Vref+35 or <Vref−10 \| – | S2 tol, S1 L1/L2 ✔ |
| | | | T/J < 500 ft | Vapp±2.5 \| Vapp±5 \| – \| >Vref+20 or <Vref−5 \| >Vref+35 or <Vref−10 | S1 SL2B/SL3 ✔ |
| | | | light | Vapp +5/−2.5 \| +10/−5 \| +15/−7.5 \| +20/−10 \| +30/−15 | S3/S4 ideal/tol ✔; L1–L3 est |
| A2 | Descent rate | `−vs`, sustained 3 s (L1: 5 s) | T/J ≥ 500 | ≤ ROD₃°+200 \| 1000 \| 1200 \| 1500 \| – | S5/S6 tol ✔, S1 ✔; ideal est |
| | | ROD₃° = 5.31 × GS kt (**new**, trivial) | T/J < 500 | ≤ ROD₃°+200 \| 1000 \| – \| 1200 \| 1500 | S1 ✔ |
| | | | light | ≤ ROD₃°+200 \| 1000 \| 1200 \| 1500 \| 2000 | S5 tol ✔; levels est |
| A3 | \|Roll\| | `roll`, sustained 3 s | T/J ≥ 500 | 3.5 \| 7 \| 15 \| 30 \| – | S6 tol, S1 ✔ |
| | | | T/J < 500 | 3.5 \| 7 \| – \| 15 \| 30 | S1 ✔ |
| | | | light < 300 ft | 5 \| 10 \| 15 \| 20 \| 30 | S5 (wings level by 300 ft) ✔; bands est |
| A4 | Pitch | `pitch`, sustained 3 s | T/J | – \| – \| >10 or <−3 \| >15 or <−10 (≥ 500) / L2–L3 below | S1 ✔ |
| A5 | ILS deviation | `nav.gs_dev`, `nav.loc_dev`, 5 s, 1000–500 ft; below 500 ft the existing gate rules | all with a valid ILS | 0.25 \| 0.5 \| 1 dot \| GS 2 / LOC 1.5 \| – | S5 1 dot ✔, S1 ✔; tol ⚠ App. 9 half scale |
| A6 | Configuration | gear change/not down; `flap.te_angle` change > 2° (**new**: config change in window) | T/J | change 1000–500 → L2; below 500 or gear not down → L3 | S1 ✔ (single-threshold criteria use SL2A/SL3) |
| | | | light | flap change < 200 ft → L1; retractable gear not down < 300 ft → L3 | est; POH flaps "as desired" ✔ |
| A7 | Low thrust | all `engN.throttle` at idle > 10 s in window (**new**) | T/J | → L1 | S1 defines it per fleet percentile; ours est. Later slice |

Rule set v1's `unstable-speed-500` used `sel.speed` as the Vapp proxy at a
single 500 ft instant. v2 replaces it with `unstable-speed-above-500`,
`unstable-speed-below-500` (against `profile.vref`) and
`unstable-speed-light` (against `profile.vapp`), held 3 s.

**New derived value: `stabilisation`**, per approach: each criterion's
sustained worst and level, by height band, and the **stable height**, the
lowest height above which every A criterion stayed at ℓ ≤ 0.5. The debrief
shows it as "Stable from 1000 ft" or "Not stable until 380 ft".

#### B. Threshold and flare energy (W 10)

| Id | Measured | How | Class | ideal \| tol \| L1 \| L2 \| L3 | Standard |
|---|---|---|---|---|---|
| B1 | Threshold crossing height (TCH) | **new**: height above the threshold (RA, else AAL against the threshold elevation) interpolated at `along_m = 0` on the approach | all | 45–55 \| 40–65 \| >75 or <30 \| >100 or <20 \| >150 or <10 ft | S8: 50 ft standard, +200 ft per 10 ft ✔; bands est |
| B2 | Touchdown IAS | `td.ias_kt` (exists, unused) vs Vref | T/J | Vref −2/+3 \| ±5 (+gust) \| +10/−7 \| +15/−10 \| +25/−15 | S9 ≥ Vref−5, Vref−10 tail clearance ✔; L1, L3 est |
| | | vs Vapp | light | ≤ Vapp−10 \| ≤ Vapp−5 \| > Vapp \| > Vapp+10 \| > Vapp+20 | POH main wheels first ✔; bands est |
| B3 | Float time | **new**: time from 50 ft above the runway to main-gear contact | T/J | 8 \| 10 \| 12 \| 15 \| 20 s | S7 RE28 measure ✔; S9 flare 4–8 s ✔; bands est |
| | | | light | 10 \| 12 \| 15 \| 20 \| 25 s | est |

Findings here (§3): stall warning, flap/gear/VMO overspeed inside the
window.

#### C. Touchdown point (W 20)

`x = fix.along_m − target`. Target: T/J use the aiming-point marking
(`fix.aiming_point_m`). Light uses the challenge's `target_m` if set, else
the aiming point (open question 2). TDZ end = `fix.tdz_m`.

| Id | Side | Class | ideal \| tol \| L1 \| L2 \| L3 | Standard |
|---|---|---|---|---|---|
| C1 | long | T/J | +250 ft \| +500 ft \| +1000 ft \| beyond TDZ end \| beyond LDA/2 | S2 ✔ tol; S7 deep landing (configurable) ✔; `long-landing` rule (L2); L1, L3 est |
| | short | T/J | −125 ft \| −250 ft \| −500 ft \| < 200 ft past threshold \| before threshold | S2 ✔; S9 "at least 1000 ft" ✔; `short-landing` (L3) |
| | unmarked runway | T/J | 1000–1250 ft \| 750–1500 ft from threshold \| … | S2 ✔ |
| | long | light | +200 ft \| +400 ft \| +800 ft \| beyond TDZ end \| beyond LDA/2 | S4 ideal, S3 tol ✔; rest est |
| | short | light | 0 \| −100 ft \| −200 ft \| < 100 ft past threshold \| before threshold | S3/S4 "on or beyond the point" ✔; rest est |

Anchors never exceed the next one. On a short runway, L1 is
`min(target + 1000 ft, TDZ end)`. The landing-1 rule "0 outside the TDZ"
goes away: outside the TDZ is level 2, not zero.

#### D. Lateral control (W 15)

| Id | Measured | How | Class | ideal \| tol \| L1 \| L2 \| L3 | Standard |
|---|---|---|---|---|---|
| D1 | Centreline offset at touchdown | `fix.cross_m` | all | ½ tol \| ½ main-gear track \| 7.6 m \| 12.2 m \| 18.3 m, or outer main gear within 3 m of the edge | S2 "centreline between the main gear" ✔; L1–L3 = `off-centreline-touchdown` 25/40/60 ft est |
| D2 | Crab at touchdown | **new**: \|true heading − runway true heading\| at contact | light | 1° \| 3° \| 5° \| 8° \| 12° | S3/S4 "axis aligned" ✔; numbers est |
| | | | T/J | 2° \| 5° \| 7° \| 10° \| 15° | S9: crab touchdown accepted ✔; numbers est |
| D3 | Drift at touchdown | **new**: GS · sin(track − runway), track over the 1 s before contact | all | 1 \| 2 \| 4 \| 6 \| 8 kt | S3/S4 "no side drift" ✔; numbers est |
| D4 | Rollout centreline | max \|cross\| from touchdown to 30 kt (exists: `rollout_cross_m`) | all | ½ track \| 3 m \| 7.6 m \| 12.2 m \| runway exit → cap | S6 RE.7 ✔ (measure); numbers est |

Main-gear track: new profile field `main_gear_track` (with its source).
Fallbacks, all est: light 2.5 m, turboprop 4 m, jet 6 m.

#### E. Touchdown contact (W 20)

| Id | Measured | How | Class | ideal \| tol \| L1 \| L2 \| L3 | Standard |
|---|---|---|---|---|---|
| E1 | Peak normal load | `td.peak_g` | T/J | 1.3 \| 1.5 \| 1.6 \| 1.8 \| profile `hard_landing_g`, else 2.1 g | S6 RE.6 (monitor below the manufacturer's limit) ✔; S12 ⚠; levels = `hard-landing` rule est |
| | | | light | 1.3 \| 1.6 \| 1.8 \| 2.2 \| 2.6 g | `hard-landing-light` est |
| E2 | Sink at touchdown | `td.sink_fpm` (no lower bound: soft is not rewarded, S9 ✔) | T/J | 240 \| 360 \| 360+ \| 480 \| 600 fpm | S10 6 / 10 ft/s ✔; 240 ⚠ Boeing service experience; L2 est |
| | | | light | 240 \| 300 \| 400 \| 500 \| 700 fpm | est (CS-23 not checked) |
| E3 | Bounce | next touchdown has `bounce` | all | – \| – \| – \| bounce \| bounce airborne > 2 s or > 5 ft | S7 RE30 count > 1 ✔; L3 est |
| E4 | Pitch at touchdown | tail margin = `tail_contact.landing − td.pitch`; nose-first = pitch < 0° | T/J | – \| margin ≥ 2° \| – \| < 2° or pitch < 0° \| < 1° | `tailstrike-risk-landing` (EOFDM 2° ⚠); nose-first ⚠ EOFDM example |
| | | | light | – \| ≥ 0° \| – \| < 0° \| – | POH main wheels first ✔; threshold est |
| E5 | \|Roll\| at touchdown | `td.roll_deg` | T/J | 2° \| 3° \| 4° \| 6° \| 8° | S12 roll ≤ 2° ⚠; 8° ⚠ EOFDM example; rest est |
| | | | light | 3° \| 5° \| 7° \| 10° \| 15° | est |

E1 and E2 measure the same impact. The domain takes the worse of the two,
so a hard landing is not counted twice.

#### F. Rollout and stopping (W 10)

| Id | Measured | How | Class | ideal \| tol \| L1 \| L2 \| L3 | Standard |
|---|---|---|---|---|---|
| F1 | Deceleration delay | **new**: main-gear contact → first of reverser unlocked (`engN.reverser`), ground spoilers > 50% (`spoiler.handle`), brakes > 30% | T/J | 1 \| 2 \| 4 \| 6 \| 10 s | S8: 1 s per action, +200 ft per s beyond 2 s ✔; S9 immediate reverse ✔; L1–L3 est |
| | | | light | n/a by design (POH: "braking minimum required" ✔) | |
| F2 | Required deceleration | **new**: a = GS² / (2 · (LDA − along)) at touchdown, in g | all | 0.10 \| 0.15 \| 0.20 \| 0.30 \| 0.40 g | S6 RE.8 formula ✔; bands est |
| F3 | Stopped on the runway | **new**: runway remaining when GS ≤ 20 kt (S2 taxi speed ✔) | all | ≥ 0 → ok; overrun or exit → excursion cap | S2 ✔ |

Reversers and brakes are recorded at 1 Hz and spoilers at 2 Hz, so F1 is
known to ±1 s. Its anchors are wide for that reason.

## 3. Findings (rule events) in the score

Each rule gets a new mandatory field **`domain:`**
(`stability | energy | point | lateral | contact | rollout | other`). The
rule loader rejects a rule without one, so every rule belongs to exactly
one domain. A unit test checks that every rule in `core.yaml` carries a
domain. The core set maps as follows:

| Domain | Rules |
|---|---|
| A stability | unstable-speed-above-500, unstable-speed-below-500, unstable-speed-light, high-sink-above-500, high-sink-below-500, high-sink-light, bank-above-500, bank-below-500, bank-light, localizer-deviation-above-500, glideslope-deviation-above-500, localizer-deviation-500, glideslope-deviation-500, gear-not-down-500, unstable-sink-500 and unstable-bank-500 (advisory gate notes) |
| B energy | stall-warning, flap-overspeed, vmo-exceedance, gear-extended-overspeed, gear-operating-overspeed |
| C point | long-landing, short-landing |
| D lateral | off-centreline-touchdown, crosswind-above-demonstrated |
| E contact | hard-landing, hard-landing-light, high-sink-touchdown, high-sink-touchdown-light, bounced-landing, tailstrike-risk-landing, hard-landing-inspection |
| F rollout | tailwind-landing-limit |
| other (never in a landing score) | tailstrike-risk-takeoff, go-around (advisory) |

How a finding is scored:

- An event counts toward a landing when its start lies in that landing's
  window: from the approach start (or the challenge gate) to the end of
  the rollout. The same rule outside the window, e.g. a flap overspeed in
  the climb, goes to the rest-of-flight list.
- Levels map directly: low → ℓ 1, medium → ℓ 2, high → ℓ 3; advisory → no
  deduction. Within a level, ℓ is not refined by the event's value: the
  event value already sits at the criterion's anchor.
- The **domain loss** is the maximum over its criteria and its findings.
  Several findings in one domain do not pile up; findings in different
  domains do.
- **The thresholds have one home.** The L1–L3 anchors of a criterion that
  has a rule (A2 ↔ unstable-sink-500 / high-sink-below-1000, E1 ↔
  hard-landing, D1 ↔ off-centreline-touchdown, …) are taken from that rule,
  through `levels_from: <rule-id>` in the scoring definition. landing-2 adds
  only the ideal and tolerance anchors. As part of this work the rule set
  moves to v2 with thresholds equal to §2.5 (e.g. `unstable-speed-500`
  against Vref, D4S height split as two rules ≥ 500 / < 500 ft).
- **The event cards are the explanation.** Each domain bar opens to show
  its criteria (value, band strip, standard) and its event cards. The card
  that set the domain's loss is marked "sets this deduction", with the
  points it cost.

**Relation to today's assessment** (`Assessment::of`: high 4 / medium 2 /
low 1; Good ≤ 1, Fair ≤ 5, Poor ≥ 6). landing-2 replaces it for every
flight with a scoreable landing. The grade bands were chosen to agree with
it:

| Today | Points | landing-2 | Typical score |
|---|---|---|---|
| Nothing or one low | 0–1 | One L1 in any domain | 91–97 → Good |
| Two lows or one medium | 2 | L2 caps the score at 84.9 (§4) | ≤ 84.9 → Fair |
| One high | 4 | L3: domain lost and capped at 84.9 | 75–84.9 → Fair |
| High + medium, or two highs | 6–8 | Two domains at L2–L3 | 45–70 → Fair/Poor |

During one release, `Assessment.points` stays in the wire as `legacy_points`.
A flight without a scoreable landing (ended airborne, or insufficient data)
keeps the points method for its grade.

**Rest of the flight.** Findings outside every landing window (taxi,
takeoff, cruise) are listed under "Other phases". They do not change the
landing score. They can only lower the flight's grade: one high gives at
most Fair; two highs give Poor (today's semantics).
**Flight grade** = the worst of every landing's grade and the rest-of-flight
grade (open question 3).

## 4. Safety caps, go-arounds, insufficient data

The caps apply after the raw score. Each cap is shown as a banner with the
event that caused it.

| Condition | Cap | Basis |
|---|---|---|
| Any criterion or finding at level ≥ 2 | 84.9 (cannot be Good) | A material FDM event; same as today's grades |
| **Continued unstable approach**: T/J by the D4S rule (≥ 1 A criterion at SL2B/SL3, i.e. L2/L3 below 500 ft, or ≥ 3 distinct A criteria at L1/L2 above 500 ft) without a go-around; light: any A criterion at L2+ below 300 ft, or ≥ 3 at L1+ | 59 (Poor) | S1 ✔, S5 "go around" ✔; light adaptation est |
| **Inspection-level contact**: `hard-landing-inspection` (peak g > the manufacturer's `hard_landing_g`, Decision 5; no separate sink cap), or tail margin < 0 (tail strike) | 39 | S10 ✔, S12 ⚠ |
| **Undershoot**: touchdown before the threshold | 39 | `short-landing` |
| **Runway excursion or crash**: left the runway sides before 30 kt, overran (F3), sim crash/damage flag | 0, outcome shown | S6 RE.7/RE.8 |

**Go-around.**

- Approach then go-around: the approach is assessed on domains A and B
  only, without a score. The card reads "Go-around: correct decision" when
  the approach was unstable by the cap rule above, "Go-around" otherwise.
  It is never a deduction, because going around is the trained response
  (S3/S4 S11 ✔, S5 ✔). Findings during the go-around itself (stall
  warning, overspeed, bank) go to the rest of the flight.
- In a challenge, an attempt with a go-around is not ranked (no landing).
  The pilot can fly again; the attempt is kept as "go-around" in the
  history.
- A touch-and-go is scored A–E. F is n/a by design, and its weight is
  redistributed (below).

**Insufficient data.** Missing data never earns full credit.

- **Required** for any score: a runway fix (`td.runway`), `peak_g`, a sink
  rate and position on the rollout. Without them the landing reads
  "insufficient data": no score, findings only, legacy grade. In a
  challenge the attempt is invalid with check `data`.
- **Optional** criteria (A5 without an ILS, F1 for a light aircraft, D2
  without a true heading, A7 before its slice) are **n/a by design**. Their
  domain is scored on what remains. If a whole domain is n/a, its weight
  is redistributed proportionally: `score = 100 − Σ W·loss × 100 / ΣW_avail`.
- **Missing but expected** data (a gap over the window > 1.5 s, no Vref
  source, RA invalid at the threshold): the criterion is n/a and the result
  is marked `confidence: partial`. The debrief shows the score with a
  "partial" badge. A challenge requires `full` to rank. Vref only counts
  as missing when §5 finds no source at all.

## 5. Vref

D4S (S1, p. 20 ✔) uses Vref in this order: a recorded Vref; a table by gross
weight and flap; a table by gross weight at a fixed landing flap.
landing-2 does the same:

1. **Profile table by weight and flap** (new profile field; source
   required, as for every profile value):
   ```yaml
   vref:
     source: "Boeing 737-800 FCOM/QRH PI, Vref table"
     flaps:
       - { flap: 30 deg, weights_kg: [40000, 50000, 60000, 70000], ias: [...] }
       - { flap: 40 deg, weights_kg: [...], ias: [...] }
   ```
   Lookup: `weight.gross` at the start of the window (sustained 1000 ft),
   landing flap = `flap.te_angle` at 500 ft, on the nearest flap entry,
   linear in weight and clamped at the ends. The result is marked
   approximate outside the table (weight, or a flap angle it lacks).
2. **Profile fixed speeds** for light aircraft:
   `approach_speed: { source, flaps: [{ flap: 0 deg, ias: [65 kt, 75 kt] },
   { flap: 30 deg, ias: [60 kt, 70 kt] }] }` (C172 from S11 ✔), read on
   the nearest flap angle. Vapp = the middle of the range, Vref = its lower end.
   `vso` is used when no published approach speed exists:
   Vapp = 1.3 VSO (S3/S4 ✔).
3. **Recorded selected speed** (`sel.speed`, held over the window, debrief
   only). It is treated as Vapp, and Vref = Vapp − 5 for T/J. It is never
   used in a challenge: the pilot sets it and could widen their own bands.
4. **No source**: A1 and B2 are n/a, `confidence: partial`, and the
   debrief says "No Vref for <type>". A challenge for that aircraft is
   rejected when it is defined: `Challenge::parse` requires a type with a
   Vref table or approach speeds in its aircraft list.

To source next: Vref tables for B738/B38M, A20N/A21N (Airbus VLS/VAPP: the
FCOM logic differs, ⚠), AT76, and the light/bizjet types. Each table must
cite a primary document, as `profiles/` already requires.

## 6. Versioning, fairness, worked examples

**Versioning.** `scoring.version` in a challenge file selects `landing-1` or
`landing-2`. The two live side by side: landing-1 is frozen code, and
landing-2 is a data file `scoring/landing-2.yaml` (anchors, weights,
caps, sources) built into `fdm` and checked on load, the way rules are.
Once a version is published its file never changes; a correction becomes
`landing-3`. The debrief always uses the newest version and states it
(`assessment.method`). The challenge server re-scores an uploaded slice
with the version the challenge names, so the leaderboard is unaffected
by app updates.

**Fairness.**

- Leaderboards are per **scoring class** at least. A challenge may narrow
  to types, as it does today. Bands differ by class, so a 90 in a C172
  and a 90 in a 737 should not share a ranking.
- Speed bands follow each pilot's own Vref from their recorded weight, so a
  heavy aircraft is not penalised for flying faster.
- Wind is measured; the challenge bands already gate it.
- Sim-detected values with sampling limits (g at 8 Hz versus a 16 Hz DFDR;
  1 Hz heading, reverser and brake) are the same for every entrant. They
  are documented as proxies.
- Ties: the continuous ℓ in the ideal-to-tolerance band separates good
  landings. A remaining tie goes to the earlier attempt (protocol).

### Worked example 1: C172, normal landing

Runway: LDA 1600 m, width 30 m, aiming point 300 m (Annex 14), TDZ 533 m.
Flaps full, Vapp 65 kt (S11 60–70 mid), calm. Light bands. Main-gear track
is the light fallback, half-track 1.25 m.

| Domain | Worst item | Value | ℓ | loss | − pts |
|---|---|---|---|---|---|
| A (25) | A1 speed at 250 ft (3 s) | +12 kt (tol +10, L1 +15) | 0.70 | 0.20 | 5.0 |
| | A2 sink | 820 fpm (ideal 529, tol 1000) | 0.31 | 0.06 | – |
| B (10) | B1 TCH | 62 ft (tol 65) | 0.35 | 0.07 | 0.7 |
| C (20) | C1 long | 470 m = target +558 ft (tol +400; L1 +800 ft = 544 m is past the TDZ end, so held at 533 m = +764 ft) | 0.72 | 0.21 | 4.2 |
| D (15) | D1 centreline | 2.4 m (tol 1.25, L1 7.6) | 0.59 | 0.15 | 2.2 |
| | D2 crab | 3° (tol 3°) | 0.50 | 0.10 | – |
| E (20) | E1 peak g | 1.52 g (tol 1.6) | 0.37 | 0.07 | 1.5 |
| | E2 sink | 260 fpm | 0.17 | 0.03 | – |
| F (10) | F2 required decel | 0.04 g; F1 n/a by design | 0 | 0 | 0 |

Score **86.5, Good**, with no FDM events. (An earlier draft read 86.7: it
left L1 at 544 m, past the TDZ end, against "anchors never exceed the next
one"; the golden test in `crates/assess/tests/score.rs` follows the rule.) The debrief still flags two items
as "outside checkride tolerance": +12 kt at 250 ft, and touchdown 558 ft
past the target, beyond the Private ACS 400 ft window.

### Worked example 2: B737-800, crosswind-free landing with a late descent

Runway: LDA 3000 m, width 45 m, aiming point 400 m, TDZ 914 m. Weight
62 t, flaps 30. Vref 141 kt is an *illustrative* value, because the B738
table is not yet sourced. Headwind 10 kt, no gust: additive
max(5, ½·10) = 5, so Vapp = 146. Autothrottle off. Jet bands, fallback
half-track 3 m.

| Domain | Worst item | Value | ℓ | loss | − pts |
|---|---|---|---|---|---|
| A (25) | A2 sink below 500 ft | 1100 fpm for 5 s at 400 ft (tol 1000; next anchor L2 1200) | 1.25 | 0.43 | 10.6 |
| | A1 speed above 500 ft | Vref+22 for 4 s at 750 ft (D4S 1st threshold, SL1) | 1.13 | 0.39 | – |
| B (10) | B3 float 50 ft → TD | 9 s (ideal 8, tol 10) | 0.25 | 0.05 | 0.5 |
| C (20) | C1 long | 650 m = aim +820 ft (tol +500, L1 +1000) | 0.82 | 0.26 | 5.2 |
| D (15) | D1 centreline | 4.1 m (tol 3, L1 7.6) | 0.62 | 0.16 | 2.4 |
| E (20) | E1 peak g | 1.48 g (tol 1.5) | 0.45 | 0.09 | 1.8 |
| F (10) | F1 reverser delay | 2.5 s (tol 2, L1 4) | 0.63 | 0.16 | 1.6 |

Rule set v2 raises no event for 1100 fpm below 500 ft (its first level
there is 1200 fpm), so no card; v1's `high-sink-below-1000` low card had
ℓ 1 and changed nothing. D4S check: one SL1 criterion, and nothing at
SL2B or SL3, so the approach
was not unstable and no cap applies. Score **77.9, Fair**. The deduction
story reads: high sink below 500 ft, a fast approach at 750 ft, and a
touchdown 820 ft past the aiming point.

Variant: the same landing with 1250 fpm held for 5 s below 500 ft. A2 is
then SL2B (L2), so the approach was unstable and was continued: cap 59,
**Poor**. The banner reads "Unstable below 500 ft (D4S), landing continued:
go around next time".

## 7. Implementation plan (small slices, each with tests)

Every slice keeps `cargo test` green on Windows (paths normalised with
`/`, no reliance on CRLF). Nothing touches the challenge server's stored
results.

1. **Profile fields** (`crates/rules` profile loader, `docs/rules.md`):
   `scoring_class`, `main_gear_track`, `vso`, `approach_speed`, `vref`
   table, each with a source. Tests: parsing, monotonic weights, a source
   is required, interpolation/clamp in the table lookup. Data: C172
   (S11). Others only as sources are found.
2. **Analyzer derived values** (`crates/analyzer`, new `approach.rs`
   beside `landing.rs`): threshold crossing (time, height, IAS, GS), float
   time 50 ft → TD, crab and drift at contact (true heading from
   `heading.mag` less the airport's recorded magnetic variation, so no
   LFL change),
   window series with sustained-worst per criterion and the stable height,
   config-change events, deceleration delay, required deceleration, GS ≤
   20 kt position. Tests: synthetic tracks from `crates/testkit` for each
   value, plus the `c172-pattern.jsonl` fixture as a regression snapshot.
3. **Rule set v2** (`rules/core.yaml`, `docs/rules.md`): a mandatory
   `domain:`, D4S height split, `unstable-speed` against Vref
   (`profile.vref`), thresholds equal to §2.5. Tests: every rule has a
   domain (load-time), existing rule tests updated, one new test per
   changed threshold.
4. **Scoring module** (new crate `crates/assess`, used by `debrief` and
   `challenge`): `scoring/landing-2.yaml` (anchors, `levels_from`,
   weights, caps, sources), ℓ interpolation, loss curve, domain max,
   caps, n/a redistribution, confidence, Vref resolution. Tests: the
   anchor/loss tables, each cap, the D4S unstable rule (3 × SL1 versus
   1 × SL2B), n/a redistribution, and the two worked examples above as
   golden tests.
5. **Debrief wire** (`crates/debrief/src/wire.rs`, `docs/debrief-api.md`):
   `Assessment { method, score, grade, confidence, caps[], landings[{ t,
   score, grade, domains[{ id, weight, loss, level, criteria[{ id, value,
   unit, ell, level, bands, standard }], findings[event ids], sets_by }] }],
   other_findings[], legacy_points }`. Tests: wire snapshot, and the grade
   bands agreeing with today's for single findings (the table in §3).
6. **Challenge** (`crates/challenge`): `landing-2` accepted by
   `Challenge::parse` (requires a Vref source for the allowed types);
   `evaluate` calls the shared module; check `data`; go-around attempts
   not ranked; boards keyed by scoring class. Tests: landing-1 results
   unchanged (frozen), landing-2 for the fixtures, parse rejections.
7. **UI** (`app/src/components/debrief`, `msfs-panel/efb`): the debrief
   header shows score, grade, confidence and any cap banner. The
   assessment panel shows six horizontal domain bars (weight as the track,
   loss shaded, level colour), each expanding to its criteria (value on a
   band strip ideal/tol/L1–L3, with the standard) and its event cards.
   The card that set the deduction is marked. EFB/panel: score, grade, six
   mini-bars, the top deduction in one line. Update
   `app/src/lib/references.ts` to the same bands, so the display and the
   score cannot disagree. Tests: component tests for the bar math and an
   n/a domain; a browser recording of the panel per the visual-check rule.
8. **Docs**: `docs/challenges.md` (scoring section → this file),
   `docs/rules.md` (`domain:`, profile fields), `docs/debrief-api.md`.

## Decisions (panel vote, 2026-10-27)

Decided by a panel (Claude Sonnet as FDM analyst/instructor, Claude Haiku
as a competitive sim pilot, Codex) under Ege's standing instruction.

1. **Vref (3 of 3):** FCOM/QRH Vref tables in the type profiles for
   transport types (737-800 first), POH approach speeds for light types.
   The recorded selected speed is only a labelled fallback in the
   per-flight debrief and never counts for a ranked challenge; a type with
   no sourced Vref cannot be ranked on speed (n/a, as in §5).
2. **Touchdown target (2 of 3):** the runway's aiming-point marking by
   default; a challenge may specify another point (ACS "specified point",
   e.g. the 1000 ft markers). The resolved target and tolerance are
   published in the challenge and shown before the attempt.
3. **Flight grade (2 of 3):** the last full-stop landing sets the flight
   grade; touch-and-goes are scored per landing but do not set it. High
   findings elsewhere in the flight, and the safety outcome of any earlier
   landing (cap level), still cap the flight grade, and a banner says why
   when the flight grade differs from a landing's score.
4. **Calibration before ranking:** anchors marked "est" are checked
   against recorded flights (and public FDM/ACS data where available)
   before any ranked challenge; the list of estimated anchors is
   published with the method.
5. **Inspection caps** use the manufacturer's hard-landing criteria from
   the type profile (`hard_landing_g`), not the certification design
   descent velocity alone; without a manufacturer value the contact
   domain grades the landing but no inspection cap applies.
