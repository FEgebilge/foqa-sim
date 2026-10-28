import { useCallback, useEffect, useMemo, useState } from "react";
import { CircleCheck, CircleDashed, CircleX, FileUp, Flag, PlaneLanding, RefreshCw, RotateCcw, SearchX, ServerCrash, Trash2, TriangleAlert, Trophy, X } from "lucide-react";
import { Badge } from "@ui/components/badge/badge";
import { Button } from "@ui/components/button/button";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import { SearchField } from "@ui/components/search-field/search-field";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { api } from "../../api";
import { useNotify } from "../../lib/context";
import { flightDate, number } from "../../lib/format";
import { challengeHref } from "../../lib/route";
import type { RecorderState } from "../../lib/use-backend";
import { useAttempts, useChallenges, useLoad } from "../../lib/use-backend";
import type { AttemptView, Briefing, ChallengeDefinition, ChallengeEntry, PreflightLine } from "../../types";
import { ConfirmDialog } from "../ConfirmDialog";
import { OPEN_STATES } from "../../lib/attempts";
import { useVisible } from "../../lib/visible";
import { type SiteAttempt, useConnect, useSiteAttempts, useSites } from "../../lib/sites";
import { AttemptList, Stages } from "./AttemptList";
import { SitesPanel } from "./SitesPanel";

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
/** The pre-flight card is measured again this often while shown; each check analyses the flight being recorded. */
const PREFLIGHT_EVERY_MS = 10_000;

/** Landing challenges, local: the library of challenge files, a briefing with its pre-flight card, and the attempts. */
export function ChallengesScreen({ selected, recorder }: { selected?: string; recorder: RecorderState | null }) {
  const challenges = useChallenges();
  const attempts = useAttempts();
  const notify = useNotify();
  const connect = useConnect();
  const sites = useSites(connect);
  const siteAttempts = useSiteAttempts(connect);
  const [importing, setImporting] = useState(false);
  const [doomed, setDoomed] = useState<ChallengeEntry | null>(null);
  const entries = useMemo(() => (challenges.state.status === "ready" ? challenges.state.data : []), [challenges.state]);
  const key = selected && entries.some((e) => e.key === selected) ? selected : entries[0]?.key;

  const openFile = () => {
    setImporting(true);
    api.challengeImport()
      .then((entry) => {
        if (!entry) return;
        challenges.refresh();
        window.location.hash = challengeHref(entry.key).slice(1);
        notify({ title: `Added “${entry.title}”`, body: `${entry.airport} ${entry.runway}, version ${entry.version}` });
      }, (error: unknown) => notify({ tone: "error", title: "Could not open the challenge file", body: message(error) }))
      .finally(() => setImporting(false));
  };
  const remove = () => {
    if (!doomed) return;
    api.challengeRemove(doomed.key)
      .then(challenges.refresh, (error: unknown) => notify({ tone: "error", title: "Could not remove the challenge", body: message(error) }))
      .finally(() => setDoomed(null));
  };
  const openButton = <Button variant="primary" size="sm" onClick={openFile} loading={importing}><FileUp size={15} aria-hidden="true" />Open challenge file…</Button>;

  let body;
  if (challenges.state.status === "loading") body = <div className="panel flex flex-col gap-3 p-4" aria-label="Loading challenges">{Array.from({ length: 4 }, (_, i) => <div key={i} className="h-12 animate-pulse rounded-md bg-muted" />)}</div>;
  else if (challenges.state.status === "error") body = <div className="panel"><EmptyState icon={<ServerCrash size={20} />} title="Could not read the challenges" description={challenges.state.message} action={<Button variant="secondary" size="sm" onClick={challenges.refresh}>Try again</Button>} /></div>;
  else if (!entries.length || !key) {
    body = (
      <div className="panel">
        <EmptyState icon={<Trophy size={20} />} title="No challenges yet"
          description="A challenge is a JSON file: an airport, a runway, an aircraft and the conditions everyone flies. Open one to see its briefing, place the aircraft at its start and get graded here."
          action={openButton} />
      </div>
    );
  } else {
    const views = attempts.state.status === "ready" ? attempts.state.data : [];
    body = (
      <div className="grid items-start gap-5 lg:grid-cols-[minmax(15rem,19rem)_minmax(0,1fr)] 2xl:grid-cols-[minmax(17rem,22rem)_minmax(0,1fr)]">
        <Library entries={entries} selected={key} attempts={views} onRemove={setDoomed} />
        <ChallengeDetail key={key} challengeKey={key} attempts={views.filter((a) => a.key === key)} sites={siteAttempts} recorder={recorder} onChanged={attempts.refresh} />
      </div>
    );
  }

  return (
    <main className="flex w-full flex-col gap-5 px-6 py-6 2xl:px-8">
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <p className="eyebrow">Landing challenges</p>
          <h1 className="mt-1 font-display text-2xl font-semibold tracking-tight">Challenges</h1>
          <p className="mt-1 text-sm text-fg-2">One landing, flown the same way every time: brief, check, place the aircraft at the start, land, and see whether it counts.</p>
        </div>
        {entries.length ? openButton : null}
      </header>
      {connect ? <SitesPanel sites={sites.data} onChanged={() => { sites.refresh(); challenges.refresh(); }} /> : null}
      {body}
      <ConfirmDialog open={doomed !== null} title="Remove this challenge?" confirmLabel="Remove challenge" onConfirm={remove} onCancel={() => setDoomed(null)}>
        {doomed ? `“${doomed.title}” (${doomed.airport} ${doomed.runway}, version ${doomed.version}) leaves the library. ` : ""}Its attempts and their logs stay. Open the file again to fly it.
      </ConfirmDialog>
    </main>
  );
}

function Library({ entries, selected, attempts, onRemove }: { entries: ChallengeEntry[]; selected: string; attempts: AttemptView[]; onRemove: (entry: ChallengeEntry) => void }) {
  const [query, setQuery] = useState("");
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const visible = entries.filter((e) => words.every((w) => `${e.title} ${e.airport} ${e.runway} ${e.id}`.toLowerCase().includes(w)));
  const best = (key: string) => attempts.filter((a) => a.key === key && a.result?.valid).reduce<number | null>((top, a) => Math.max(top ?? 0, a.result?.score?.total ?? 0), null);

  return (
    <section aria-label="Challenge library" className="flex flex-col gap-2 lg:sticky lg:top-20">
      <div className="flex items-baseline justify-between gap-3">
        <h2 className="font-display text-sm font-semibold">Library</h2>
        <span className="readout text-xs text-fg-3">{visible.length === entries.length ? `${entries.length} challenge${entries.length > 1 ? "s" : ""}` : `${visible.length} of ${entries.length}`}</span>
      </div>
      {entries.length > 5 ? <SearchField label="Search challenges" hideLabel size="sm" placeholder="Title, airport or runway" value={query} onValueChange={setQuery} /> : null}
      {visible.length ? (
        <ul className="panel divide-y divide-line overflow-hidden">
          {visible.map((entry) => {
            const current = entry.key === selected;
            const top = best(entry.key);
            return (
              <li key={entry.key} className={`group relative flex items-center ${current ? "bg-muted" : "hover:bg-muted"}`} style={current ? { boxShadow: "inset 3px 0 0 var(--accent)" } : undefined}>
                <a href={challengeHref(entry.key)} aria-current={current ? "page" : undefined}
                  className={`min-w-0 flex-1 px-4 py-3 no-underline ${current ? "text-fg" : "text-fg-2 hover:text-fg"}`}>
                  <span className="block truncate text-sm font-medium">{entry.title}</span>
                  <span className="readout mt-0.5 flex items-center gap-2 text-xs text-fg-3">
                    <span>{entry.airport} {entry.runway}</span><span>v{entry.version}</span>
                    {top !== null ? <span className="ml-auto text-fg-2">best {number(top, 1)}</span> : null}
                  </span>
                </a>
                <Tooltip content="Remove from the library" side="left">
                  <button type="button" className="mr-2 shrink-0 rounded p-1.5 text-fg-3 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 hover:bg-muted hover:text-danger focus-visible:opacity-100" aria-label={`Remove ${entry.title}`} onClick={() => onRemove(entry)}>
                    <Trash2 size={14} aria-hidden="true" />
                  </button>
                </Tooltip>
              </li>
            );
          })}
        </ul>
      ) : (
        <div className="panel"><EmptyState size="sm" icon={<SearchX size={18} />} title="No challenges match" description="Change the search." action={<Button variant="secondary" size="sm" onClick={() => setQuery("")}>Clear search</Button>} /></div>
      )}
    </section>
  );
}

function ChallengeDetail({ challengeKey, attempts, sites, recorder, onChanged }: { challengeKey: string; attempts: AttemptView[]; sites: Map<string, SiteAttempt>; recorder: RecorderState | null; onChanged: () => void }) {
  const { lines, checking, check } = usePreflight(challengeKey, recorder?.value.state);
  const load = useCallback(() => api.challengeBriefing(challengeKey), [challengeKey]);
  const { state } = useLoad<Briefing>(load);

  if (state.status === "loading") return <div className="panel h-64 animate-pulse" aria-label="Loading the briefing" />;
  if (state.status === "error") return <div className="panel"><EmptyState icon={<ServerCrash size={20} />} title="Could not read this challenge" description={state.message} /></div>;
  const briefing = state.data;
  return (
    <div className="flex min-w-0 flex-col gap-5">
      <div className="grid items-start gap-5 xl:grid-cols-[minmax(0,3fr)_minmax(20rem,2fr)]">
        <BriefingCard briefing={briefing} />
        <div className="flex flex-col gap-5">
          <PreflightCard lines={lines} checking={checking} onCheck={check} />
          <Actions briefing={briefing} attempts={attempts} simReady={lines?.find((l) => l.name === "sim")?.state === "ok"} ranking={lines?.find((l) => l.name === "ranking")} onChanged={onChanged} />
        </div>
      </div>
      <AttemptList attempts={attempts} sites={sites} />
    </div>
  );
}

/** The pre-flight card, measured on open, when the recorder's state changes and every `PREFLIGHT_EVERY_MS`. */
function usePreflight(key: string, recorder: string | undefined) {
  // The recorder's state is part of the load, so a change measures again.
  const measure = useCallback((): Promise<PreflightLine[]> => api.challengePreflight(key)
    .catch((error: unknown) => [{ name: "sim", state: "unknown", text: `Could not check: ${message(error)}` }]), [key, recorder]); // eslint-disable-line react-hooks/exhaustive-deps
  const { state, reload, refresh, refreshing } = useLoad(measure);
  // Each measure analyses the flight being recorded: not while the window is hidden.
  const visible = useVisible();
  useEffect(() => {
    if (!visible) return;
    const timer = window.setInterval(reload, PREFLIGHT_EVERY_MS);
    return () => window.clearInterval(timer);
  }, [reload, visible]);
  return { lines: state.status === "ready" ? state.data : null, checking: refreshing, check: refresh };
}

const pad3 = (deg: number) => String(Math.round(deg)).padStart(3, "0");
const band = ({ min, max }: { min?: number; max?: number }) =>
  min !== undefined && max !== undefined ? `${min} to ${max} kt` : max !== undefined ? `up to ${max} kt` : min !== undefined ? `at least ${min} kt` : "any";

/** The definition in plain words, fact by fact. */
function facts(d: ChallengeDefinition): { label: string; value: string; note?: string }[] {
  const { start, conditions = {}, rules = {}, aircraft = {} } = d;
  const types = aircraft.types ?? [], classes = aircraft.classes ?? [];
  const list: { label: string; value: string; note?: string }[] = [
    { label: "Start", value: `${start.distance_nm} NM out, ${number(start.height_ft)} ft`, note: `${start.ias_kt} kt, gear ${start.gear_down ? "down" : "up"}, flaps ${Math.round((start.flaps ?? 0) * 100)} %. ${start.mode === "teleport" ? "The app places you there." : "Fly there yourself."}` },
    { label: "Gate", value: `${d.gate.distance_nm} NM`, note: "Grading starts here, so settling after the start never counts." },
    { label: "Aircraft", value: types.length ? types.join(" or ") : classes.length ? classes.map((c) => `${c[0]!.toUpperCase()}${c.slice(1)}`).join(" or ") : "Any", note: types.length && classes.length ? `${classes.join(" or ")} class` : undefined },
  ];
  if (conditions.wind) {
    const w = conditions.wind;
    list.push({ label: "Wind", value: `${pad3(w.from_deg)}° ${w.speed_kt} kt`, note: `±${w.tolerance_deg}°, ±${w.tolerance_kt} kt, mean from the gate to touchdown` });
  }
  if (conditions.crosswind_kt) list.push({ label: "Crosswind", value: band(conditions.crosswind_kt), note: "At touchdown, on the runway" });
  if (conditions.tailwind_kt) list.push({ label: "Tailwind", value: band(conditions.tailwind_kt), note: "At touchdown, on the runway" });
  if (conditions.preset) list.push({ label: "Weather preset", value: conditions.preset });
  const ruleText = [
    rules.ap_off_by_height_ft !== undefined ? `Autopilot off below ${number(rules.ap_off_by_height_ft)} ft` : null,
    rules.stop_on_runway ? "Slow to taxi speed on the runway" : null,
  ].filter(Boolean);
  list.push({ label: "Rules", value: ruleText.length ? ruleText.join(" · ") : "Integrity only", note: "Always: sim rate 1, no pause, no slew, continuous data." });
  if (d.window) list.push({ label: "Open", value: `${flightDate(d.window.from)} to ${flightDate(d.window.to)}`, note: "Enforced by a site; local attempts can be flown any time." });
  list.push({ label: "Scoring", value: d.scoring.version, note: "0 to 100: touchdown point, sink rate, centreline, stability, peak g, bounce, rollout." });
  return list;
}

function BriefingCard({ briefing }: { briefing: Briefing }) {
  const d = briefing.definition;
  return (
    <section className="panel min-w-0 overflow-hidden" aria-labelledby="briefing-title">
      <div className="flex flex-wrap items-start justify-between gap-3 border-b border-line px-5 py-4">
        <div className="min-w-0">
          <p className="eyebrow">Briefing · {d.airport} runway {d.runway}</p>
          <h2 id="briefing-title" className="mt-1 font-display text-lg font-semibold tracking-tight">{d.title}</h2>
        </div>
        <div className="flex flex-wrap gap-1.5">
          <Badge size="sm">v{d.version}</Badge>
          <Badge size="sm" tone="info">{d.start.mode === "teleport" ? "Placed on final" : "Self-flown"}</Badge>
        </div>
      </div>
      <dl className="grid grid-cols-1 gap-px bg-line sm:grid-cols-2 2xl:grid-cols-3">
        {facts(d).map((fact) => (
          <div key={fact.label} className="bg-surface px-5 py-3">
            <dt className="eyebrow">{fact.label}</dt>
            <dd className="readout mt-1 text-base">{fact.value}</dd>
            {fact.note ? <dd className="mt-0.5 text-xs text-fg-3">{fact.note}</dd> : null}
          </div>
        ))}
      </dl>
      <p className="readout border-t border-line px-5 py-2.5 text-[11px] break-all text-fg-3" title="SHA-256 of the definition file, logged with every attempt">{d.id} · sha256 {briefing.sha256}</p>
    </section>
  );
}

const LINE_STATE: Record<PreflightLine["state"], { color: string; Icon: typeof CircleCheck; label: string }> = {
  ok: { color: "var(--success)", Icon: CircleCheck, label: "OK" },
  warn: { color: "var(--warning)", Icon: TriangleAlert, label: "Warn" },
  fail: { color: "var(--danger)", Icon: CircleX, label: "Fail" },
  unknown: { color: "var(--text-muted)", Icon: CircleDashed, label: "Pending" },
};
const LINE_TITLE: Record<PreflightLine["name"], string> = { sim: "Simulator", aircraft: "Aircraft", weather: "Weather", ranking: "Ranking" };

function PreflightCard({ lines, checking, onCheck }: { lines: PreflightLine[] | null; checking: boolean; onCheck: () => void }) {
  const failing = lines?.filter((l) => l.state === "fail").length ?? 0;
  return (
    <section className="panel overflow-hidden" aria-labelledby="preflight-title">
      <div className="flex items-center justify-between gap-3 border-b border-line px-4 py-3">
        <div>
          <h2 id="preflight-title" className="font-display text-sm font-semibold">Pre-flight check</h2>
          <p className="text-xs text-fg-3">{!lines ? "Checking…" : failing ? `${failing} to fix before the attempt counts` : "Ready to fly"}</p>
        </div>
        <Button variant="ghost" size="sm" onClick={onCheck} loading={checking}><RefreshCw size={14} aria-hidden="true" />Re-check</Button>
      </div>
      {lines ? (
        <ul className="divide-y divide-line">
          {lines.map((line) => {
            const { color, Icon, label } = LINE_STATE[line.state];
            return (
              <li key={line.name} className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 px-4 py-3" style={{ boxShadow: `inset 3px 0 0 ${color}` }}>
                <Icon size={17} className="mt-0.5" style={{ color }} aria-hidden="true" />
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-x-2">
                    <h3 className="text-sm font-medium">{LINE_TITLE[line.name] ?? line.name}</h3>
                    <span className="readout text-[11px] font-semibold tracking-wider uppercase" style={{ color }}>{label}</span>
                  </div>
                  <p className="mt-0.5 text-sm text-fg-2">{line.text}</p>
                  {line.fix ? <p className="mt-1 text-sm text-fg"><span className="font-medium">Fix: </span>{line.fix}</p> : null}
                </div>
              </li>
            );
          })}
        </ul>
      ) : <div className="flex flex-col gap-2 p-4">{Array.from({ length: 4 }, (_, i) => <div key={i} className="h-10 animate-pulse rounded-md bg-muted" />)}</div>}
    </section>
  );
}

/** Place, the attempt in progress (Finish, Fly again, Abandon), or Fly again after the last one. */
function Actions({ briefing, attempts, simReady, ranking, onChanged }: { briefing: Briefing; attempts: AttemptView[]; simReady: boolean; ranking?: PreflightLine; onChanged: () => void }) {
  const notify = useNotify();
  const [pending, setPending] = useState<string | null>(null);
  const last = attempts[0];
  const open = last && OPEN_STATES.includes(last.state) ? last : undefined;
  const self = briefing.definition.start.mode === "self";
  const run = (name: string, call: () => Promise<unknown>, failed: string) => {
    setPending(name);
    call().catch((error: unknown) => notify({ tone: "error", title: failed, body: message(error) })).finally(() => { setPending(null); onChanged(); });
  };
  const place = () => run("place", () => (last ? api.attemptFlyAgain(last.id) : api.attemptStart(briefing.key)), self ? "Could not start the attempt" : "Could not place the aircraft");
  const busy = pending !== null;

  return (
    <section className="panel flex flex-col gap-3 p-4" aria-label="Attempt">
      {open ? (
        <>
          <div className="flex items-baseline justify-between gap-3">
            <h2 className="font-display text-sm font-semibold">Attempt {open.n} in progress</h2>
            <span className="readout text-xs text-fg-3">{open.flight_id ?? ""}</span>
          </div>
          <Stages attempt={open} />
          <p className="text-xs text-fg-3">{open.state === "rollout" ? "Landed. It's graded when the rollout ends (2 min), or now with Finish." : open.state === "finished" ? "Grading from the recorded flight…" : self ? "Fly to the gate yourself; the landing after it is graded." : "Land on the runway; the landing after the start is graded."}</p>
          <div className="flex flex-wrap gap-2">
            <Button variant="primary" size="sm" onClick={() => run("finish", () => api.attemptFinish(open.id), "Could not grade the attempt")} loading={pending === "finish"} disabled={busy && pending !== "finish"}>
              <Flag size={15} aria-hidden="true" />Finish and grade
            </Button>
            <Button variant="secondary" size="sm" onClick={place} loading={pending === "place"} disabled={!simReady || (busy && pending !== "place")}>
              <RotateCcw size={15} aria-hidden="true" />Fly again
            </Button>
            {open.state !== "finished" ? (
              <Button variant="ghost" size="sm" onClick={() => run("abandon", () => api.attemptAbandon(open.id), "Could not abandon the attempt")} loading={pending === "abandon"} disabled={busy && pending !== "abandon"}>
                <X size={15} aria-hidden="true" />Abandon
              </Button>
            ) : null}
          </div>
        </>
      ) : (
        <>
          <div>
            <h2 className="font-display text-sm font-semibold">{last ? `Attempt ${last.n + 1}` : "First attempt"}</h2>
            <p className="mt-0.5 text-xs text-fg-3">{ranking && !ranking.text.startsWith("Local") ? ranking.text : "Local practice: graded here from the recording, never uploaded."}</p>
          </div>
          <Button variant="primary" onClick={place} loading={pending === "place"} disabled={!simReady || busy}>
            {last ? <RotateCcw size={16} aria-hidden="true" /> : <PlaneLanding size={16} aria-hidden="true" />}
            {last ? "Fly again" : self ? "Start attempt" : "Place aircraft"}
          </Button>
          {!simReady ? <p className="text-xs text-fg-3">Available once MSFS is connected with a flight loaded.</p> : null}
        </>
      )}
    </section>
  );
}
