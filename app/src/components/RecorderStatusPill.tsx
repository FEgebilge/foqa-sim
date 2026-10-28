import { FileText, PackageCheck } from "lucide-react";
import { Button } from "@ui/components/button/button";
import { Popover, PopoverContent, PopoverTrigger } from "@ui/components/popover/popover";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { api } from "../api";
import { useNotify } from "../lib/context";
import { clock } from "../lib/format";
import { SETUP_HREF } from "../lib/route";
import type { RecorderState } from "../lib/use-backend";
import { useVisible } from "../lib/visible";
import { useElapsed } from "./use-elapsed";
import type { RecorderStatus } from "../types";

const COPY: Record<RecorderStatus["state"], { label: string; title: string; hint: string; color: string }> = {
  waiting_for_sim: { label: "Waiting for sim", title: "Waiting for MSFS", hint: "MSFS 2020 or 2024 is not running, or has not reached its main menu yet. The recorder connects on its own.", color: "var(--text-muted)" },
  connected: { label: "Connected", title: "Connected to MSFS", hint: "Load a flight and it is recorded from that moment. Nothing to press.", color: "var(--success)" },
  recording: { label: "Recording", title: "Recording this flight", hint: "When the flight ends, or you go back to the main menu, it is analysed and listed under Flights.", color: "var(--level-high)" },
  error: { label: "Recorder error", title: "The recorder stopped", hint: "Restarting FOQA-Sim usually reconnects it. The log has the details.", color: "var(--danger)" },
};

/** The recorder's state, always visible in the header: the first thing a pilot checks before flying. A click shows the details. */
export function RecorderStatusPill({ status }: { status: RecorderState | null }) {
  const visible = useVisible();
  const notify = useNotify();
  const recording = status?.value.state === "recording";
  const seconds = useElapsed(status);

  if (!status) return <span className="h-8 w-36 animate-pulse rounded-full bg-muted" aria-label="Checking the recorder" />;
  const { value } = status;
  const copy = COPY[value.state];
  const elapsed = seconds === null ? null : clock(seconds);
  const openLog = () => api.openLogFolder().catch((error: unknown) => notify({ tone: "error", title: "Could not open the log folder", body: error instanceof Error ? error.message : String(error) }));

  return (
    <>
    {/* The state change is announced; the ticking clock is not. */}
    <span className="sr-only" aria-live="polite">{`Recorder: ${copy.label}`}</span>
    <Popover>
      <Tooltip content={value.state === "error" ? value.message : copy.hint} side="bottom">
        <PopoverTrigger className="inline-flex h-8 items-center gap-2 rounded-full border border-line bg-surface px-3 text-sm whitespace-nowrap hover:bg-muted"
          style={value.state === "error" ? { borderColor: `color-mix(in oklab, ${copy.color} 45%, var(--border))` } : undefined}
          aria-label={`Recorder: ${copy.label}${elapsed ? `, ${elapsed}` : ""}. Details`}>
          <span className="relative flex size-2.5" aria-hidden="true">
            {/* Only a recording pulses, and only while the window is shown and motion is welcome. */}
            {visible && recording ? <span className="absolute inline-flex size-full rounded-full opacity-60 motion-safe:animate-ping" style={{ background: copy.color }} /> : null}
            <span className="relative inline-flex size-2.5 rounded-full" style={{ background: copy.color }} />
          </span>
          <span className="hidden font-medium sm:inline">{copy.label}</span>
          {elapsed ? <span className="readout text-fg-2">{elapsed}</span> : null}
        </PopoverTrigger>
      </Tooltip>
      <PopoverContent align="end" className="w-80">
        <div className="flex flex-col gap-2">
          <p className="flex items-center gap-2 text-sm font-semibold">
            <span className="size-2.5 shrink-0 rounded-full" style={{ background: copy.color }} aria-hidden="true" />{copy.title}
          </p>
          {value.state === "error" ? <p className="readout rounded-md border border-line-subtle bg-muted px-2.5 py-1.5 text-xs break-words text-fg">{value.message}</p> : null}
          {value.state === "recording" ? (
            <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-0.5 text-xs">
              <dt className="text-fg-3">Elapsed</dt><dd className="readout">{elapsed}</dd>
              <dt className="text-fg-3">Flight</dt><dd className="readout truncate">{value.flight_id}</dd>
            </dl>
          ) : null}
          <p className="text-xs leading-relaxed text-fg-2">{copy.hint}</p>
          <div className="mt-1 flex flex-wrap gap-2">
            {value.state === "error" ? <Button variant="secondary" size="sm" onClick={openLog}><FileText size={15} aria-hidden="true" />Open log folder</Button> : null}
            <Button href={SETUP_HREF} variant="ghost" size="sm"><PackageCheck size={15} aria-hidden="true" />Setup checks</Button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
    </>
  );
}
