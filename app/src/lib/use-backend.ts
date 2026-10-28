import { useCallback, useEffect, useRef, useState } from "react";
import { api, type BackendEvents } from "../api";
import type { Debrief, FlightSummary, RecorderStatus, SetupStatus, TrendFilter } from "../types";

export type Load<T> = { status: "loading" } | { status: "ready"; data: T } | { status: "error"; message: string };

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** Subscribes to a backend event for the component's lifetime. */
export function useBackendEvent<K extends keyof BackendEvents>(event: K, handler: (payload: BackendEvents[K]) => void) {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    api.on(event, handler).then((stop) => (gone ? stop() : (unlisten = stop)));
    return () => {
      gone = true;
      unlisten?.();
    };
  }, [event, handler]);
}

/** A load that can be repeated; the previous data stays on screen while a refresh runs, and only the latest request's
 * answer is kept, so a slow earlier one (an older filter) never replaces a newer one. */
function useLoad<T>(load: () => Promise<T>) {
  const [state, setState] = useState<Load<T>>({ status: "loading" });
  const [refreshing, setRefreshing] = useState(false);
  const latest = useRef(0);
  const fetchData = useCallback(() => {
    const request = ++latest.current;
    const keep = (next: Load<T>) => { if (request === latest.current) setState(next); };
    return load()
      .then((data) => keep({ status: "ready", data }))
      .catch((error: unknown) => keep({ status: "error", message: message(error) }));
  }, [load]);
  useEffect(() => { fetchData(); }, [fetchData]);
  const refresh = useCallback(() => {
    setRefreshing(true);
    fetchData().finally(() => setRefreshing(false));
  }, [fetchData]);
  return { state, setState, refresh, refreshing };
}

export function useFlights() {
  const load = useCallback(() => api.listFlights(), []);
  const result = useLoad(load);
  const { setState } = result;
  // A newly saved flight goes to the top (or replaces its older summary).
  const onSaved = useCallback((flight: FlightSummary) => setState((state) => state.status !== "ready" ? state : { status: "ready", data: [flight, ...state.data.filter((f) => f.id !== flight.id)] }), [setState]);
  useBackendEvent("flight-saved", onSaved);
  return result;
}

export function useDebrief(id: string) {
  const load = useCallback(() => api.debrief(id), [id]);
  const result = useLoad<Debrief>(load);
  return { state: result.state, refresh: result.refresh, refreshing: result.refreshing };
}

/** The recorder state, with the time it arrived so a recording clock can keep ticking between pushes. */
export type RecorderState = { value: RecorderStatus; at: number };

export function useRecorderStatus() {
  const [status, setStatus] = useState<RecorderState | null>(null);
  useEffect(() => {
    api.recorderStatus().then((value) => setStatus({ value, at: Date.now() })).catch((error: unknown) => setStatus({ value: { state: "error", message: message(error) }, at: Date.now() }));
  }, []);
  const onStatus = useCallback((value: RecorderStatus) => setStatus({ value, at: Date.now() }), []);
  useBackendEvent("recorder-status", onStatus);
  return status;
}

/** The installation checks, run again whenever the recorder's state changes; `fix` runs one fix and shows the checks it returns. */
export function useSetup(recorder: RecorderStatus["state"] | undefined) {
  const load = useCallback(() => api.setupStatus(), [recorder]); // eslint-disable-line react-hooks/exhaustive-deps
  const result = useLoad(load);
  const { setState } = result;
  const [pending, setPending] = useState<string | null>(null);
  const fix = useCallback((key: string, run: () => Promise<SetupStatus>) => {
    setPending(key);
    return run().then((data) => setState({ status: "ready", data })).finally(() => setPending(null));
  }, [setState]);
  return { ...result, pending, fix };
}

export type Setup = ReturnType<typeof useSetup>;

export function useChallenges() {
  const load = useCallback(() => api.challengesList(), []);
  return useLoad(load);
}

/** Every local attempt, loaded again whenever the backend reports one changed. */
export function useAttempts() {
  const load = useCallback(() => api.attemptsList(), []);
  const result = useLoad(load);
  const { refresh } = result;
  useBackendEvent("attempt-changed", useCallback(() => refresh(), [refresh]));
  return result;
}

/** The trends for a filter; the previous numbers stay on screen while a changed filter loads. */
export function useTrends(filter: TrendFilter) {
  const { aircraft, airport, days } = filter;
  const load = useCallback(() => api.trends({ aircraft, airport, days }), [aircraft, airport, days]);
  const result = useLoad(load);
  // A newly saved flight changes every aggregate: ask again.
  const { refresh } = result;
  const onSaved = useCallback(() => refresh(), [refresh]);
  useBackendEvent("flight-saved", onSaved);
  return result;
}
