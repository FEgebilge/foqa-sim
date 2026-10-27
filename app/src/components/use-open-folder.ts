import { useCallback } from "react";
import { api } from "../api";
import { useNotify } from "../lib/context";

/** "Open flights folder" from any screen; a failure is reported instead of silently ignored. */
export function useOpenFolder() {
  const notify = useNotify();
  return useCallback(() => {
    api.openFlightsFolder().catch((error: unknown) => notify({ tone: "error", title: "Could not open the flights folder", body: error instanceof Error ? error.message : String(error) }));
  }, [notify]);
}
