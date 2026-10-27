import { aircraftName } from "../../lib/format";
import type { FlightSummary } from "../../types";

/** Flat sortable values next to the summary itself, so every column sorts on the value it shows. */
export type FlightRow = Record<string, unknown> & {
  flight: FlightSummary;
  id: string;
  date: number;
  aircraft: string;
  route: string;
  duration: number;
  landings: number;
  assessment: number;
  events: number;
};

export const toRow = (flight: FlightSummary): FlightRow => ({
  flight,
  id: flight.id,
  date: flight.started_utc ? Date.parse(flight.started_utc) : 0,
  aircraft: aircraftName(flight.aircraft),
  route: `${flight.departure ?? ""}${flight.arrival ?? ""}`,
  duration: flight.duration_s,
  landings: flight.landings,
  assessment: flight.assessment.score ?? 100 - flight.assessment.points * 10,
  events: Object.values(flight.event_counts).reduce((sum, count) => sum + count, 0),
});
