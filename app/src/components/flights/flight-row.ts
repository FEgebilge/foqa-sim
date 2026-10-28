import { aircraftName, flightDate } from "../../lib/format.ts";
import type { FlightSummary, Level } from "../../types";

/** Flat sortable values next to the summary itself, so every column sorts on the value it shows. */
export type FlightRow = Record<string, unknown> & {
  flight: FlightSummary;
  id: string;
  /** ISO UTC, which sorts in time order; none without a start time. */
  date?: string;
  aircraft: string;
  route: string;
  duration: number;
  landings: number;
  /** The landing score; none without a scored landing, so those flights sort after the scored ones either way. */
  assessment?: number;
  /** Severity first: one High outranks any number of lesser events. */
  events: number;
  /** A recording Trends counts as a flight. */
  counted: boolean;
  /** Lower-case text search looks in. */
  text: string;
};

/** Counts per level as digits of one number, the most severe on top (999 of a level at most). */
const SEVERITY: [Level, number][] = [["high", 1e9], ["medium", 1e6], ["low", 1e3], ["advisory", 1]];
export const severity = (counts: Record<Level, number>) => SEVERITY.reduce((sum, [level, weight]) => sum + Math.min(counts[level] ?? 0, 999) * weight, 0);

/** The local calendar day, as the date column shows it: "2026-10-26". */
export const dayKey = (iso?: string) => {
  if (!iso) return undefined;
  const d = new Date(iso);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};

export const toRow = (flight: FlightSummary): FlightRow => {
  const { aircraft, departure, arrival, meta, started_utc } = flight;
  return {
    flight,
    id: flight.id,
    date: started_utc,
    aircraft: aircraftName(aircraft),
    route: `${departure ?? ""}${arrival ?? ""}`,
    duration: flight.duration_s,
    landings: flight.landings,
    assessment: flight.assessment.score,
    events: severity(flight.event_counts),
    counted: (flight.kind ?? "flight") === "flight",
    text: [aircraft?.icao, aircraft?.title, departure, arrival, flight.id, meta?.title, meta?.notes, ...(meta?.tags ?? []), started_utc && flightDate(started_utc), dayKey(started_utc)]
      .filter(Boolean).join(" ").toLowerCase(),
  };
};
