import type { Aircraft, Assessment, FlightEvent, Grade, Level } from "../types";

/** Most severe first. */
export const LEVELS: Level[] = ["high", "medium", "low", "advisory"];
export const LEVEL_LABEL: Record<Level, string> = { high: "High", medium: "Medium", low: "Low", advisory: "Advisory" };
/** What each level means, for the legend. */
export const LEVEL_MEANING: Record<Level, string> = {
  high: "A limit clearly exceeded. In an airline this is reviewed with the crew and may need a maintenance check.",
  medium: "A significant deviation from the standard. Worth debriefing.",
  low: "A minor deviation, just past the first threshold.",
  advisory: "Recorded for context, not a fault.",
};
/** Best first. */
export const GRADES: Grade[] = ["good", "fair", "poor"];
export const GRADE_LABEL: Record<Grade, string> = { good: "Good", fair: "Fair", poor: "Poor" };
/** "Fair · 77.4 of 100"; without a scored landing "Fair · 4 pts from 2 findings", each broken rule once at its worst level. */
export const assessmentText = ({ grade, points, findings, score, confidence }: Assessment) => score !== undefined
  ? `${GRADE_LABEL[grade]} · ${number(score, 1)} of 100${confidence === "partial" ? " (partial data)" : ""}`
  : `${GRADE_LABEL[grade]} · ${points} pt${points === 1 ? "" : "s"} from ${findings} finding${findings === 1 ? "" : "s"} (no scored landing)`;

/** Higher is more severe; for sorting. */
export const levelRank = (level?: Level) => (level ? 4 - LEVELS.indexOf(level) : 0);

const pad = (value: number) => String(value).padStart(2, "0");

/** 754 -> "12:34", 4000 -> "1:06:40". */
export function clock(seconds: number) {
  const total = Math.max(0, Math.round(seconds));
  const h = Math.floor(total / 3600), m = Math.floor((total % 3600) / 60), s = total % 60;
  return h ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

/** 754 -> "12 min", 4000 -> "1 h 07 min". */
export function duration(seconds: number) {
  const minutes = Math.round(seconds / 60);
  if (minutes < 1) return `${Math.round(seconds)} s`;
  return minutes >= 60 ? `${Math.floor(minutes / 60)} h ${pad(minutes % 60)} min` : `${minutes} min`;
}

const dateFormat = new Intl.DateTimeFormat("en-GB", { day: "2-digit", month: "short", year: "numeric" });
const timeFormat = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", timeZone: "UTC" });
export const flightDate = (iso?: string | null) => (iso ? dateFormat.format(new Date(iso)) : "Unknown date");
export const flightTimeUtc = (iso?: string) => (iso ? `${timeFormat.format(new Date(iso))}Z` : "");

/** "InitialClimb" -> "Initial climb". Unknown names from a newer backend still read well. */
export const phaseName = (phase: string) => phase.replace(/([a-z])([A-Z])/g, "$1 $2").replace(/ (\w)/g, (_, c: string) => ` ${c.toLowerCase()}`);

export const aircraftName = (aircraft?: Aircraft) => aircraft?.icao ?? aircraft?.title ?? "Unknown aircraft";

export const number = (value: number, digits = 0) => value.toLocaleString("en-US", { minimumFractionDigits: digits, maximumFractionDigits: digits });

/** Decimals for a measured value: enough for g and Mach (M), none for fpm or ft unless `digits` asks for more. */
export function measured(value: number, unit?: string, digits?: number) {
  const places = digits ?? (unit === "g" ? 2 : unit === "M" ? 3 : Math.abs(value) < 10 && !Number.isInteger(value) ? 1 : 0);
  return `${number(value, places)}${unit ? ` ${unit}` : ""}`;
}

/** The "why" of an event: its measure, the value against the threshold it crossed. */
export function why(event: FlightEvent) {
  if (event.value === undefined) return { expression: event.measure, comparison: "was true" };
  if (event.threshold === undefined) return { expression: event.measure, comparison: `= ${measured(event.value, event.unit)}` };
  const sign = event.value < event.threshold ? "<" : ">";
  // A value just past its threshold keeps a decimal, so "15.4 deg > 15 deg" never reads as "15 deg > 15 deg".
  const digits = measured(event.value, event.unit) === measured(event.threshold, event.unit) ? (event.unit === "g" ? 3 : 1) : undefined;
  return { expression: event.measure, comparison: `= ${measured(event.value, event.unit, digits)} ${sign} ${measured(event.threshold, event.unit)}` };
}

/** Metres per foot, exact. */
export const M_PER_FT = 0.3048;
