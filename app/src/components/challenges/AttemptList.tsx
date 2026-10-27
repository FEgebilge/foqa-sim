import { useState } from "react";
import { Check, ChevronRight, History, X } from "lucide-react";
import { Badge, type BadgeTone } from "@ui/components/badge/badge";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import { flightDate, flightTimeUtc, measured, number } from "../../lib/format";
import type { AttemptLogLine, AttemptResult, AttemptState, AttemptView } from "../../types";

const STATE: Record<AttemptState, { label: string; tone: BadgeTone }> = {
  created: { label: "Created", tone: "info" },
  placing: { label: "Placing", tone: "info" },
  failed_start: { label: "Didn't start", tone: "danger" },
  flying: { label: "Flying", tone: "info" },
  approach: { label: "Past the gate", tone: "info" },
  rollout: { label: "Rolling out", tone: "info" },
  finished: { label: "Grading", tone: "info" },
  graded_local: { label: "Graded", tone: "neutral" },
  sliced: { label: "Graded", tone: "neutral" },
  done: { label: "Graded", tone: "neutral" },
  abandoned: { label: "Abandoned", tone: "neutral" },
};

const REASON: Record<string, string> = {
  stopped: "the rollout ended",
  finish: "Finish pressed",
  flight_closed: "the flight closed",
  recovered: "recovered after the app restarted",
  interrupted: "the app closed mid-attempt",
  replaced: "flown again",
  cancelled: "cancelled",
};
const reason = (r: unknown) => (typeof r === "string" ? REASON[r] ?? r : "");

const CHECK: Record<string, string> = {
  aircraft: "Aircraft", runway: "Runway", gate: "Gate", sim_rate: "Sim rate", slew: "Slew", pause: "Pause",
  continuity: "Continuous data", wind: "Wind", crosswind: "Crosswind", tailwind: "Tailwind", autopilot: "Autopilot",
  stop_on_runway: "Stopped on the runway",
};
const PART: Record<string, string> = {
  touchdown_point: "Touchdown point", sink_rate: "Sink rate", centreline: "Centreline", stability: "Stability from the gate",
  peak_g: "Peak g", bounce: "Bounce", rollout: "Rollout",
};
const words = (name: string, known: Record<string, string>) => known[name] ?? name.replace(/_/g, " ");

/** The live stages of an attempt, as a stepper: placed, landed, graded. */
export function Stages({ attempt }: { attempt: AttemptView }) {
  const rank = STAGE_RANK[attempt.state] ?? 0;
  return (
    <ol className="grid grid-cols-4 gap-1" aria-label="Attempt stages">
      {["Placed", "Landed", "Finished", "Graded"].map((label, i) => (
        <li key={label} className="flex flex-col gap-1.5" aria-current={rank === i + 1 ? "step" : undefined}>
          <span className="h-1 rounded-full" style={{ background: rank > i ? "var(--accent)" : "var(--border)" }} />
          <span className={`text-xs ${rank > i ? "text-fg" : "text-fg-3"}`}>{label}</span>
        </li>
      ))}
    </ol>
  );
}

/** How many of the stepper's stages a state has reached. */
const STAGE_RANK: Partial<Record<AttemptState, number>> = { flying: 1, approach: 1, rollout: 2, finished: 3, graded_local: 4, sliced: 4, done: 4 };

/** The attempts at one challenge, newest first; each opens to its checks, score parts and log. */
export function AttemptList({ attempts }: { attempts: AttemptView[] }) {
  return (
    <section aria-label="Attempts" className="flex flex-col gap-2">
      <div className="flex items-baseline justify-between gap-3">
        <h2 className="font-display text-sm font-semibold">Attempts</h2>
        <span className="readout text-xs text-fg-3">{attempts.length ? `${attempts.length} on this PC` : ""}</span>
      </div>
      {attempts.length ? (
        <ul className="panel divide-y divide-line overflow-hidden">
          {attempts.map((attempt, i) => <AttemptRow key={attempt.id} attempt={attempt} initiallyOpen={i === 0 && attempt.result !== null} />)}
        </ul>
      ) : (
        <div className="panel"><EmptyState size="sm" icon={<History size={18} />} title="No attempts yet" description="Each attempt is logged step by step and graded from the recorded flight." /></div>
      )}
    </section>
  );
}

function AttemptRow({ attempt, initiallyOpen }: { attempt: AttemptView; initiallyOpen: boolean }) {
  const [open, setOpen] = useState(initiallyOpen);
  const { label, tone } = STATE[attempt.state];
  const result = attempt.result;
  const failed = result?.checks.filter((c) => !c.ok).length ?? 0;
  return (
    <li>
      <button type="button" className="grid w-full grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3 px-4 py-3 text-left hover:bg-muted" aria-expanded={open} onClick={() => setOpen(!open)}>
        <ChevronRight size={15} className={`text-fg-3 transition-transform ${open ? "rotate-90" : ""}`} aria-hidden="true" />
        <span className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
          <span className="text-sm font-medium">Attempt {attempt.n}</span>
          <span className="readout text-xs text-fg-3">{flightDate(attempt.created_at)} {flightTimeUtc(attempt.created_at)}</span>
          <Badge size="sm" tone={tone}>{label}</Badge>
          {result ? <Badge size="sm" tone={result.valid ? "success" : "danger"}>{result.valid ? "Counts" : `Doesn't count · ${failed} check${failed === 1 ? "" : "s"} failed`}</Badge> : null}
          {attempt.state === "abandoned" && attempt.reason ? <span className="text-xs text-fg-3">{reason(attempt.reason)}</span> : null}
        </span>
        <span className="readout text-right">
          <span className="text-xl">{result?.score ? number(result.score.total, 1) : "–"}</span>
          {result?.score ? <span className="text-xs text-fg-3"> / 100</span> : null}
        </span>
      </button>
      {open ? (
        <div className="grid gap-5 border-t border-line-subtle bg-[color-mix(in_oklab,var(--surface-muted)_40%,transparent)] px-4 py-4 lg:grid-cols-2 2xl:grid-cols-3">
          {result ? <Checks result={result} /> : null}
          {result?.score ? <ScoreParts score={result.score} /> : result ? <p className="text-sm text-fg-2">No score: no landing on the challenge's runway after its gate.</p> : null}
          <Timeline log={attempt.log} />
        </div>
      ) : null}
    </li>
  );
}

function Checks({ result }: { result: AttemptResult }) {
  return (
    <div className="min-w-0">
      <h3 className="eyebrow mb-2">Checks</h3>
      <ul className="flex flex-col gap-1.5">
        {result.checks.map((check) => (
          <li key={check.name} className="grid grid-cols-[auto_minmax(0,1fr)] items-start gap-2 text-sm">
            {check.ok ? <Check size={15} className="mt-0.5" style={{ color: "var(--success)" }} aria-label="Passed" /> : <X size={15} className="mt-0.5" style={{ color: "var(--danger)" }} aria-label="Failed" />}
            <span className="min-w-0"><span className={check.ok ? "" : "font-medium"}>{words(check.name, CHECK)}</span> <span className="text-fg-3">{check.detail}</span></span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function ScoreParts({ score }: { score: NonNullable<AttemptResult["score"]> }) {
  return (
    <div className="min-w-0">
      <h3 className="eyebrow mb-2">Score · {score.version}</h3>
      <ul className="flex flex-col gap-2.5">
        {score.parts.map((part) => (
          <li key={part.name} className="text-sm">
            <div className="flex items-baseline justify-between gap-2">
              <span>{words(part.name, PART)}{part.value !== null ? <span className="readout text-xs text-fg-3"> · {measured(part.value, part.unit || undefined)}</span> : null}</span>
              <span className="readout text-xs text-fg-2">{number(part.weight * part.score, 1)} / {part.weight}</span>
            </div>
            <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-muted" role="meter" aria-valuemin={0} aria-valuemax={part.weight} aria-valuenow={part.weight * part.score} aria-label={words(part.name, PART)}>
              <div className="h-full rounded-full" style={{ width: `${part.score * 100}%`, background: part.score >= 0.8 ? "var(--success)" : part.score >= 0.4 ? "var(--warning)" : "var(--danger)" }} />
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** A log line in words. Unknown events from a newer app show their name. */
function describe(line: AttemptLogLine): string {
  const f = line as Record<string, unknown>;
  const n = (key: string, unit: string) => (typeof f[key] === "number" ? measured(f[key] as number, unit) : "?");
  switch (line.event) {
    case "created": return `Created, definition ${String(f.definition_sha256 ?? "").slice(0, 12)}, app ${String(f.app ?? "?")}`;
    case "placing": return "Placing the aircraft";
    case "placed": {
      const start = f.start as { alt_ft?: number; ias_kt?: number } | string | undefined;
      const where = typeof start === "object" && start ? ` at ${measured(start.alt_ft ?? 0, "ft")}, ${measured(start.ias_kt ?? 0, "kt")}` : "; fly to the gate yourself";
      return `Started in ${String(f.flight_id ?? "?")}${where}`;
    }
    case "failed_start": return `Could not start: ${String(f.error ?? "")}`;
    case "gate": return `Gate passed at ${n("sim_s", "s")} sim time`;
    case "touchdown": return `Touchdown, ${n("sink_fpm", "fpm")}, ${n("peak_g", "g")}`;
    case "finished": return `Finished: ${reason(f.reason)}`;
    case "graded_local": {
      const failed = Array.isArray(f.failed_checks) ? (f.failed_checks as string[]) : [];
      const score = typeof f.score === "number" ? `score ${number(f.score, 1)}` : "no score";
      return `Graded here: ${score}, ${f.valid ? "counts" : `doesn't count${failed.length ? ` (${failed.map((c) => words(c, CHECK).toLowerCase()).join(", ")})` : ""}`}`;
    }
    case "sliced": return "Recording slice cut";
    case "done": return "Done";
    case "abandoned": return `Abandoned: ${reason(f.reason)}`;
    case "error": return `Error while ${String(f.during ?? "working")}: ${String(f.message ?? "")}`;
    default: return line.event.replace(/_/g, " ");
  }
}

const time = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", second: "2-digit", timeZone: "UTC" });

function Timeline({ log }: { log: AttemptLogLine[] }) {
  return (
    <div className="min-w-0">
      <h3 className="eyebrow mb-2">Log</h3>
      <ol className="relative flex flex-col gap-2 border-l border-line pl-4">
        {log.map((line) => (
          <li key={line.seq} className="relative text-sm">
            <span className="absolute top-1.5 -left-[1.3rem] size-2 rounded-full" style={{ background: line.event === "error" || line.event === "failed_start" ? "var(--danger)" : line.event === "graded_local" ? "var(--accent)" : "var(--text-muted)" }} aria-hidden="true" />
            <span className="readout mr-2 text-xs text-fg-3">{time.format(new Date(line.at))}Z</span>
            {describe(line)}
          </li>
        ))}
      </ol>
    </div>
  );
}
