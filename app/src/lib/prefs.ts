import { useSyncExternalStore } from "react";

/** How numbers are shown. Display only: recordings, exports and the backend stay SI, as recorded.
 * Altitude stays in feet and speed in knots: those are the aviation standard (ICAO Annex 5) whatever the country, so they are not options. */
export interface Prefs {
  fuel: "kg" | "lb";
  distance: "m" | "ft";
  time: "utc" | "local";
  /** A BCP 47 tag, or "system" for the operating system's language and region. */
  locale: string;
}

export const DEFAULT_PREFS: Prefs = { fuel: "kg", distance: "m", time: "utc", locale: "system" };
const KEY = "foqa-sim.prefs";

/** Keeps only known values, so a hand-edited or older entry cannot break the formatters. */
export function parsePrefs(raw: unknown): Prefs {
  const o = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const pick = <T extends string>(value: unknown, allowed: readonly T[], fallback: T) => (allowed.includes(value as T) ? (value as T) : fallback);
  return {
    fuel: pick(o.fuel, ["kg", "lb"], DEFAULT_PREFS.fuel),
    distance: pick(o.distance, ["m", "ft"], DEFAULT_PREFS.distance),
    time: pick(o.time, ["utc", "local"], DEFAULT_PREFS.time),
    locale: typeof o.locale === "string" && o.locale ? o.locale : DEFAULT_PREFS.locale,
  };
}

function stored(): Prefs {
  try {
    return parsePrefs(JSON.parse(localStorage.getItem(KEY) ?? "null"));
  } catch {
    return DEFAULT_PREFS;
  }
}

let current = stored();
const listeners = new Set<() => void>();

export const getPrefs = () => current;

export function setPrefs(change: Partial<Prefs>) {
  current = parsePrefs({ ...current, ...change });
  try {
    localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // Storage can be unavailable; the choice still applies for this session.
  }
  listeners.forEach((listener) => listener());
}

const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
};

/** The preferences, re-rendering the caller when they change. The formatters in lib/format.ts read them directly. */
export const useUnits = () => useSyncExternalStore(subscribe, getPrefs);
export const subscribePrefs = subscribe;
