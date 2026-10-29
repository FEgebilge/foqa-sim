import { useEffect, useRef, useState } from "react";
import { Flag, Info, ShieldCheck } from "lucide-react";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import SegmentedControl from "@ui/components/segmented-control/segmented-control";
import { clock, duration, LEVEL_LABEL, LEVEL_MEANING, LEVELS, phaseName, why } from "../../lib/format";
import { levelVar } from "../../lib/palette";
import { eventMode, type FlyingMode } from "../../lib/series";
import type { FlightEvent, PhaseSpan, Series } from "../../types";
import { Term } from "../Term";

/** The rule of a moment the pilot marked from the in-sim panel; its note is the description. */
export const PILOT_REPORT = "pilot-report";

interface Props {
  events: FlightEvent[];
  phases: PhaseSpan[];
  series: Series;
  selected: number | null;
  onSelect: (index: number) => void;
}

/** Phase an event starts in; a start on a boundary belongs to the phase that begins there. */
const phaseAt = (phases: PhaseSpan[], t: number) => (phases.find((p) => t >= p.start_s && t < p.end_s) ?? phases.at(-1))?.phase;

type ModeFilter = "all" | FlyingMode;
const MODE_TAG: Record<FlyingMode, string> = { ap: "AP", manual: "Hand-flown" };
const MODE_EMPTY: Record<FlyingMode, string> = { ap: "No events on the autopilot.", manual: "No events while hand-flown." };

/** What the four levels mean, behind a small disclosure. */
export function LevelLegend() {
  return (
    <details className="text-xs text-fg-2">
      <summary className="inline-flex cursor-pointer list-none items-center gap-1 text-fg-3 hover:text-fg"><Info size={13} aria-hidden="true" />What the levels mean</summary>
      <dl className="mt-2 flex flex-col gap-1.5 rounded-[var(--radius-control)] border border-line-subtle bg-bg p-2.5">
        {LEVELS.map((level) => (
          <div key={level} className="flex gap-2">
            <dt className="flex w-[4.5rem] shrink-0 items-center gap-1.5 font-medium text-fg"><span className="size-2 rounded-[2px]" style={{ background: levelVar(level) }} aria-hidden="true" />{LEVEL_LABEL[level]}</dt>
            <dd>{LEVEL_MEANING[level]}</dd>
          </div>
        ))}
      </dl>
    </details>
  );
}

/** Events grouped by level, most severe first: the value against its limit, why it matters, and what to do about it. */
export function EventList({ events, phases, series, selected, onSelect }: Props) {
  const [mode, setMode] = useState<ModeFilter>("all");
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (selected !== null) listRef.current?.querySelector(`[data-index="${selected}"]`)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [selected]);

  if (!events.length) {
    return <div className="panel"><EmptyState size="sm" icon={<ShieldCheck size={18} />} title="No events" description="No rule of the set was exceeded on this flight." /></div>;
  }

  // An event's index stays its place in the whole list, so the cursor and selection keep working while a filter hides some.
  const modes = events.map((event) => eventMode(series, event));
  const count = (m: FlyingMode) => modes.filter((x) => x === m).length;
  const filtering = mode !== "all" && modes.some(Boolean);
  const shown = events.map((event, index) => ({ event, index })).filter(({ index }) => !filtering || modes[index] === mode);

  return (
    <div ref={listRef} className="flex flex-col gap-4">
      {modes.some(Boolean) ? (
        <SegmentedControl label="Flying mode" value={mode} onValueChange={(v) => setMode(v as ModeFilter)}
          options={[{ value: "all", label: "All", accessory: <span className="readout">{events.length}</span> }, { value: "ap", label: "Autopilot", accessory: <span className="readout">{count("ap")}</span> }, { value: "manual", label: "Hand-flown", accessory: <span className="readout">{count("manual")}</span> }]} />
      ) : null}
      {filtering && !shown.length ? <div className="panel"><EmptyState size="sm" icon={<ShieldCheck size={18} />} title="No events" description={MODE_EMPTY[mode as FlyingMode]} /></div> : null}
      {LEVELS.map((level) => {
        const group = shown.filter(({ event }) => event.level === level);
        if (!group.length) return null;
        return (
          <section key={level} aria-labelledby={`events-${level}`}>
            <h3 id={`events-${level}`} className="eyebrow mb-1.5 flex items-center gap-2" title={LEVEL_MEANING[level]}>
              <span className="size-2.5 rounded-[2px]" style={{ background: levelVar(level) }} aria-hidden="true" />
              {LEVEL_LABEL[level]} <span className="readout">{group.length}</span>
            </h3>
            <ul className="flex flex-col gap-1.5">
              {group.map(({ event, index }) => {
                const active = index === selected;
                const phase = phaseAt(phases, event.start_s);
                const span = event.end_s - event.start_s;
                const mark = event.rule === PILOT_REPORT;
                const flying = modes[index];
                return (
                  // The title, time and value are the button (its accessible name); the rest of the card is text that a click also selects.
                  <li key={index} data-index={index} onClick={() => { if (!active) onSelect(index); }} className="cursor-pointer rounded-[var(--radius-control)] border bg-surface transition-colors hover:border-line-strong"
                    style={{ borderColor: active ? levelVar(level) : "var(--border)", boxShadow: `inset 3px 0 0 ${levelVar(level)}${active ? `, 0 0 0 1px ${levelVar(level)}` : ""}` }}>
                    <button type="button" aria-pressed={active} onClick={(e) => { e.stopPropagation(); onSelect(index); }} className="block w-full cursor-pointer rounded-[var(--radius-control)] px-3 pt-3 text-left">
                      <span className="flex items-baseline justify-between gap-2">
                        <span className="flex items-center gap-1.5 font-medium">{mark ? <><Flag size={13} className="text-accent" aria-hidden="true" />Your mark</> : event.title}</span>
                        <span className="readout shrink-0 text-xs text-fg-2">T+{clock(event.start_s)}</span>
                      </span>
                      {mark && event.description ? <span className="mt-1 block text-sm italic text-fg-2">“{event.description}”</span> : null}
                      {event.detail ? <span className="readout mt-1 block text-sm font-semibold" style={{ color: levelVar(level) }}>{event.detail}</span> : null}
                    </button>
                    <div className="px-3 pb-2">
                      <span className="mt-1 flex flex-wrap gap-x-3 text-[11px] text-fg-3">
                        {phase ? <span>{phaseName(phase)}</span> : null}
                        {flying ? <span className="font-medium text-fg-2">{MODE_TAG[flying]}</span> : null}
                        {span > 0 ? <span className="readout"><Term id="sustain">lasted</Term> {duration(span)}</span> : null}
                      </span>
                      {event.why_it_matters ? <span className={`mt-1.5 block text-xs leading-relaxed text-fg-2 ${active ? "" : "line-clamp-2"}`}>{event.why_it_matters}</span> : null}
                      {active && event.advice ? (
                        <span className="mt-2 block rounded-sm bg-bg px-2 py-1.5 text-xs leading-relaxed">
                          <span className="eyebrow mr-1.5">What to do</span>{event.advice}
                        </span>
                      ) : null}
                    </div>
                    {mark ? null : <details className="cursor-auto border-t border-line-subtle px-3 py-1.5 text-[11px] text-fg-3" onClick={(e) => e.stopPropagation()}>
                      <summary className="cursor-pointer hover:text-fg">Details</summary>
                      <p className="mt-1 flex flex-wrap gap-x-1.5"><span>Measure</span><code className="readout text-fg-2">{event.measure}</code><span className="readout text-fg-2">{why(event).comparison}</span></p>
                      <p className="mt-1 leading-relaxed text-fg-2">{event.description}</p>
                      <p className="readout mt-1">{event.rule}@{event.version}</p>
                    </details>}
                  </li>
                );
              })}
            </ul>
          </section>
        );
      })}
    </div>
  );
}
