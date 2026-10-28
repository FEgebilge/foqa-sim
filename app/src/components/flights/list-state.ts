// What the flight list shows: search, filters, sort and scroll. Kept in a module-level store, so a trip into a debrief
// (which unmounts the list) comes back to the same place. Pure functions, so the filters are tested without a browser.
import type { Grade } from "../../types";
import { dayKey, type FlightRow } from "./flight-row.ts";

export type GradeFilter = "all" | Grade;
export type SortKey = "date" | "aircraft" | "route" | "assessment" | "duration" | "landings" | "events";

export interface ListState {
  query: string;
  grade: GradeFilter;
  sort: { key: SortKey; direction: "asc" | "desc" };
  aircraft?: string;
  /** Departure or arrival. */
  airport?: string;
  tag?: string;
  /** Local days, inclusive: "2026-10-01". */
  from?: string;
  to?: string;
  /** Also recordings that aren't flights (ground-only, too short). */
  showAll: boolean;
  /** Rows rendered, a page at a time: each row costs about a millisecond on every sort or selection. */
  limit: number;
  /** The window's scroll when the list was left. */
  scrollY: number;
}

export const PAGE = 100;
export const INITIAL: ListState = { query: "", grade: "all", sort: { key: "date", direction: "desc" }, showAll: false, limit: PAGE, scrollY: 0 };

let saved: ListState = INITIAL;
export const savedState = () => saved;
export const saveState = (state: ListState) => { saved = state; };

/** No search, filter or non-flights shown: what Clear all goes back to (the sort stays). */
export const cleared = (state: ListState): ListState => ({ ...INITIAL, sort: state.sort, scrollY: 0 });
export const isFiltered = (s: ListState) => Boolean(s.query.trim() || s.grade !== "all" || s.aircraft || s.airport || s.tag || s.from || s.to);

const aircraftOf = (row: FlightRow) => row.flight.aircraft?.icao ?? row.aircraft;

/** Every filter but the grade and the flight/non-flight split, which the screen counts separately. */
function matchesFilters(row: FlightRow, s: ListState, words: string[]) {
  const { departure, arrival, meta, started_utc } = row.flight;
  if (s.aircraft && aircraftOf(row) !== s.aircraft) return false;
  if (s.airport && departure !== s.airport && arrival !== s.airport) return false;
  if (s.tag && !meta?.tags?.some((tag) => tag.toLowerCase() === s.tag!.toLowerCase())) return false;
  const day = dayKey(started_utc);
  if ((s.from || s.to) && (!day || (s.from && day < s.from) || (s.to && day > s.to))) return false;
  return words.every((word) => row.text.includes(word));
}

const wordsOf = (query: string) => query.trim().toLowerCase().split(/\s+/).filter(Boolean);

/** The rows the other filters let through, before the grade and the non-flight split: what their counts are of. */
export const filtered = (rows: FlightRow[], s: ListState) => {
  const words = wordsOf(s.query);
  return rows.filter((row) => matchesFilters(row, s, words));
};

/** The rows the list shows. */
export const visibleRows = (base: FlightRow[], s: ListState) =>
  base.filter((row) => (s.showAll || row.counted) && (s.grade === "all" || row.flight.assessment.grade === s.grade));

export interface Option { value: string; hint: number }

/** Each value of the filter menus, with how many flights have it, most first. */
export function options(rows: FlightRow[]) {
  const tally = (values: (string | undefined)[][]) => {
    const counts = new Map<string, number>();
    for (const list of values) for (const value of new Set(list)) if (value) counts.set(value, (counts.get(value) ?? 0) + 1);
    return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([value, hint]): Option => ({ value, hint }));
  };
  return {
    aircraft: tally(rows.map((row) => [aircraftOf(row)])),
    airports: tally(rows.map(({ flight }) => [flight.departure, flight.arrival])),
    tags: tally(rows.map(({ flight }) => flight.meta?.tags ?? [])),
  };
}

const UNITS = ["B", "KB", "MB", "GB", "TB"];
/** 1_800_000 -> "1.7 MB" (binary multiples, as Explorer shows them). */
export function bytes(n: number) {
  let value = n, unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) { value /= 1024; unit++; }
  return `${unit && value < 10 ? value.toFixed(1) : Math.round(value)} ${UNITS[unit]}`;
}
