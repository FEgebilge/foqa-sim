// The personal context of the latest flight: how it compares with your own recent flights, in the type and overall.
import type { FlightSummary, LandingFacts } from "../../types";

/** How many of your previous flights the means and the sparkline look back over. */
export const WINDOW = 10;

/** The trends rule for a flight (crates/debrief/src/trends.rs `counts`): a minute and a mile in the air at 40 kt or more.
 * A summary without `flown` (a mock, an older file) counts, so it is never silently dropped. */
export const counted = ({ flown }: FlightSummary) => !flown || (flown.airborne_s >= 60 && flown.distance_nm >= 1 && flown.max_gs_kt >= 40);

const time = (f: FlightSummary) => Date.parse(f.started_utc ?? "") || 0;

/** The aircraft type a flight is compared within; none without an ICAO code. */
export const typeOf = (f: FlightSummary) => f.aircraft?.icao;

/** Counted flights, newest first (ties by id, so the order is stable). */
export const byNewest = (flights: FlightSummary[]) =>
  flights.filter(counted).sort((a, b) => time(b) - time(a) || b.id.localeCompare(a.id));

/** The landing the assessment scored: the last full stop with a score, else the last landing. */
export const scoredLanding = (f: FlightSummary): LandingFacts | undefined => {
  const all = f.landings_detail ?? [];
  return all.findLast((l) => l.full_stop && l.score !== undefined) ?? all.at(-1);
};

export interface Compare {
  /** Previous flights it is compared with. */
  n: number;
  mean: number;
  /** Latest minus the mean. */
  delta: number;
}

const mean = (values: number[]) => values.reduce((a, b) => a + b, 0) / values.length;

function compare(value: number | undefined, previous: number[]): Compare | null {
  if (value === undefined || !previous.length) return null;
  const m = mean(previous);
  return { n: previous.length, mean: m, delta: value - m };
}

export interface Personal {
  /** Score against your last WINDOW scored flights in this type, and in any type. */
  type: Compare | null;
  overall: Compare | null;
  /** Place among every scored flight in the type, 1 = best, with how many there are. */
  rank: { place: number; of: number } | null;
  /** The scores of the last WINDOW + 1 scored flights in the type, oldest first, the latest last. */
  spark: number[];
  /** Touchdown sink rate and peak load against the same previous flights in the type. */
  sink: Compare | null;
  g: Compare | null;
}

/** `flights` are newest first (`byNewest`), `flights[0]` the latest. */
export function personal(flights: FlightSummary[]): Personal | null {
  const [latest, ...rest] = flights;
  if (!latest) return null;
  const score = (f: FlightSummary) => f.assessment.score;
  const scored = rest.filter((f) => score(f) !== undefined);
  const type = typeOf(latest);
  const sameType = type ? scored.filter((f) => typeOf(f) === type) : [];
  const recent = sameType.slice(0, WINDOW);
  const landing = (fs: FlightSummary[], pick: (l: LandingFacts) => number) => fs.flatMap((f) => { const l = scoredLanding(f); return l ? [pick(l)] : []; });
  const now = scoredLanding(latest);
  const own = score(latest);
  return {
    type: compare(own, recent.map((f) => score(f)!)),
    overall: compare(own, scored.slice(0, WINDOW).map((f) => score(f)!)),
    rank: own !== undefined && type ? { place: 1 + sameType.filter((f) => score(f)! > own).length, of: sameType.length + 1 } : null,
    spark: own === undefined ? [] : [...recent.map((f) => score(f)!).reverse(), own],
    sink: compare(now?.sink_fpm, landing(recent, (l) => l.sink_fpm)),
    g: compare(now?.peak_g, landing(recent, (l) => l.peak_g)),
  };
}

/** "1st", "2nd", "3rd", "11th". */
export const ordinal = (n: number) => {
  const tens = n % 100;
  return `${n}${tens >= 11 && tens <= 13 ? "th" : ["th", "st", "nd", "rd"][n % 10] ?? "th"}`;
};
