import type { ReactNode } from "react";
import { ArrowLeft, ArrowRight, ChevronLeft, ChevronRight, FolderOpen, RefreshCw, ShieldAlert } from "lucide-react";
import { Badge } from "@ui/components/badge/badge";
import { Button } from "@ui/components/button/button";
import { InlineEdit } from "@ui/components/inline-edit/inline-edit";
import { TagInput } from "@ui/components/tag-input/tag-input";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import type { FlightMeta } from "../../api";
import { aircraftName, assessmentText, duration, flightDate, flightTimeUtc, number } from "../../lib/format";
import { useNotify } from "../../lib/context";
import { openFlight, openFlights } from "../../lib/route";
import type { Debrief, FlightSummary } from "../../types";
import { AssessmentBreakdown } from "./AssessmentBreakdown";
import { EventCounts, GradeBadge, GradeTag } from "../level";
import { Term } from "../Term";

interface Props {
  flight: Debrief["flight"];
  refreshing: boolean;
  onRefresh: () => void;
  onOpenFolder: () => void;
  /** The Export/Share menu. */
  exportMenu?: ReactNode;
  /** Banners that change how the score reads (cut-off recording, unidentified aircraft); they sit above it. */
  notices?: ReactNode;
  /** The flights before and after this one in time. */
  older?: FlightSummary;
  newer?: FlightSummary;
  /** The pilot's title, notes and tags, and how to save a change to them. */
  meta: FlightMeta;
  onMeta: (change: Partial<FlightMeta>) => Promise<unknown>;
}

export function DebriefHeader({ flight, refreshing, onRefresh, onOpenFolder, exportMenu, notices, older, newer, meta, onMeta }: Props) {
  const facts: [string, ReactNode][] = [
    ["Date", <>{flightDate(flight.started_utc)} <span className="text-fg-3">{flightTimeUtc(flight.started_utc)}</span></>],
    ["Duration", duration(flight.duration_s)],
    ["Landings", String(flight.landings)],
    ["Profile", flight.profile],
  ];
  return (
    <header className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <nav className="flex flex-wrap items-center gap-1" aria-label="Flights">
          <Button variant="ghost" size="sm" onClick={openFlights}><ArrowLeft size={15} aria-hidden="true" />All flights</Button>
          <Step flight={older} label="Older flight" keyName="J" icon={<ChevronLeft size={15} aria-hidden="true" />} />
          <Step flight={newer} label="Newer flight" keyName="K" icon={<ChevronRight size={15} aria-hidden="true" />} after />
        </nav>
        <div className="flex flex-wrap items-center gap-2">
          {exportMenu}
          <Button variant="secondary" size="sm" onClick={onRefresh} loading={refreshing}><RefreshCw size={15} aria-hidden="true" />Refresh</Button>
          <Button variant="secondary" size="sm" onClick={onOpenFolder}><FolderOpen size={15} aria-hidden="true" />Open flights folder</Button>
        </div>
      </div>
      {notices}
      <div className="flex flex-wrap items-end justify-between gap-x-8 gap-y-3">
        <div className="min-w-0 flex-1">
          <p className="eyebrow">Debrief · <span className="normal-case tracking-normal">{flight.id}</span></p>
          <h1 className="mt-1 flex flex-wrap items-baseline gap-x-3 font-display text-2xl font-semibold tracking-tight">
            <span className={flight.aircraft ? "" : "text-fg-3"}>{aircraftName(flight.aircraft)}</span>
            <span className="inline-flex items-center gap-2 text-xl text-fg-2">
              <span className={flight.departure ? "" : "text-fg-3"}>{flight.departure ?? "----"}</span>
              <ArrowRight size={18} className="text-fg-3" aria-label="to" />
              <span className={flight.arrival ? "" : "text-fg-3"}>{flight.arrival ?? "----"}</span>
            </span>
          </h1>
          {flight.aircraft?.icao ? <p className="mt-0.5 text-sm text-fg-2">{flight.aircraft.title}</p> : null}
          <InlineEdit className="-ml-2 mt-1 max-w-[42rem]" label="Flight title" placeholder="Add a title" value={meta.title ?? ""} onSave={(title) => onMeta({ title: title.trim() || undefined })} />
        </div>
        <Score flight={flight} />
      </div>
      <CapBanner flight={flight} />
      {flight.assessment_detail.landings?.length ? null : <AssessmentBreakdown detail={flight.assessment_detail} points={flight.assessment.points} />}
      <dl className="flex flex-wrap gap-x-8 gap-y-2 border-y border-line-subtle py-2.5">
        {facts.map(([label, value]) => (
          <div key={label} className="flex items-baseline gap-2">
            <dt className="eyebrow">{label}</dt>
            <dd className="readout text-sm">{value}</dd>
          </div>
        ))}
      </dl>
      <Logbook meta={meta} onMeta={onMeta} />
    </header>
  );
}

/** A step to the flight before or after, named in its tooltip with its key; disabled at either end of the list. */
function Step({ flight, label, keyName, icon, after = false }: { flight?: FlightSummary; label: string; keyName: string; icon: ReactNode; after?: boolean }) {
  const button = (
    <Button variant="ghost" size="sm" disabled={!flight} onClick={() => flight && openFlight(flight.id)} aria-keyshortcuts={keyName.toLowerCase()}>
      {after ? null : icon}{label.split(" ")[0]}{after ? icon : null}
    </Button>
  );
  if (!flight) return button;
  const where = [flight.departure, flight.arrival].some(Boolean) ? ` · ${flight.departure ?? "----"} → ${flight.arrival ?? "----"}` : "";
  return (
    <Tooltip content={<span className="text-xs">{label}: {aircraftName(flight.aircraft)}{where} · {flightDate(flight.started_utc)} <kbd className="readout ml-1 rounded-sm border border-current/30 px-1">{keyName}</kbd></span>}>
      {button}
    </Tooltip>
  );
}

/** The pilot's own notes and tags on the flight: what turns a recording into a logbook entry. */
function Logbook({ meta, onMeta }: { meta: FlightMeta; onMeta: Props["onMeta"] }) {
  const notify = useNotify();
  return (
    <section aria-label="Your notes" className="grid gap-x-8 gap-y-2 min-[960px]:grid-cols-[minmax(0,3fr)_minmax(16rem,2fr)]">
      <div className="min-w-0">
        <p className="eyebrow">Notes</p>
        <InlineEdit className="-ml-2" variant="body" multiline label="Notes" placeholder="Weather, what you practised, what to try next time" value={meta.notes ?? ""} onSave={(notes) => onMeta({ notes: notes.trim() || undefined })} />
      </div>
      <div className="min-w-0 [&_label]:eyebrow [&_label]:mb-1">
        <TagInput label="Tags" placeholder="Add a tag, then Enter" value={meta.tags} onValueChange={(tags) => { onMeta({ tags }).catch((error: unknown) => notify({ title: "Could not save the tags", body: String(error), tone: "error" })); }} />
      </div>
    </section>
  );
}

/** The flight's score out of 100 with its grade and data confidence; the points method's grade without a scored landing. */
function Score({ flight }: { flight: Debrief["flight"] }) {
  const a = flight.assessment_detail, score = flight.assessment.score;
  if (score === undefined) {
    return (
      <div className="flex flex-col items-start gap-1 sm:items-end">
        <div className="flex items-center gap-3"><GradeBadge assessment={flight.assessment} size="md" /><EventCounts counts={flight.event_counts} /></div>
        <p className="text-xs text-fg-3">{assessmentText(flight.assessment)} · <Term id="points">how points work</Term></p>
      </div>
    );
  }
  const graded = a.graded_by ?? 0, many = (a.landings?.length ?? 0) > 1;
  return (
    <div className="flex flex-col items-start gap-1 sm:items-end">
      <div className="flex items-center gap-3">
        <p className="flex items-baseline gap-1" aria-label={`Score ${number(score, 1)} of 100`}>
          <span className="readout text-4xl font-semibold leading-none tracking-tight">{number(score, 1)}</span>
          <span className="readout text-sm text-fg-3">/100</span>
        </p>
        <div className="flex flex-col items-start gap-1">
          <GradeTag grade={flight.assessment.grade} size="md" />
          {flight.assessment.confidence === "partial" ? (
            <Tooltip content={<span className="block max-w-[320px] text-xs">{a.partial_note}</span>}>
              <span tabIndex={0}><Badge size="sm" tone="warning" animated={false}>Partial data</Badge></span>
            </Tooltip>
          ) : <Badge size="sm" tone="neutral" animated={false}>Full data</Badge>}
        </div>
        <EventCounts counts={flight.event_counts} />
      </div>
      <p className="text-xs text-fg-3">{many ? `Graded on landing ${graded + 1}, the last full stop` : "Landing score"} · {flight.assessment.points} <Term id="points">event point{flight.assessment.points === 1 ? "" : "s"}</Term></p>
    </div>
  );
}

/** The caps that hold the flight's score down, the lowest first, each with what caused it, as the debrief words them. */
function CapBanner({ flight }: { flight: Debrief["flight"] }) {
  const caps = [...(flight.assessment_detail.caps ?? [])].sort((a, b) => a.score - b.score);
  if (!caps.length) return null;
  const worst = caps[0]!;
  const color = worst.score < 60 ? "var(--danger)" : "var(--warning)";
  return (
    <div role="note" className="flex gap-3 rounded-[var(--radius-panel)] border px-4 py-2.5" style={{ borderColor: `color-mix(in oklab, ${color} 40%, var(--border))`, background: `color-mix(in oklab, ${color} 8%, var(--surface))` }}>
      <ShieldAlert size={16} className="mt-0.5 shrink-0" style={{ color }} aria-hidden="true" />
      <ul className="flex min-w-0 flex-col gap-1 text-sm">
        {caps.map((c, i) => (
          <li key={i}>
            <span className="font-medium"><Term id="cap">{c.title}</Term></span>
            <span className="block text-xs text-fg-2">{c.landing !== undefined && (flight.assessment_detail.landings?.length ?? 0) > 1 ? `Landing ${c.landing + 1}: ` : ""}{c.reason}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
