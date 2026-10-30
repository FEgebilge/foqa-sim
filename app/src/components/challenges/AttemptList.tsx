import { useState } from "react";
import { Check, ChevronRight, ExternalLink, History, PlaneLanding, RefreshCw, ServerCrash, X } from "lucide-react";
import { Badge, type BadgeTone } from "@ui/components/badge/badge";
import { Button } from "@ui/components/button/button";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import { api } from "../../api";
import { partLabel } from "../../lib/challenge-flow";
import { openFlight } from "../../lib/route";
import { RETRYABLE, SITE_STATE, type SiteAttempt } from "../../lib/sites";
import { flightDate, flightTimeUtc, GRADE_LABEL, measured, number } from "../../lib/format";
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
  crashed: "crashed: flying again from the clean start",
  flight_closed: "the flight closed",
  recovered: "recovered after the app restarted",
  interrupted: "the app closed mid-attempt",
  replaced: "flown again",
  cancelled: "cancelled",
  no_marker: "the recording has no marker for this attempt",
  no_flight: "its flight is gone",
};
const reason = (r: unknown) => (typeof r === "string" ? REASON[r] ?? r : "");

const CHECK: Record<string, string> = {
  aircraft: "Aircraft", runway: "Runway", gate: "Gate", sim_rate: "Sim rate", slew: "Slew", pause: "Pause",
  continuity: "Continuous data", wind: "Wind", crosswind: "Crosswind", tailwind: "Tailwind", autopilot: "Autopilot",
  stop_on_runway: "Stopped on the runway", go_around: "No go-around", data: "Recorded data",
  ground_strike: "No wing or nose strike",
};
const words = (name: string, known: Record<string, string>) => known[name] ?? name.replace(/_/g, " ");
const sentence = (text: string) => `${text[0]!.toUpperCase()}${text.slice(1)}`;

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

/** Attempts still loading, or not readable: the list says so instead of showing "No attempts yet". */
export type AttemptsState = { loading: true } | { error: string; retry: () => void };

/** The attempts at one challenge, newest first; each opens to its checks, score parts and log. */
export function AttemptList({ attempts, sites, state }: { attempts: AttemptView[]; sites?: Map<string, SiteAttempt>; state?: AttemptsState }) {
  return (
    <section aria-label="Attempts" className="flex flex-col gap-2">
      <div className="flex items-baseline justify-between gap-3">
        <h2 className="font-display text-sm font-semibold">Attempts</h2>
        <span className="readout text-xs text-fg-3">{attempts.length ? `${attempts.length} on this PC` : ""}</span>
      </div>
      {state && "loading" in state ? <div className="panel h-20 animate-pulse" aria-label="Loading attempts" /> : state ? (
        <div className="panel"><EmptyState size="sm" icon={<ServerCrash size={18} />} title="Could not read the attempts" description={state.error} action={<Button variant="secondary" size="sm" onClick={state.retry}>Try again</Button>} /></div>
      ) : attempts.length ? (
        <ul className="panel divide-y divide-line overflow-hidden">
          {attempts.map((attempt, i) => <AttemptRow key={attempt.id} attempt={attempt} site={sites?.get(attempt.id)} initiallyOpen={i === 0 && attempt.result !== null} />)}
        </ul>
      ) : (
        <div className="panel"><EmptyState size="sm" icon={<History size={18} />} title="No attempts yet" description="Each attempt is logged step by step and graded from the recorded flight." /></div>
      )}
    </section>
  );
}

function AttemptRow({ attempt, site, initiallyOpen }: { attempt: AttemptView; site?: SiteAttempt; initiallyOpen: boolean }) {
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
          {site ? <Badge size="sm" tone={SITE_STATE[site.state]?.tone ?? "info"}>{SITE_STATE[site.state]?.label ?? "Site"}</Badge> : null}
        </span>
        <span className="readout text-right">
          <span className="text-xl">{result?.score ? number(result.score.total, 1) : "–"}</span>
          {result?.score ? <span className="text-xs text-fg-3"> / 100</span> : null}
        </span>
      </button>
      {open ? (
        <div className="grid gap-5 border-t border-line-subtle bg-[color-mix(in_oklab,var(--surface-muted)_40%,transparent)] px-4 py-4 lg:grid-cols-2 2xl:grid-cols-3">
          {site ? <SitePart site={site} /> : null}
          {result ? <Checks result={result} /> : null}
          {result?.score ? <ScoreParts score={result.score} assessment={result.assessment} /> : result ? <p className="text-sm text-fg-2">No score: no landing on the challenge's runway after its gate.</p> : null}
          {attempt.flight_id && result ? <div className="lg:col-span-2 2xl:col-span-3"><Button variant="secondary" size="sm" onClick={() => openFlight(attempt.flight_id!)}><PlaneLanding size={14} aria-hidden="true" />Open the debrief</Button></div> : null}
          <Timeline log={site ? [...attempt.log, ...site.log.map((l) => ({ ...l, site: true }))].sort((a, b) => a.at.localeCompare(b.at)) : attempt.log} />
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

function ScoreParts({ score, assessment }: { score: NonNullable<AttemptResult["score"]>; assessment?: AttemptResult["assessment"] }) {
  return (
    <div className="min-w-0">
      <h3 className="eyebrow mb-2">Score · {score.version}</h3>
      <ul className="flex flex-col gap-2.5">
        {score.parts.map((part) => (
          <li key={part.name} className="text-sm">
            <div className="flex items-baseline justify-between gap-2">
              <span>{sentence(partLabel(score.version, part.name))}{part.value !== null ? <span className="readout text-xs text-fg-3"> · {measured(part.value, part.unit || undefined)}</span> : null}</span>
              <span className="readout text-xs text-fg-2">{number(part.weight * part.score, 1)} / {number(part.weight, 1)}</span>
            </div>
            <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-muted" role="meter" aria-valuemin={0} aria-valuemax={part.weight} aria-valuenow={part.weight * part.score} aria-label={sentence(partLabel(score.version, part.name))}>
              <div className="h-full rounded-full" style={{ width: `${part.score * 100}%`, background: part.score >= 0.8 ? "var(--success)" : part.score >= 0.4 ? "var(--warning)" : "var(--danger)" }} />
            </div>
          </li>
        ))}
      </ul>
      {assessment ? (
        <div className="mt-3 text-sm">
          <p><span className="font-medium">{GRADE_LABEL[assessment.grade]}</span> <span className="text-fg-3">· {assessment.confidence === "partial" ? "partial data" : "full data"}</span></p>
          {assessment.caps.length ? <ul className="mt-1 flex flex-col gap-1 text-xs text-fg-2">{assessment.caps.map((cap, i) => <li key={i}><span className="font-medium">{cap.title}.</span> {cap.reason}</li>)}</ul> : null}
        </div>
      ) : null}
    </div>
  );
}

/** The attempt on its site: the app's own words for its state, the site's message, its page and a retry. */
function SitePart({ site }: { site: SiteAttempt }) {
  const [retrying, setRetrying] = useState(false);
  const retry = () => {
    setRetrying(true);
    api.attemptUpload(site.id).finally(() => setRetrying(false));
  };
  return (
    <div className="min-w-0 lg:col-span-2 2xl:col-span-3">
      <h3 className="eyebrow mb-2">Site</h3>
      <p className="text-sm">{site.text} <span className="readout text-xs text-fg-3">· {site.site}</span></p>
      {site.detail ? <details className="mt-1 text-xs text-fg-3"><summary className="cursor-pointer">Details</summary><p className="readout mt-1 break-all">{site.detail}</p></details> : null}
      <div className="mt-2 flex flex-wrap gap-2">
        {RETRYABLE.includes(site.state) ? <Button variant="secondary" size="sm" onClick={retry} loading={retrying}><RefreshCw size={14} aria-hidden="true" />{site.state.startsWith("upload") ? "Upload now" : "Check now"}</Button> : null}
        {site.url ? <Button variant="ghost" size="sm" onClick={() => api.siteOpen(site.url!)}><ExternalLink size={14} aria-hidden="true" />View on site</Button> : null}
      </div>
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
      const start = f.start as { alt_ft?: number; ias_kt?: number; nav1?: string } | string | undefined;
      const where = typeof start === "object" && start ? ` at ${measured(start.alt_ft ?? 0, "ft")}, ${measured(start.ias_kt ?? 0, "kt")}${start.nav1 ? `; ${start.nav1}` : ""}` : "; fly to the gate yourself";
      return `Started in ${String(f.flight_id ?? "?")}${where}`;
    }
    case "failed_start": return `Could not start: ${String(f.error ?? "")}`;
    // Saved after the first placement, reloaded before Fly again, or why not; in words already.
    case "clean_start": return String(f.text ?? "");
    case "gate": return `Gate passed at ${n("sim_s", "s")} sim time`;
    // A wing, propeller or nose on the runway, told live (rules::strike); in words already.
    case "strike": return String(f.text ?? "Ground strike");
    case "touchdown": return `Touchdown, ${n("sink_fpm", "fpm")}, ${n("peak_g", "g")}`;
    case "finished": return `Finished: ${reason(f.reason)}`;
    case "graded_local": {
      const failed = Array.isArray(f.failed_checks) ? (f.failed_checks as string[]) : [];
      const score = typeof f.score === "number" ? `score ${number(f.score, 1)}` : "no score";
      return `Graded here: ${score}, ${f.valid ? "counts" : `doesn't count${failed.length ? ` (${failed.map((c) => words(c, CHECK).toLowerCase()).join(", ")})` : ""}`}`;
    }
    case "sliced": return "Recording slice cut";
    case "done": return f.slice_failed ? `Done without a recording slice: ${reason(f.slice_failed)}` : "Done";
    case "abandoned": return `Abandoned: ${reason(f.reason)}`;
    case "error": return `Error while ${String(f.during ?? "working")}: ${String(f.message ?? "")}`;
    // The site's log (site.log.jsonl), merged in by time.
    case "issued": return `Issued by the site, attempt ${String(f.attempt_id ?? "?")}${f.definition_match === false ? ", definition differs" : ""}`;
    case "practice": return `Practice, not uploaded (${String(f.reason ?? "no attempt issued")})`;
    case "upload_queued": return "Upload queued";
    case "abandon_queued": return `Abandoning on the site${f.reason ? `: ${reason(f.reason)}` : ""}`;
    case "upload": return typeof f.status === "string" ? `Uploaded, try ${String(f.try)}: ${f.status}` : `Upload try ${String(f.try)} failed: ${String(f.error ?? "?")}${typeof f.retry_in_s === "number" ? `, retry in ${f.retry_in_s} s` : ""}`;
    case "server_status": return `Site: ${String(f.status)}${typeof f.score === "number" ? `, ${number(f.score, 1)}` : ""}${typeof f.rank === "number" ? `, #${f.rank}` : ""}`;
    case "upload_refused": return `Upload refused (${String(f.reason ?? "by the site")})`;
    case "upload_abandoned": return "Not uploaded: signed in as another account";
    case "expired": return "Upload window closed";
    default: return line.event.replace(/_/g, " ");
  }
}

const time = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", second: "2-digit", timeZone: "UTC" });

function Timeline({ log }: { log: AttemptLogLine[] }) {
  return (
    <div className="min-w-0">
      <h3 className="eyebrow mb-2">Log</h3>
      <ol className="relative flex flex-col gap-2 border-l border-line pl-4">
        {log.map((line, i) => (
          <li key={i} className="relative text-sm">
            <span className="absolute top-1.5 -left-[1.3rem] size-2 rounded-full" style={{ background: line.event === "error" || line.event === "failed_start" || line.event === "strike" ? "var(--danger)" : line.event === "graded_local" ? "var(--accent)" : "var(--text-muted)" }} aria-hidden="true" />
            <span className="readout mr-2 text-xs text-fg-3">{time.format(new Date(line.at))}Z</span>
            {describe(line)}
          </li>
        ))}
      </ol>
    </div>
  );
}
