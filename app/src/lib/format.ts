import { getPrefs } from "./prefs.ts";
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
/** "Fair · 77.4 of 100"; without a scored landing "Fair · 4 pts from 2 events", each broken rule once at its worst level. */
export const assessmentText = ({ grade, points, findings, score, confidence }: Assessment) => score !== undefined
  ? `${GRADE_LABEL[grade]} · ${number(score, 1)} of 100${confidence === "partial" ? " (partial data)" : ""}`
  : `${GRADE_LABEL[grade]} · ${points} pt${points === 1 ? "" : "s"} from ${findings} event${findings === 1 ? "" : "s"} (no scored landing)`;

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

/** The language tag for dates and numbers: the user's choice, or the system's (`undefined`). */
const locale = () => { const l = getPrefs().locale; return l === "system" ? undefined : l; };
const zone = () => (getPrefs().time === "utc" ? "UTC" : undefined);
// Formats are cached per locale and zone, as building an Intl formatter per table cell is slow at 500 rows.
const cache = new Map<string, Intl.DateTimeFormat>();
function dateTime(options: Intl.DateTimeFormatOptions) {
  const key = `${locale()}|${zone()}|${JSON.stringify(options)}`;
  let format = cache.get(key);
  if (!format) cache.set(key, (format = new Intl.DateTimeFormat(locale(), { ...options, timeZone: zone() })));
  return format;
}
/** The date in the chosen time zone (UTC or local), so it never disagrees with the time beside it. */
export const flightDate = (iso?: string | null) => (iso ? dateTime({ day: "2-digit", month: "short", year: "numeric" }).format(new Date(iso)) : "Unknown date");
/** "14:32Z" in UTC; "14:32" in local time, where the zone is the user's own. The name is kept for its many call sites. */
export const flightTimeUtc = (iso?: string) => (iso ? `${dateTime({ hour: "2-digit", minute: "2-digit", hourCycle: "h23" }).format(new Date(iso))}${getPrefs().time === "utc" ? "Z" : ""}` : "");

/** "InitialClimb" -> "Initial climb". Unknown names from a newer backend still read well. */
export const phaseName = (phase: string) => phase.replace(/([a-z])([A-Z])/g, "$1 $2").replace(/ (\w)/g, (_, c: string) => ` ${c.toLowerCase()}`);

export const aircraftName = (aircraft?: Aircraft) => aircraft?.icao ?? aircraft?.title ?? "Unknown aircraft";

/** A flight's readable name: the user's title when it has one, else "28 Oct 2026 · LTFM → LTAI · A320". */
export function flightLabel(flight: { started_utc?: string; departure?: string; arrival?: string; aircraft?: Aircraft; title?: string }) {
  if (flight.title?.trim()) return flight.title.trim();
  const route = flight.departure || flight.arrival ? `${flight.departure ?? "?"} → ${flight.arrival ?? "?"}` : "";
  return [flightDate(flight.started_utc), route, aircraftName(flight.aircraft)].filter(Boolean).join(" · ");
}

export const number = (value: number, digits = 0) => value.toLocaleString(locale(), { minimumFractionDigits: digits, maximumFractionDigits: digits });

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

const LB_PER_KG = 2.2046226218;

/** A length given in metres, in the chosen runway unit: "1,830 m" or "6,004 ft". `other` gives the unit not chosen, for the secondary line. */
export function lengthM(metres: number, digits = 0, other = false) {
  const feet = (getPrefs().distance === "ft") !== other;
  return feet ? `${number(metres / M_PER_FT, digits)} ft` : `${number(metres, digits)} m`;
}
/** The chosen runway length unit, "m" or "ft". */
export const lengthUnit = () => getPrefs().distance;
/** A length in metres as a bare number in the chosen unit, for gauges that print the unit apart. */
export const lengthValue = (metres: number) => (getPrefs().distance === "ft" ? metres / M_PER_FT : metres);

/** Fuel mass recorded in kg, as a bare number in the chosen unit (the unit comes from `fuelUnit`). */
export const fuelValue = (kg: number, digits = 0) => number(getPrefs().fuel === "lb" ? kg * LB_PER_KG : kg, digits);
/** "kg", "lb", "kg/h", "lb/nm": the chosen fuel unit, over `per` if given. */
export const fuelUnit = (per?: string) => `${getPrefs().fuel}${per ? `/${per}` : ""}`;
