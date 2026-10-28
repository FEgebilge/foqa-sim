import { useState } from "react";
import { ChevronRight, Crosshair, Gauge, Info, Scale } from "lucide-react";
import SegmentedControl from "@ui/components/segmented-control/segmented-control";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { againstText, anchorTicks, domainBar, ellColor, ellWords, LEVEL_WORD, levelColor, stripRange, topDomain, ZONE_COLOR, ZONES, zoneOf, zones } from "../../lib/assessment";
import { clock, GRADE_LABEL, number } from "../../lib/format";
import { levelVar } from "../../lib/palette";
import type { CriterionAssessment, Debrief, DomainAssessment, FindingAssessment, LandingAssessment } from "../../types";
import { LevelBadge } from "../level";
import { AssessmentBreakdown } from "./AssessmentBreakdown";

interface Props { debrief: Debrief; onSelectEvent: (index: number) => void }

/**
 * The landing-3 assessment of each landing (docs/scoring.md): six area bars, each track as long as its weight and shaded
 * by the points it lost in its level's colour; the selected domain opens beside them (below when narrow) to its criteria on
 * their band strips and its event cards, the one that set the deduction marked. Findings outside every landing follow.
 */
export function AssessmentPanel({ debrief, onSelectEvent }: Props) {
  const detail = debrief.flight.assessment_detail;
  const landings = detail.landings ?? [];
  const [pick, setPick] = useState(detail.graded_by ?? 0);
  const landing = landings[pick];
  if (!landing) return null;
  const options = landings.map((l, i) => ({ value: String(i), label: `Landing ${i + 1}${l.score !== null ? ` · ${number(l.score, 1)}` : ""}${l.full_stop ? "" : " (touch-and-go)"}` }));

  return (
    <section className="panel flex flex-col" aria-labelledby="assessment-title">
      <header className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-line-subtle px-4 py-2.5">
        <h2 id="assessment-title" className="eyebrow flex items-center gap-1.5"><Scale size={13} aria-hidden="true" />Landing assessment</h2>
        {landings.length > 1 ? <SegmentedControl label="Landing" options={options} value={String(pick)} onValueChange={(v) => setPick(Number(v))} /> : null}
        <LandingFacts landing={landing} />
      </header>
      {landing.status === "scored"
        ? <Scored key={pick} landing={landing} debrief={debrief} onSelectEvent={onSelectEvent} />
        : (
          <div className="flex flex-col gap-3 p-4 text-sm text-fg-2">
            <p>{landing.note}</p>
            <AssessmentBreakdown detail={detail} points={detail.legacy_points ?? 0} />
          </div>
        )}
      <OtherFindings debrief={debrief} onSelectEvent={onSelectEvent} />
    </section>
  );
}

/** The grading limits, reference speeds and stable height, in one line as the debrief words them. */
function LandingFacts({ landing }: { landing: LandingAssessment }) {
  const s = landing.speeds, facts = landing.facts;
  return (
    <p className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-fg-2 sm:ml-auto">
      {facts.map((f) => <span key={f} className="readout">{f}</span>)}
      {s ? <Tooltip content={<span className="block max-w-[340px] text-xs">{s.basis}</span>}><button type="button" className="text-fg-3 hover:text-fg" aria-label="Where the reference speeds come from"><Info size={13} aria-hidden="true" /></button></Tooltip> : null}
    </p>
  );
}

function Scored({ landing, debrief, onSelectEvent }: { landing: LandingAssessment; debrief: Debrief; onSelectEvent: (index: number) => void }) {
  const [open, setOpen] = useState(topDomain(landing)?.id);
  const domain = landing.domains.find((d) => d.id === open) ?? landing.domains[0]!;
  const maxWeight = Math.max(...landing.domains.map((d) => d.weight));
  return (
    <div className="grid gap-x-6 gap-y-4 p-4 lg:grid-cols-[minmax(300px,5fr)_minmax(0,8fr)]">
      <div className="flex flex-col gap-1">
        <ul className="flex flex-col gap-1" aria-label="Assessment areas">
          {landing.domains.map((d) => <DomainBar key={d.id} domain={d} maxWeight={maxWeight} active={d.id === domain.id} onSelect={() => setOpen(d.id)} />)}
        </ul>
        <div className="mt-2 flex items-baseline justify-between border-t border-line-subtle pt-2 text-xs">
          <span className="text-fg-2">Before safety caps {number(landing.raw ?? 0, 1)}{landing.caps.length ? ((landing.score ?? 0) < (landing.raw ?? 0) ? `, held to ${number(landing.score ?? 0, 1)} by a cap` : `; a cap at ${number(Math.min(...landing.caps.map((c) => c.score)), 1)} did not apply`) : ""}</span>
          <span className="readout text-base font-semibold">{number(landing.score ?? 0, 1)} <span className="text-xs font-normal text-fg-3">{landing.grade ? GRADE_LABEL[landing.grade] : ""}</span></span>
        </div>
        <p className="text-[11px] leading-relaxed text-fg-3">Each area loses points for its worst item only, so one mistake is never counted twice. An area with nothing measured is not graded and its weight is shared among the others.</p>
      </div>
      <DomainDetail domain={domain} landing={landing} debrief={debrief} onSelectEvent={onSelectEvent} />
    </div>
  );
}

function DomainBar({ domain, maxWeight, active, onSelect }: { domain: DomainAssessment; maxWeight: number; active: boolean; onSelect: () => void }) {
  const bar = domainBar(domain, maxWeight);
  return (
    <li>
      <button type="button" aria-pressed={active} onClick={onSelect}
        className={`grid w-full cursor-pointer grid-cols-[minmax(7.5rem,9rem)_minmax(0,1fr)_3.5rem_1rem] items-center gap-x-3 rounded-[var(--radius-control)] px-2 py-1.5 text-left transition-colors hover:bg-muted ${active ? "bg-muted" : ""}`}>
        <span className="min-w-0">
          <span className="block truncate text-sm">{domain.title}</span>
          <span className="readout block text-[10.5px] text-fg-3">{bar.na ? "not graded" : domain.points ? LEVEL_WORD[Math.min(3, domain.level)] : "full marks"} · worth {number(domain.weight)} pts</span>
        </span>
        <span className="relative h-3" aria-hidden="true">
          <span className={`absolute inset-y-0 left-0 overflow-hidden rounded-sm ${bar.na ? "border border-dashed border-line" : "bg-[color-mix(in_oklab,var(--foreground)_16%,transparent)]"}`} style={{ width: `${bar.track}%` }}>
            {bar.lost ? <span className="absolute inset-y-0 right-0" style={{ width: `${bar.lost}%`, background: `repeating-linear-gradient(135deg, ${bar.color} 0 3px, color-mix(in oklab, ${bar.color} 55%, transparent) 3px 6px)` }} /> : null}
          </span>
        </span>
        <span className="readout text-right text-sm" style={domain.points ? { color: bar.color } : undefined}>{bar.na ? "–" : domain.points ? `−${number(domain.points, 1)}` : "0"}</span>
        <ChevronRight size={14} className={`text-fg-3 transition-transform ${active ? "translate-x-0.5 text-fg" : ""}`} aria-hidden="true" />
      </button>
    </li>
  );
}

function DomainDetail({ domain, landing, debrief, onSelectEvent }: { domain: DomainAssessment; landing: LandingAssessment; debrief: Debrief; onSelectEvent: (index: number) => void }) {
  const set = domain.set_by_text;
  return (
    <div className="flex min-w-0 flex-col gap-3" aria-live="polite">
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <h3 className="font-display text-base font-semibold">{domain.title}</h3>
        <span className="readout text-sm text-fg-2">{domain.loss === null ? "Not graded: nothing measured" : `−${number(domain.points, 1)} of ${number(domain.weight)}`}</span>
        {set && domain.points ? <span className="text-xs text-fg-2">Biggest deduction: <span className="text-fg">{set}</span></span> : null}
      </div>
      <p className="-mt-2 text-xs text-fg-3">{domain.summary}</p>
      {domain.criteria.length ? (
        <ul className="flex flex-col divide-y divide-line-subtle rounded-[var(--radius-control)] border border-line-subtle">
          {[...domain.criteria].sort((a, b) => a.id.localeCompare(b.id)).map((c) => <CriterionRow
 key={`${c.id}-${c.height}`} c={c} landing={landing} domain={domain} />)}
        </ul>
      ) : <p className="text-xs text-fg-3">No measured value in this area.</p>}
      {domain.findings.length ? (
        <div className="flex flex-col gap-1.5">
          <h4 className="eyebrow">Flagged events</h4>
          <ul className="grid gap-1.5 [grid-template-columns:repeat(auto-fill,minmax(min(100%,280px),1fr))]">
            {domain.findings.map((f, i) => <FindingCard key={`${f.rule}-${i}`} f={f} domain={domain} debrief={debrief} onSelectEvent={onSelectEvent} />)}
          </ul>
        </div>
      ) : null}
      <ZoneLegend />
    </div>
  );
}

function CriterionRow({ c, landing, domain }: { c: CriterionAssessment; landing: LandingAssessment; domain: DomainAssessment }) {
  const where = [c.height_text, againstText(c, landing)].filter(Boolean).join(" · ");
  const cost = (c.loss * domain.weight * domain.points) / Math.max(1e-9, (domain.loss ?? 0) * domain.weight);
  return (
    <li className={`grid gap-x-4 gap-y-1.5 px-3 py-2 md:grid-cols-[minmax(11rem,15rem)_minmax(0,1fr)] ${c.sets ? "bg-[color-mix(in_oklab,var(--accent)_7%,transparent)]" : ""}`}>
      <div className="min-w-0">
        <p className="flex items-center gap-1.5 text-sm">
          <span className="readout text-[10.5px] text-fg-3">{c.id}</span>
          <span className="truncate" title={c.title}>{c.title}</span>
        </p>
        {where ? <p className="text-[11px] text-fg-3">{where}</p> : null}
        <p className="mt-0.5 flex flex-wrap items-baseline gap-x-2">
          <span className="readout text-base leading-tight" style={c.ell > 0.5 ? { color: ellColor(c.ell) } : undefined}>{c.value_text}</span>
          <span className="text-[11px] text-fg-2">{ellWords(c.ell)} <span className="readout text-fg-3" title="Severity from 0 (ideal) to 3 (worst)">severity {number(c.ell, 1)} of 3</span></span>
        </p>
        {c.sets ? <SetsMark points={Number.isFinite(cost) ? cost : domain.points} /> : null}
      </div>
      <div className="min-w-0 self-center">
        {c.unit === null ? <LevelSteps c={c} /> : <BandStrip c={c} />}
        <p className="mt-0.5 line-clamp-1 text-[10.5px] text-fg-3" title={c.standard}>{c.standard}</p>
      </div>
    </li>
  );
}

const SetsMark = ({ points }: { points: number }) => (
  <span className="mt-1 inline-flex items-center gap-1 rounded-sm bg-accent-subtle px-1.5 py-px text-[10.5px] font-medium text-accent-strong">
    <Gauge size={11} aria-hidden="true" />Biggest deduction here · −{number(points, 1)}
  </span>
);

const pct = (lo: number, hi: number, x: number) => Math.min(100, Math.max(0, ((x - lo) / (hi - lo)) * 100));

/** The value on its criterion's zones: ideal in green, tolerance lighter, then the FDM levels in their colours. */
function BandStrip({ c }: { c: CriterionAssessment }) {
  const [lo, hi] = stripRange(c);
  const list = zones(c.bands);
  // Anchor labels, each at least 9% from the last one kept.
  const ticks = anchorTicks(c)
    .reduce<{ at: number; text: string }[]>((kept, t) => (kept.length && pct(lo, hi, t.at) - pct(lo, hi, kept.at(-1)!.at) < 9 ? kept : [...kept, t]), []);

  const zone = zoneOf(list, c.value);
  const label = `${c.value_text}: ${ZONES[zone]}. Anchors ${ticks.map((t) => t.text).join(", ")}`;
  return (
    <div role="img" aria-label={label}>
      <div className="relative mt-1.5 h-2.5 overflow-hidden rounded-sm bg-muted">
        {list.map((z) => {
          const from = pct(lo, hi, z.from), to = pct(lo, hi, z.to);
          return to > from ? <span key={`${z.from}-${z.zone}`} className="absolute inset-y-0" title={ZONES[z.zone]} style={{ left: `${from}%`, width: `${to - from}%`, background: ZONE_COLOR[z.zone] }} /> : null;
        })}
      </div>
      <div className="relative -mt-3.5 h-4" aria-hidden="true">
        <span className="absolute top-0 h-4 w-[3px] -translate-x-1/2 rounded-full ring-2 ring-surface" style={{ left: `${pct(lo, hi, c.value)}%`, background: c.ell > 0.5 ? ellColor(c.ell) : "var(--foreground)" }} />
      </div>
      <div className="readout relative h-3 text-[9.5px] text-fg-3" aria-hidden="true">
        {ticks.map((t) => <span key={t.at} className="absolute -translate-x-1/2 whitespace-nowrap" style={{ left: `${pct(lo, hi, t.at)}%` }}>{t.text}</span>)}
      </div>
    </div>
  );
}

/** A criterion graded in whole levels (configuration, bounce): four steps, the reached one lit. */
function LevelSteps({ c }: { c: CriterionAssessment }) {
  return (
    <div className="flex gap-1 pt-1" role="img" aria-label={`Level ${c.level}`}>
      {[0, 1, 2, 3].map((l) => (
        <span key={l} className="readout flex-1 rounded-sm py-0.5 text-center text-[10px]"
          style={{ background: l === c.level ? `color-mix(in oklab, ${levelColor(l)} 55%, transparent)` : "var(--surface-muted)", color: l === c.level ? "var(--foreground)" : "var(--text-muted)" }}>
          {l ? `L${l}` : "None"}
        </span>
      ))}
    </div>
  );
}

function FindingCard({ f, domain, debrief, onSelectEvent }: { f: FindingAssessment; domain: DomainAssessment; debrief: Debrief; onSelectEvent: (index: number) => void }) {
  const event = f.event !== null ? debrief.events[f.event] : undefined;
  return (
    <li className="flex flex-col gap-1 rounded-[var(--radius-control)] border bg-surface p-2.5"
      style={{ borderColor: f.sets ? "var(--accent)" : "var(--border)", boxShadow: `inset 3px 0 0 ${levelVar(f.level)}` }}>
      <span className="flex items-start justify-between gap-2">
        <span className="text-sm font-medium">{event?.title ?? f.rule}</span>
        <LevelBadge level={f.level} />
      </span>
      {event?.detail ? <span className="readout text-xs" style={{ color: levelVar(f.level) }}>{event.detail}</span> : null}
      <span className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-fg-3">
        <span>{f.level === "advisory" ? "Advisory: no deduction" : `severity ${number(f.ell, 1)} of 3 · costs ${number(f.loss * 100)}% of the area`}</span>
        {event ? <button type="button" className="inline-flex items-center gap-1 hover:text-fg" onClick={() => onSelectEvent(f.event!)}><Crosshair size={12} aria-hidden="true" />T+{clock(event.start_s)}</button> : null}
      </span>
      {f.sets ? <SetsMark points={domain.points} /> : null}
    </li>
  );
}

function ZoneLegend() {
  return (
    <p className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[10.5px] text-fg-3">
      {ZONES.map((z, i) => <span key={z} className="inline-flex items-center gap-1"><span className="h-2 w-3 rounded-[2px]" style={{ background: ZONE_COLOR[i] }} aria-hidden="true" />{z}</span>)}
      <span>· ideal and tolerance follow the checkride standard (ACS or airline SOP); levels 1–3 follow flight-data-monitoring limits</span>
    </p>
  );
}

/** Findings outside every landing: they never change a landing's score, and can only lower the flight's grade. */
function OtherFindings({ debrief, onSelectEvent }: Props) {
  const detail = debrief.flight.assessment_detail, other = detail.other_findings ?? [];
  if (!other.length) return null;
  const scored = detail.landings?.some((l) => l.status === "scored");
  return (
    <div className="flex flex-col gap-1.5 border-t border-line-subtle px-4 py-2.5">
      <p className="text-xs text-fg-2">{scored
        ? <><span className="eyebrow mr-2">Other phases</span>Outside every scored landing: no effect on a landing's score; a high one holds the flight at Fair, two at Poor.</>
        : <><span className="eyebrow mr-2">Findings</span>No landing was scored, so these grade the flight by the points method.</>}</p>
      <ul className="flex flex-wrap gap-1.5">
        {other.map((i) => {
          const e = debrief.events[i];
          return e ? (
            <li key={i}>
              <button type="button" onClick={() => onSelectEvent(i)} className="inline-flex items-center gap-1.5 rounded-[var(--radius-control)] border border-line px-2 py-0.5 text-xs hover:border-line-strong">
                <span className="size-2 rounded-[2px]" style={{ background: levelVar(e.level) }} aria-hidden="true" />{e.title}<span className="readout text-fg-3">T+{clock(e.start_s)}</span>
              </button>
            </li>
          ) : null;
        })}
      </ul>
    </div>
  );
}
