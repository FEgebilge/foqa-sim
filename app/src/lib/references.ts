// Sourced context for the landing card and charts, display only (events come from the rules, type limits from the profile).
// A "reference" is published (manufacturer, regulator, ICAO/FAA); a "sim comfort band" is a flight-sim community convention.
import type { Debrief, RunwayFix } from "../types";
import { M_PER_FT, number } from "./format";

type Flight = Debrief["flight"];
/** A sourced reference on a scale: a point, or a range when `to` is set. */
export interface Mark { at: number; to?: number; label: string; source?: string }
/** Sim comfort bands: each covers values up to `to` (inclusive), the last one ends at Infinity. */
interface Comfort { bands: { to: number; label: string }[]; source: string }
export interface Scale { min: number; max: number; marks: Mark[]; comfort?: Comfort }

const SRC = {
  flare: "FLARE light-single landing scoring curve (flight-sim community convention, not a Cessna limit), https://flaresim.io/help/",
  loadflight: "Load Flight landing ratings (flight-sim community convention), https://loadflight.com/documentation.php",
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

const comfort = (source: string, ...bands: [label: string, to: number][]): Comfort =>
  ({ source, bands: bands.map(([label, to]) => ({ label, to })) });

/** Touchdown sink rate, fpm downward: published figures for transport aircraft, sim comfort bands for light ones. */
export function sinkScale(flight: Flight): Scale | null {
  if (flight.class === "light") return { min: 0, max: 450, marks: [], comfort: comfort(SRC.flare, ["Smooth", 120], ["Normal", 210], ["Firm", 330], ["Hard", Infinity]) };
  return flight.class !== "transport" ? null : { min: 0, max: 720, marks: [
    { at: 120, to: 180, label: "Boeing typical touchdown, 120-180 fpm", source: SRC.boeingTypical },
    { at: 240, label: "Crews often report a hard landing above 240 fpm", source: SRC.boeingReport },
    { at: 600, label: "CS 25 design landing sink, 600 fpm", source: SRC.cs25 },
  ] };
}

/** The type's hard-landing inspection load, from its profile. */
export function hardLanding(flight: Flight): Mark | null {
  const hard = flight.profile_limits?.hard_landing_g;
  return hard ? { at: hard.value, label: `${hard.value} g: hard-landing inspection (type profile)`, source: hard.source } : null;
}

/** Peak normal load at touchdown, total g (1 g in level flight): the type's hard-landing load and the class's sim comfort bands. */
export function gScale(flight: Flight): Scale | null {
  const hard = hardLanding(flight);
  const bands = flight.class === "light" ? comfort(SRC.flare, ["Smooth", 1.1], ["Normal", 1.35], ["Firm", 1.75], ["Hard", Infinity])
    : flight.class === "transport" ? comfort(SRC.loadflight, ["Smooth", 1.1], ["Normal", 1.4], ["Firm", Infinity]) : undefined;
  return hard || bands ? { min: 1, max: Math.max(flight.class === "light" ? 2 : 2.5, (hard?.at ?? 0) + 0.3), marks: hard ? [hard] : [], comfort: bands } : null;
}

/** Pitch at touchdown: the transport flare attitude and the type's tail contact pitch. */
export function pitchScale(flight: Flight): Scale | null {
  const tail = flight.profile_limits?.tail_landing_pitch, marks: Mark[] = [];
  if (flight.class === "transport") marks.push({ at: 4, to: 7, label: "Boeing 4-7° nose up at touchdown", source: SRC.fctmPitch });
  if (tail) marks.push({ at: tail.value, label: `Tail contact at ${tail.value}° (type profile)`, source: tail.source });
  return marks.length ? { min: 0, max: Math.max(10, (tail?.value ?? 0) + 1), marks } : null;
}
export const LIGHT_PITCH = { text: "Target: main wheels first, nosewheel held off", source: SRC.poh };

/** Touchdown distance past the threshold; the touchdown zone and the aiming point come from the analyser. */
export function distanceScale(fix: RunwayFix, flight: Flight): Scale {
  const ap = fix.aiming_point_m, max = Math.min(fix.lda_m, Math.max(fix.tdz_m * 1.25, fix.along_m * 1.1));
  const marks: Mark[] = [
    { at: 0, to: fix.tdz_m, label: `Touchdown zone, first ${number(fix.tdz_m)} m`, source: SRC.faaTdz },
    { at: ap, label: `Aiming point ${ap} m`, source: SRC.annex14 },
  ];
  if (flight.class === "transport") {
    marks.push({ at: ap - 250 * M_PER_FT, to: ap + 500 * M_PER_FT, label: "Aiming-point window, -250 / +500 ft", source: SRC.atp },
      { at: 1500 * M_PER_FT, label: "1,500 ft landing-data assumption", source: SRC.ac9179 });
  }
  return { min: -0.06 * max, max, marks };
}

/** Chart reference lines. */
export const APPROACH_GATES: Mark[] = [
  { at: 1000, label: "1,000 ft gate: stable by here (IMC)", source: SRC.fsf },
  { at: 500, label: "500 ft gate: stable by here (VMC)", source: SRC.fsf },
];
export const SINK_LIMIT: Mark = { at: -1000, label: "-1,000 fpm: stabilised-approach sink limit", source: SRC.fsf };
