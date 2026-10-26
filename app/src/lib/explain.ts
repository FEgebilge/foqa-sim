// Events and flights in plain language: the measured value against its limit, with units, and the one-line verdict.
import type { Debrief, FlightEvent, Level, Series } from "../types";
import { LEVEL_LABEL, LEVELS, measured, number } from "./format";
import { indexAt } from "./time";

/** What each severity level means, for the legend. */
export const LEVEL_MEANING: Record<Level, string> = {
  high: "A limit clearly exceeded. In an airline this is reviewed with the crew and may need a maintenance check.",
  medium: "A significant deviation from the standard. Worth debriefing.",
  low: "A minor deviation, just past the first threshold.",
  advisory: "Recorded for context, not a fault.",
};

const DEG = "°";
/** A measured value with its unit, degrees as a symbol. */
const show = (value: number, unit?: string, digits?: number) => (unit === "deg" ? `${measured(value, undefined, digits)}${DEG}` : measured(value, unit, digits));
const signed = (value: number, unit?: string) => `${value > 0 ? "+" : value < 0 ? "−" : ""}${show(Math.abs(value), unit)}`;

/** Phrases for the rule measures of the core set, given the value and its magnitude as shown; other measures read as the value alone. */
const PHRASE: Record<string, (value: number, shown: string) => string> = {
  "-vs": (_, s) => `${s} descent`,
  "abs(roll)": (_, s) => `${s} bank`,
  "ias - sel.speed": (v, s) => `${s} ${v < 0 ? "slow" : "fast"}`,
  "abs(nav.loc_dev)": (_, s) => `${s} off the localizer`,
  "abs(nav.gs_dev)": (_, s) => `${s} off the glideslope`,
  "td.peak_g": (_, s) => `${s} at touchdown`,
  "td.sink": (_, s) => `${s} sink at touchdown`,
  "abs(td.cross)": (_, s) => `${s} off the centreline`,
  "ias - profile.vfe": (_, s) => `${s} over VFE`,
  "profile.tail_takeoff_pitch - pitch": (_, s) => `${s} tail clearance left`,
  "profile.tail_landing_pitch - td.pitch": (_, s) => `${s} tail clearance left`,
  "td.peak_g - profile.hard_landing_g": (_, s) => `${s} over the inspection load`,
};
/** Measures that are a difference against a reference state the limit from it ("VFE +10 kt"). */
const REFERENCE: Record<string, string> = { "ias - sel.speed": "target", "ias - profile.vfe": "VFE", "td.peak_g - profile.hard_landing_g": "inspection load" };

const at = (series: Series, key: keyof Omit<Series, "t_s">, t: number) => series[key][indexAt(series.t_s, t)] ?? null;

export interface Reading {
  /** The value in words, e.g. "2,490 fpm descent" or "111 kt at flaps 20°, VFE 85 kt". */
  value: string;
  /** The threshold of the event's level, e.g. "High above 1,500 fpm". */
  limit: string;
}

/** The measured value against the limit it crossed; `null` for a condition (bounce, stall warning). */
export function reading(event: FlightEvent, series: Series): Reading | null {
  const { value, threshold, unit, measure } = event;
  if (value === undefined) return null;
  // A value just past its threshold keeps a decimal, so 15.4° never reads as 15° against a 15° limit.
  const digits = threshold !== undefined && show(value, unit) === show(threshold, unit) ? (unit === "g" ? 3 : 1) : undefined;
  const shown = show(Math.abs(value), unit, digits);
  let text = PHRASE[measure]?.(value, shown) ?? show(value, unit, digits);
  // Flap overspeed: IAS and flap angle at the worst point, VFE from the excess over it.
  if (measure === "ias - profile.vfe") {
    const ias = at(series, "ias_kt", event.peak_s ?? event.start_s), flap = at(series, "flap_deg", event.peak_s ?? event.start_s);
    if (ias !== null) text = `${show(ias, "kt")}${flap !== null ? ` at flaps ${show(flap, "deg")}` : ""}, VFE ${show(ias - value, "kt")}`;
  }
  if (threshold === undefined) return { value: text, limit: "" };
  const side = value < threshold ? "below" : "above";
  const ref = REFERENCE[measure];
  const limit = !ref ? show(threshold, unit) : threshold ? `${ref} ${signed(threshold, unit)}` : ref;
  return { value: text, limit: `${LEVEL_LABEL[event.level]} ${side} ${limit}` };
}

/** The worst finding first: the highest level, then the earliest. */
export const keyEvent = (events: FlightEvent[]) => LEVELS.flatMap((level) => events.filter((e) => e.level === level))[0] ?? null;

/** One sentence: the worst level, the key issue and where the aircraft landed. */
export function verdict({ flight, events, touchdowns, series }: Debrief) {
  const key = keyEvent(events);
  const runway = touchdowns.at(-1)?.runway;
  const landed = !touchdowns.length ? "no landing recorded"
    : runway ? `landed on runway ${runway.runway} at ${runway.icao}`
      : `landed${flight.arrival ? ` at ${flight.arrival}` : ""}, runway not identified`;
  const landings = touchdowns.length > 1 ? ` (${touchdowns.length} touchdowns)` : "";
  if (!key) return `Clean flight: no rule exceeded; ${landed}${landings}.`;
  const same = events.filter((e) => e.level === key.level).length - 1;
  const detail = reading(key, series)?.value;
  const more = same ? `, plus ${number(same)} more ${LEVEL_LABEL[key.level].toLowerCase()} event${same > 1 ? "s" : ""}` : "";
  return `${LEVEL_LABEL[key.level]}: ${key.title.toLowerCase()}${detail ? ` (${detail})` : ""}${more}; ${landed}${landings}.`;
}
