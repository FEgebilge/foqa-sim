import { useEffect, useState } from "react";
import type { RecorderState } from "../lib/use-backend";
import { useVisible } from "../lib/visible";

/** Seconds into the recording, ticking between the recorder's pushes while the window is shown; null when not recording. */
export function useElapsed(status: RecorderState | null) {
  const [now, setNow] = useState(() => Date.now());
  const visible = useVisible();
  const recording = status?.value.state === "recording";
  useEffect(() => {
    if (!recording || !visible) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [recording, visible]);
  return status?.value.state === "recording" ? status.value.elapsed_s + Math.max(0, now - status.at) / 1000 : null;
}
