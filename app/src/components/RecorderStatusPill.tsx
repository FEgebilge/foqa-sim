import { useEffect, useState } from "react";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { clock } from "../lib/format";
import type { RecorderState } from "../lib/use-backend";
import type { RecorderStatus } from "../types";

const COPY: Record<RecorderStatus["state"], { label: string; hint: string; color: string }> = {
  waiting_for_sim: { label: "Waiting for sim", hint: "MSFS 2024 is not running. Recording starts on its own when the sim connects.", color: "var(--text-muted)" },
  connected: { label: "Connected", hint: "Connected to MSFS 2024. A flight is recorded as soon as one is loaded.", color: "var(--success)" },
  recording: { label: "Recording", hint: "This flight is being recorded. It is analysed and listed when it closes.", color: "var(--level-high)" },
  error: { label: "Recorder error", hint: "", color: "var(--danger)" },
};

/** The recorder's state, always visible in the header: the first thing a pilot checks before flying. */
export function RecorderStatusPill({ status }: { status: RecorderState | null }) {
  const [now, setNow] = useState(() => Date.now());
  const recording = status?.value.state === "recording";
  useEffect(() => {
    if (!recording) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [recording]);

  if (!status) return <span className="h-8 w-36 animate-pulse rounded-full bg-muted" aria-label="Checking the recorder" />;
  const { value } = status;
  const copy = COPY[value.state];
  const elapsed = value.state === "recording" ? clock(value.elapsed_s + Math.max(0, now - status.at) / 1000) : null;
  const hint = value.state === "error" ? value.message : copy.hint;

  return (
    <Tooltip content={hint} side="bottom">
      <span role="status" tabIndex={0} className="inline-flex h-8 items-center gap-2 rounded-full border border-line bg-surface px-3 text-sm">
        <span className="relative flex size-2.5" aria-hidden="true">
          {recording || value.state === "connected" ? <span className="absolute inline-flex size-full animate-ping rounded-full opacity-60" style={{ background: copy.color }} /> : null}
          <span className="relative inline-flex size-2.5 rounded-full" style={{ background: copy.color }} />
        </span>
        <span className="font-medium">{copy.label}</span>
        {elapsed ? <span className="readout text-fg-2">{elapsed}</span> : null}
      </span>
    </Tooltip>
  );
}
