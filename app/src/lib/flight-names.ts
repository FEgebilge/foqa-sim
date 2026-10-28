import { useSyncExternalStore } from "react";
import type { FlightSummary } from "../types";

// The flight summaries the app has already loaded (the list, a debrief), so the header can name the open flight without a call of its own.
type Known = FlightSummary & { title?: string };
const known = new Map<string, Known>();
const listeners = new Set<() => void>();

export function rememberFlights(flights: Known[]) {
  for (const flight of flights) known.set(flight.id, flight);
  listeners.forEach((listener) => listener());
}

/** The loaded summary of flight `id` (name it with `flightLabel`), or `undefined` until one is loaded. */
export const useKnownFlight = (id: string) => useSyncExternalStore(
  (listener) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
  () => known.get(id),
);
