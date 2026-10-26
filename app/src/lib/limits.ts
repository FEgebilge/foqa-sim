// Display bands and references for a landing, as data with their sources.
// Two kinds, never mixed up in the UI: a "reference" comes from a manufacturer, regulator or ICAO/FAA document;
// a "comfort" band is a flight-sim community convention. Neither is an airworthiness verdict.
import type { Debrief, FlightEvent, Level, PhaseSpan, RunwayFix, Touchdown } from "../types";

export type AircraftClass = "light" | "transport";
/** Better first. */
export type Grade = "good" | "ok" | "firm" | "review";
export type Provenance = "reference" | "comfort";

/** One band of a scale: values up to `to` (inclusive); the last band of a scale ends at Infinity. */
export interface Band { to: number; grade: Grade; label: string; kind: Provenance; source: string }
/** An official marker on a scale (a point, or a range when `to` is set). */
export interface Mark { at: number; to?: number; label: string; source: string }
export interface Scale { min: number; max: number; bands: Band[]; marks?: Mark[] }

export const GRADES: Grade[] = ["good", "ok", "firm", "review"];
export const worst = (a: Grade, b: Grade) => (GRADES.indexOf(a) >= GRADES.indexOf(b) ? a : b);
export const PROVENANCE_LABEL: Record<Provenance, string> = { reference: "reference", comfort: "sim comfort band" };

const SRC = {
  flare: "FLARE light-single landing scoring curve (flight-sim community convention, not a Cessna limit), https://flaresim.io/help/",
  loadflight: "Load Flight landing ratings (flight-sim community convention), https://loadflight.com/documentation.php",
  boeingTypical: "Boeing AERO Q3 2007: typical touchdown sink rate 2-3 ft/s (120-180 fpm), https://studylib.net/doc/26309989/aero-q307-article3",
  boeingReport: "Boeing service experience: crews often report a hard landing above about 4 ft/s (240 fpm), https://manuals.plus/m/0a9030e54ce9888ef7195abcd74eb5e15f82ffb5676ac4ca996ab3177c863feb",
  cs25: "EASA CS 25.473: 10 ft/s (600 fpm) design landing descent velocity at design landing weight. A structural design reference, not proof of damage.",
  amm: "ATSB AO-2023-010 quoting the Boeing 737 AMM: peak recorded acceleration above 2.2 g indicates a hard landing (main gear first, roll <= 2 deg), https://www.atsb.gov.au/investigations/ao-2023-010",
  fctmPitch: "Boeing 737NG FCTM, Landing Flare Profile: about 4-7 deg nose-up at touchdown, https://studylib.net/doc/27982068/737ng-fctm",
  annex14: "ICAO Annex 14, Table 5-1: aiming point marking 150 / 250 / 300 / 400 m from the threshold by landing distance available",
  faaTdz: "FAA Pilot/Controller Glossary: touchdown zone, the first 3,000 ft (914 m) of the runway",
  atp: "FAA ATP ACS AA.III.B.S10: touch down -250 / +500 ft (-76 / +152 m) from the aiming point markings, https://www.faa.gov/training_testing/testing/acs/atp_airplane_acs_11.pdf",
  ac9179: "FAA AC 91-79B 5.2.4: landing data commonly assumes touchdown 1,500 ft (457 m) past the threshold",
  fsf: "Flight Safety Foundation stabilised-approach criteria: stable by 1,000 ft (IMC) / 500 ft (VMC), sink normally <= 1,000 fpm, https://flightsafety.org/wp-content/uploads/2016/12/asw_aug08.pdf",
  poh: "Cessna 172S POH: land on the main wheels first, then lower the nosewheel gently",
} as const;

/** The aircraft class the rules ran with, or null when the debrief does not say ("unknown class"). */
export const aircraftClass = (flight: Debrief["flight"]): AircraftClass | null => flight.class ?? null;

const band = (to: number, grade: Grade, label: string, kind: Provenance, source: string): Band => ({ to, grade, label, kind, source });

/** Touchdown sink rate, fpm downward. */
export const SINK: Record<AircraftClass, Scale> = {
  light: { min: 0, max: 450, bands: [
    band(120, "good", "Smooth", "comfort", SRC.flare),
    band(210, "ok", "Normal", "comfort", SRC.flare),
    band(330, "firm", "Firm", "comfort", SRC.flare),
    band(Infinity, "review", "High sink rate, review", "comfort", SRC.flare),
  ] },
  transport: { min: 0, max: 720, bands: [
    band(180, "good", "Typical (Boeing 120-180 fpm)", "reference", SRC.boeingTypical),
    band(240, "ok", "Normal", "reference", SRC.boeingReport),
    band(600, "firm", "Firm (crews report hard above 240)", "reference", SRC.boeingReport),
    band(Infinity, "review", "Above the 600 fpm design reference", "reference", SRC.cs25),
  ] },
};

/** Peak normal load at touchdown, total g (1 g in level flight). */
export const PEAK_G: Record<AircraftClass, Scale> = {
  light: { min: 1, max: 2, bands: [
    band(1.1, "good", "Smooth", "comfort", SRC.flare),
    band(1.35, "ok", "Normal", "comfort", SRC.flare),
    band(1.75, "firm", "Firm", "comfort", SRC.flare),
    band(Infinity, "review", "Hard (comfort review only)", "comfort", SRC.flare),
  ] },
  transport: { min: 1, max: 2.5, bands: [
    band(1.1, "good", "Smooth", "comfort", SRC.loadflight),
    band(1.4, "ok", "Normal", "comfort", SRC.loadflight),
    band(2.2, "firm", "Firm", "comfort", SRC.loadflight),
    band(Infinity, "review", "Possible inspection trigger (737 AMM 2.2 g)", "reference", SRC.amm),
  ] },
};

/** Pitch at touchdown: only the transport reference is numeric; a light aircraft gets the POH technique as text. */
export const PITCH: { transport: Scale; light: { text: string; source: string } } = {
  transport: { min: 0, max: 10, bands: [
    band(4, "firm", "Below the 4-7 deg reference", "reference", SRC.fctmPitch),
    band(7, "good", "Normal attitude (Boeing 4-7 deg)", "reference", SRC.fctmPitch),
    band(Infinity, "firm", "Above the 4-7 deg reference: tail clearance", "reference", SRC.fctmPitch),
  ] },
  light: { text: "Target: main wheels first, nosewheel held off", source: SRC.poh },
};

/** ICAO Annex 14 aiming point distance from the threshold, by landing distance available. */
export const aimingPoint = (lda: number) => (lda < 800 ? 150 : lda < 1200 ? 250 : lda < 2400 ? 300 : 400);

/** Touchdown distance past the threshold; the touchdown zone length comes from the analyser (fix.tdz_m). */
export function distanceScale(fix: RunwayFix, cls: AircraftClass | null): Scale {
  const ap = aimingPoint(fix.lda_m), tdz = fix.tdz_m;
  const max = Math.min(fix.lda_m, Math.max(tdz * 1.25, fix.along_m * 1.1));
  const short = band(0, "review", "Before the threshold (short)", "reference", SRC.faaTdz);
  const long = band(Infinity, "review", "Long: past the touchdown zone", "reference", SRC.faaTdz);
  const marks: Mark[] = [{ at: ap, label: `Aiming point ${ap} m`, source: SRC.annex14 }];
  if (cls !== "transport") return { min: -0.06 * max, max, marks, bands: [short, band(tdz, "good", "Inside the touchdown zone", "reference", SRC.faaTdz), long] };
  marks.push({ at: 457, label: "1,500 ft landing-data assumption", source: SRC.ac9179 });
  return { min: -0.06 * max, max, marks, bands: [
    short,
    band(ap - 76, "ok", "Before the aiming-point window", "reference", SRC.atp),
    band(ap + 152, "good", "Aiming-point window (-76 / +152 m)", "reference", SRC.atp),
    band(tdz, "firm", "Past the aiming window, inside the touchdown zone", "reference", SRC.atp),
    long,
  ] };
}

/** Chart reference lines, with their source. */
export interface RefLine { y: number; label: string; source?: string }
export const APPROACH_GATES: RefLine[] = [
  { y: 1000, label: "1,000 ft gate: stable by here (IMC)", source: SRC.fsf },
  { y: 500, label: "500 ft gate: stable by here (VMC)", source: SRC.fsf },
];
export const SINK_LIMIT: RefLine = { y: -1000, label: "-1,000 fpm: stabilised-approach sink limit", source: SRC.fsf };
/** The g line where the class's touchdown scale turns to "review". */
export function gLine(cls: AircraftClass | null): RefLine | null {
  const last = cls ? PEAK_G[cls].bands.at(-2) : undefined;
  return last && cls ? { y: last.to, label: cls === "transport" ? "2.2 g: 737 AMM hard-landing indication" : "1.75 g: sim comfort review", source: PEAK_G[cls].bands.at(-1)!.source } : null;
}

export const bandOf = (scale: Scale, value: number) => scale.bands.find((b) => value <= b.to) ?? scale.bands.at(-1)!;

/** From the start of the approach that ended in this touchdown to 10 s into the rollout. */
export function landingWindow(t: number, phases: PhaseSpan[]): [number, number] {
  const before = phases.filter((p) => p.start_s <= t);
  const approach = before.findLast((p) => p.phase === "Approach") ?? before.at(-1);
  return [Math.min(approach?.start_s ?? t, t), t + 10];
}
export const eventsIn = (events: FlightEvent[], [start, end]: [number, number]) => events.filter((e) => e.end_s >= start && e.start_s <= end);

const LEVEL_GRADE: Record<Level, Grade | null> = { high: "review", medium: "firm", low: "ok", advisory: null };
export interface Reason { grade: Grade; text: string }
export interface Verdict { grade: Grade; title: string; reasons: Reason[] }
const TITLE: Record<Grade, string> = { good: "Good landing", ok: "Acceptable", firm: "Firm, check", review: "Needs review" };

/**
 * The card's overall verdict: the worst of the touchdown bands, the runway position, a bounce and every event
 * from the approach to the rollout, so a soft touchdown never hides an unstable approach or a long landing.
 */
export function landingVerdict(td: Touchdown, cls: AircraftClass | null, events: FlightEvent[]): Verdict {
  const reasons: Reason[] = [];
  const add = (grade: Grade, text: string) => { if (grade !== "good") reasons.push({ grade, text }); };
  if (cls) {
    const sink = bandOf(SINK[cls], td.sink_fpm), g = bandOf(PEAK_G[cls], td.peak_g);
    add(sink.grade, `Sink rate: ${sink.label.toLowerCase()}`);
    add(g.grade, `Peak load: ${g.label.toLowerCase()}`);
    if (cls === "transport" && td.pitch_deg !== undefined) { const p = bandOf(PITCH.transport, td.pitch_deg); add(p.grade, `Pitch: ${p.label.toLowerCase()}`); }
  }
  if (td.bounce) add("firm", "Bounced");
  const fix = td.runway;
  if (fix) { const d = bandOf(distanceScale(fix, cls), fix.along_m); add(d.grade, d.label); }
  for (const level of ["high", "medium", "low"] as const) {
    const titles = [...new Set(events.filter((e) => e.level === level).map((e) => e.title))];
    if (titles.length) add(LEVEL_GRADE[level]!, `${level[0]!.toUpperCase()}${level.slice(1)} events, approach to rollout: ${titles.join(", ")}`);
  }
  reasons.sort((a, b) => GRADES.indexOf(b.grade) - GRADES.indexOf(a.grade));
  const grade = reasons.reduce<Grade>((acc, r) => worst(acc, r.grade), "good");
  // Without class bands only events and position were judged, so "good" claims no more than that.
  return { grade, title: grade === "good" && !cls ? "No issues flagged" : TITLE[grade], reasons };
}
