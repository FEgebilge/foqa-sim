// Sourced context for the landing card and charts, display only (events come from the rules, type limits from the profile).
// A "reference" is published (manufacturer, regulator, ICAO/FAA). The shaded bands under a gauge are the landing-2 bands of
// the criterion that grades that value (lib/assessment.ts), so the gauge and the score cannot disagree.
import type { CriterionAssessment, Debrief, RunwayFix, Sourced } from "../types";
import { ZONE_COLOR, ZONES, zones } from "./assessment";
import { M_PER_FT, number } from "./format";

type Flight = Debrief["flight"];
/** A sourced reference on a scale: a point, or a range when `to` is set. */
export interface Mark { at: number; to?: number; label: string; source?: string }
/** The score's bands on the gauge's axis: each covers values up to `to` (inclusive), the last one ends at Infinity. */
export interface ScoreBands { bands: { to: number; label: string; color: string }[]; source: string }
export interface Scale { min: number; max: number; marks: Mark[]; bands?: ScoreBands }

/** A criterion's zones as gauge bands; `map` takes its unit to the gauge's axis (increasing), e.g. ft past the target to m. */
export function scoreBands(c: CriterionAssessment | undefined, map: (x: number) => number = (x) => x): ScoreBands | undefined {
  if (!c) return undefined;
  const bands = zones(c.bands).map((z) => ({ to: z.to === Infinity ? Infinity : map(z.to), label: ZONES[z.zone], color: ZONE_COLOR[z.zone] }));
  return { bands, source: `landing-2 ${c.id}, ${c.title} (docs/scoring.md): ${c.standard}` };
}

/** The finite band edges, for sizing a scale around them. */
const edges = (b?: ScoreBands) => b?.bands.map((x) => x.to).filter(Number.isFinite) ?? [];

const SRC = {
  boeingTypical: "Boeing AERO Q3 2007: typical touchdown sink rate 2-3 ft/s (120-180 fpm), https://studylib.net/doc/26309989/aero-q307-article3",
  boeingReport: "Boeing service experience: crews often report a hard landing above about 4 ft/s (240 fpm), https://manuals.plus/m/0a9030e54ce9888ef7195abcd74eb5e15f82ffb5676ac4ca996ab3177c863feb",
  cs25: "EASA CS 25.473: 10 ft/s (600 fpm) design landing descent velocity at design landing weight. A structural design reference, not proof of damage.",
  fctmPitch: "Boeing 737NG FCTM, Landing Flare Profile: about 4-7 deg nose-up at touchdown, https://studylib.net/doc/27982068/737ng-fctm",
  annex14: "ICAO Annex 14, Table 5-1: aiming point marking 150 / 250 / 300 / 400 m from the threshold by landing distance available",
  faaTdz: "FAA Pilot/Controller Glossary: touchdown zone, the first 3,000 ft (914 m) of the runway",
  atp: "FAA ATP ACS AA.III.B.S10: touch down -250 / +500 ft from the aiming point markings, https://www.faa.gov/training_testing/testing/acs/atp_airplane_acs_11.pdf",
  ac9179: "FAA AC 91-79B 5.2.4: landing data commonly assumes touchdown 1,500 ft past the threshold",
  fsf: "Flight Safety Foundation stabilised-approach criteria: stable by 1,000 ft (IMC) / 500 ft (VMC), sink normally <= 1,000 fpm, https://flightsafety.org/wp-content/uploads/2016/12/asw_aug08.pdf",
  poh: "Cessna 172S POH: land on the main wheels first, then lower the nosewheel gently",
} as const;

/** A scale from `min` past the largest band edge, the marks and the value. */
function sized(min: number, value: number, bands: ScoreBands | undefined, marks: Mark[], floor: number): Scale {
  const top = Math.max(floor, value * 1.08, ...edges(bands).map((e) => e * 1.15), ...marks.map((m) => (m.to ?? m.at) * 1.1));
  return { min, max: top, marks, bands };
}

/** "(type profile)", or "(≈ type profile)" for an approximate value. */
const fromProfile = (s: Sourced) => s.approximate ? "(≈ type profile)" : "(type profile)";
const sourceOf = (s: Sourced) => s.approximate && s.note ? `${s.source}. Approximate: ${s.note}` : s.source;

/** Touchdown sink rate, fpm downward: the E2 bands, and published figures for transport aircraft. */
export function sinkScale(flight: Flight, sink: number, e2?: CriterionAssessment): Scale | null {
  const marks: Mark[] = flight.class !== "transport" ? [] : [
    { at: 120, to: 180, label: "Boeing typical touchdown, 120-180 fpm", source: SRC.boeingTypical },
    { at: 240, label: "Crews often report a hard landing above 240 fpm", source: SRC.boeingReport },
    { at: 600, label: "CS 25 design landing sink, 600 fpm", source: SRC.cs25 },
  ];
  const bands = scoreBands(e2);
  return bands || marks.length ? sized(0, sink, bands, marks, 450) : null;
}

/** The type's hard-landing inspection load, from its profile. */
export function hardLanding(flight: Flight): Mark | null {
  const hard = flight.profile_limits?.hard_landing_g;
  return hard ? { at: hard.value, label: `${hard.value} g: hard-landing inspection ${fromProfile(hard)}`, source: sourceOf(hard) } : null;
}

/** Peak normal load at touchdown, total g (1 g in level flight): the E1 bands and the type's hard-landing load. */
export function gScale(flight: Flight, g: number, e1?: CriterionAssessment): Scale | null {
  const hard = hardLanding(flight), bands = scoreBands(e1);
  if (!hard && !bands) return null;
  const top = Math.max(2, g + 0.1, (hard?.at ?? 0) + 0.3, ...edges(bands).map((e) => e + 0.2));
  return { min: 1, max: top, marks: hard ? [hard] : [], bands };
}

/** |Bank| at touchdown: the E5 bands. */
export function rollScale(roll: number, e5?: CriterionAssessment): Scale | null {
  const bands = scoreBands(e5);
  return bands ? sized(0, roll, bands, [], 6) : null;
}

/** Touchdown IAS against the reference speed (B2: Vref for transport classes, Vapp for light), in kt. */
export function speedScale(ias: number, reference: number, b2?: CriterionAssessment): Scale | null {
  const bands = scoreBands(b2, (x) => reference + x);
  if (!bands) return null;
  const finite = edges(bands);
  const lo = Math.min(ias, ...finite), hi = Math.max(ias, ...finite), pad = (hi - lo) * 0.1 || 5;
  return { min: lo - pad, max: hi + pad, marks: [{ at: reference, label: `Reference ${number(reference)} kt` }], bands };
}

/** Centreline offset at touchdown, m either side: the D1 bands (ft) in metres. */
export function centrelineScale(cross: number, widthM: number | undefined, d1?: CriterionAssessment): Scale | null {
  const bands = scoreBands(d1, (ft) => ft * M_PER_FT);
  if (!bands) return null;
  const marks: Mark[] = widthM ? [{ at: widthM / 2, label: `Runway edge ${number(widthM / 2, 1)} m from the centreline` }] : [];
  return sized(0, Math.abs(cross), bands, marks, 5);
}

/** Pitch at touchdown: the transport flare attitude and the type's tail contact pitch. */
export function pitchScale(flight: Flight): Scale | null {
  const tail = flight.profile_limits?.tail_landing_pitch, marks: Mark[] = [];
  if (flight.class === "transport") marks.push({ at: 4, to: 7, label: "Boeing 4-7° nose up at touchdown", source: SRC.fctmPitch });
  if (tail) marks.push({ at: tail.value, label: `Tail contact at ${tail.value}° ${fromProfile(tail)}`, source: sourceOf(tail) });
  return marks.length ? { min: 0, max: Math.max(10, (tail?.value ?? 0) + 1), marks } : null;
}
export const LIGHT_PITCH = { text: "Target: main wheels first, nosewheel held off", source: SRC.poh };

/** Touchdown distance past the threshold: the C1 bands (ft from the aiming point) in metres, the touchdown zone and aiming point. */
export function distanceScale(fix: RunwayFix, flight: Flight, c1?: CriterionAssessment): Scale {
  const ap = fix.aiming_point_m, max = Math.min(fix.lda_m, Math.max(fix.tdz_m * 1.25, fix.along_m * 1.1));
  const marks: Mark[] = [
    { at: 0, to: fix.tdz_m, label: `Touchdown zone, first ${number(fix.tdz_m)} m`, source: SRC.faaTdz },
    { at: ap, label: `Aiming point ${ap} m`, source: SRC.annex14 },
  ];
  if (flight.class === "transport") {
    marks.push({ at: ap - 250 * M_PER_FT, to: ap + 500 * M_PER_FT, label: "Aiming-point window, -250 / +500 ft", source: SRC.atp },
      { at: 1500 * M_PER_FT, label: "1,500 ft landing-data assumption", source: SRC.ac9179 });
  }
  return { min: -0.06 * max, max, marks, bands: scoreBands(c1, (ft) => ap + ft * M_PER_FT) };
}


/** Chart reference lines. */
export const APPROACH_GATES: Mark[] = [
  { at: 1000, label: "1,000 ft gate: stable by here (IMC)", source: SRC.fsf },
  { at: 500, label: "500 ft gate: stable by here (VMC)", source: SRC.fsf },
];
export const SINK_LIMIT: Mark = { at: -1000, label: "-1,000 fpm: stabilised-approach sink limit", source: SRC.fsf };
