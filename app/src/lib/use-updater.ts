import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { AppUpdate } from "../types";
import { useNotify } from "./context";

const CHECKED = "foqa-sim.update-checked";
const DAY = 24 * 60 * 60 * 1000;

export type UpdaterState =
  | { phase: "idle" | "checking" }
  | { phase: "available"; update: AppUpdate }
  | { phase: "installing"; update: AppUpdate; done: number; total?: number; finished?: boolean };

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** True when the last successful check was less than a day ago. Without storage the check just runs. */
function checkedToday() {
  try { return Date.now() - Number(localStorage.getItem(CHECKED)) < DAY; } catch { return false; }
}

/**
 * Looks for a newer version once at start (at most once a day) and when asked. The automatic check stays silent when
 * the feed cannot be reached (offline, no release yet); the button's check says so. Never installs while a flight records.
 */
export function useUpdater(recording: boolean) {
  const notify = useNotify();
  const [state, setState] = useState<UpdaterState>({ phase: "idle" });
  const [dismissed, setDismissed] = useState(false);
  const [error, setError] = useState<string>();
  const busy = useRef(false);
  const stateRef = useRef(state);
  useEffect(() => { stateRef.current = state; }, [state]);
  const recordingNow = useRef(recording);
  useEffect(() => { recordingNow.current = recording; }, [recording]);

  const check = useCallback(async (manual: boolean) => {
    if (busy.current) return;
    busy.current = true;
    // A found update stays offered while the next check runs and if it fails.
    const found = stateRef.current.phase === "available" ? stateRef.current : null;
    if (!found) setState({ phase: "checking" });
    setError(undefined);
    try {
      const update = await api.checkUpdate();
      // Only "nothing new" waits a day: a found update is offered again at the next start until it is installed.
      if (!update) try { localStorage.setItem(CHECKED, String(Date.now())); } catch { /* storage unavailable: asked again next start */ }
      setDismissed(false);
      setState(update ? { phase: "available", update } : { phase: "idle" });
      if (manual && !update) notify({ title: "FOQA-Sim is up to date" });
    } catch (e) {
      setState(found ?? { phase: "idle" });
      if (manual) notify({ title: "Could not check for updates", body: message(e), tone: "error" });
    } finally {
      busy.current = false;
    }
  }, [notify]);

  // Once at start, after the window has loaded.
  useEffect(() => {
    if (!api.live || checkedToday()) return;
    const timer = window.setTimeout(() => void check(false), 3000);
    return () => window.clearTimeout(timer);
  }, [check]);

  const install = useCallback(async () => {
    if (state.phase !== "available" || busy.current) return;
    if (recordingNow.current) { setError("A flight is being recorded."); return; }
    const { update } = state;
    busy.current = true;
    setError(undefined);
    setState({ phase: "installing", update, done: 0 });
    try {
      await api.installUpdate((done, total, finished) => setState({ phase: "installing", update, done, total, finished }));
      // The installer ends this process and starts the new version; nothing more to do here.
    } catch (e) {
      setState({ phase: "available", update });
      setError(message(e));
    } finally {
      busy.current = false;
    }
  }, [state]);

  return { state, error, dismissed, recording, check: () => check(true), install, dismiss: () => setDismissed(true) };
}

export type Updater = ReturnType<typeof useUpdater>;
