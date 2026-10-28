import { useSyncExternalStore } from "react";

const subscribe = (notify: () => void) => {
  document.addEventListener("visibilitychange", notify);
  return () => document.removeEventListener("visibilitychange", notify);
};

/** Whether the window is shown: a minimised or hidden one stops its timers and animations. */
export const useVisible = () => useSyncExternalStore(subscribe, () => document.visibilityState === "visible");
