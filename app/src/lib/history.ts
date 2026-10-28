// The flight among the pilot's other flights: the older and newer one for stepping through a session, and the pilot's own
// median touchdown in the type for the landing gauges.
import type { FlightSummary, TrendLanding } from "../types";

/** Oldest first by start time; a flight without one goes first, and the id breaks ties so the order is total. */
const byDate = (a: FlightSummary, b: FlightSummary) => (a.started_utc ?? "").localeCompare(b.started_utc ?? "") || a.id.localeCompare(b.id);

/** The flights just before and after `id` in time; absent at either end, or when `id` is not in the list. */
export function adjacentFlights(flights: FlightSummary[], id: string): { older?: FlightSummary; newer?: FlightSummary } {
  const sorted = [...flights].sort(byDate);
  const at = sorted.findIndex((f) => f.id === id);
  return at < 0 ? {} : { older: sorted[at - 1], newer: sorted[at + 1] };
}

export function median(values: number[]): number | null {
  if (!values.length) return null;
  const s = [...values].sort((a, b) => a - b), mid = s.length >> 1;
  return s.length % 2 ? s[mid]! : (s[mid - 1]! + s[mid]!) / 2;
}

/** Fewer other flights in the type than this, and a median says nothing yet. */
export const MEDIAN_MIN_FLIGHTS = 3;

export interface TypeMedian { sink_fpm: number; peak_g: number; landings: number; flights: number }

/** The pilot's median sink rate and peak load over every landing in `type` on other flights than `id`; null under
 * MEDIAN_MIN_FLIGHTS such flights. */
export function typeMedian(landings: TrendLanding[], type: string, id: string): TypeMedian | null {
  const rows = landings.filter((l) => l.aircraft === type && l.flight_id !== id);
  const flights = new Set(rows.map((l) => l.flight_id)).size;
  if (flights < MEDIAN_MIN_FLIGHTS) return null;
  return { sink_fpm: median(rows.map((l) => l.sink_fpm))!, peak_g: median(rows.map((l) => l.peak_g))!, landings: rows.length, flights };
}
