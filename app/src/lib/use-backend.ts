import { useCallback, useEffect, useState } from "react";
import { api, type BackendEvents } from "../api";
import type { Debrief, FlightSummary, RecorderStatus } from "../types";

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

/** A load that can be repeated; the previous data stays on screen while a refresh runs. */
function useLoad<T>(load: () => Promise<T>) {
  const [state, setState] = useState<Load<T>>({ status: "loading" });
  const [refreshing, setRefreshing] = useState(false);
  const fetchData = useCallback(() => load()
    .then((data) => setState({ status: "ready", data }))
    .catch((error: unknown) => setState({ status: "error", message: message(error) })), [load]);
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
