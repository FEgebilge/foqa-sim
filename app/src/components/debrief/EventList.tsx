import { useEffect, useRef } from "react";
import { Info, ShieldCheck } from "lucide-react";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import { reading, LEVEL_MEANING } from "../../lib/explain";
import { clock, duration, LEVEL_LABEL, LEVELS, phaseName, why } from "../../lib/format";
import type { FlightEvent, PhaseSpan, Series } from "../../types";

interface Props {
  events: FlightEvent[];
  phases: PhaseSpan[];
  series: Series;
  selected: number | null;
  onSelect: (index: number) => void;
}

/** Phase an event starts in; a start on a boundary belongs to the phase that begins there. */
const phaseAt = (phases: PhaseSpan[], t: number) => (phases.find((p) => t >= p.start_s && t < p.end_s) ?? phases.at(-1))?.phase;

/** What the four levels mean, behind a small disclosure. */
export function LevelLegend() {
  return (
    <details className="text-xs text-fg-2">
      <summary className="inline-flex cursor-pointer list-none items-center gap-1 text-fg-3 hover:text-fg"><Info size={13} aria-hidden="true" />What the levels mean</summary>
      <dl className="mt-2 flex flex-col gap-1.5 rounded-[var(--radius-control)] border border-line-subtle bg-bg p-2.5">
        {LEVELS.map((level) => (
          <div key={level} className="flex gap-2">
            <dt className="flex w-[4.5rem] shrink-0 items-center gap-1.5 font-medium text-fg"><span className="size-2 rounded-[2px]" style={{ background: `var(--level-${level})` }} aria-hidden="true" />{LEVEL_LABEL[level]}</dt>
            <dd>{LEVEL_MEANING[level]}</dd>
          </div>
        ))}
      </dl>
    </details>
  );
}

/** Events grouped by level, most severe first: the value against its limit, why it matters, and what to do about it. */
export function EventList({ events, phases, series, selected, onSelect }: Props) {
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (selected !== null) listRef.current?.querySelector(`[data-index="${selected}"]`)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [selected]);

  if (!events.length) {
    return <div className="panel"><EmptyState size="sm" icon={<ShieldCheck size={18} />} title="No events" description="No rule of the set was exceeded on this flight." /></div>;
  }

  return (
    <div ref={listRef} className="flex flex-col gap-4">
      {LEVELS.map((level) => {
        const group = events.map((event, index) => ({ event, index })).filter(({ event }) => event.level === level);
        if (!group.length) return null;
        return (
          <section key={level} aria-labelledby={`events-${level}`}>
            <h3 id={`events-${level}`} className="eyebrow mb-1.5 flex items-center gap-2" title={LEVEL_MEANING[level]}>
              <span className="size-2.5 rounded-[2px]" style={{ background: `var(--level-${level})` }} aria-hidden="true" />
              {LEVEL_LABEL[level]} <span className="readout">{group.length}</span>
            </h3>
            <ul className="flex flex-col gap-1.5">
              {group.map(({ event, index }) => {
                const active = index === selected;
                const read = reading(event, series);
                const phase = phaseAt(phases, event.start_s);
                const span = event.end_s - event.start_s;
                return (
                  <li key={index} data-index={index} className="rounded-[var(--radius-control)] border bg-surface transition-colors hover:border-line-strong"
                    style={{ borderColor: active ? `var(--level-${level})` : "var(--border)", boxShadow: `inset 3px 0 0 var(--level-${level})${active ? `, 0 0 0 1px var(--level-${level})` : ""}` }}>
                    <button type="button" aria-pressed={active} onClick={() => onSelect(index)} className="block w-full cursor-pointer p-3 pb-2 text-left">
                      <span className="flex items-baseline justify-between gap-2">
                        <span className="font-medium">{event.title}</span>
                        <span className="readout shrink-0 text-xs text-fg-2">T+{clock(event.start_s)}</span>
                      </span>
                      {read ? (
                        <span className="mt-1 flex flex-wrap items-baseline gap-x-2 text-sm">
                          <span className="readout font-semibold" style={{ color: `var(--level-${level})` }}>{read.value}</span>
                          {read.limit ? <span className="text-xs text-fg-3">{read.limit}</span> : null}
                        </span>
                      ) : null}
                      <span className="mt-1 flex flex-wrap gap-x-3 text-[11px] text-fg-3">
                        {phase ? <span>{phaseName(phase)}</span> : null}
                        {span > 0 ? <span className="readout">lasted {duration(span)}</span> : null}
                      </span>
                      {event.why_it_matters ? <span className={`mt-1.5 block text-xs leading-relaxed text-fg-2 ${active ? "" : "line-clamp-2"}`}>{event.why_it_matters}</span> : null}
                      {active && event.advice ? (
                        <span className="mt-2 block rounded-sm bg-bg px-2 py-1.5 text-xs leading-relaxed">
                          <span className="eyebrow mr-1.5">What to do</span>{event.advice}
                        </span>
                      ) : null}
                    </button>
                    <details className="border-t border-line-subtle px-3 py-1.5 text-[11px] text-fg-3">
                      <summary className="cursor-pointer hover:text-fg">Details</summary>
                      <p className="mt-1 flex flex-wrap gap-x-1.5"><span>Measure</span><code className="readout text-fg-2">{event.measure}</code><span className="readout text-fg-2">{why(event).comparison}</span></p>
                      <p className="mt-1 leading-relaxed text-fg-2">{event.description}</p>
                      <p className="readout mt-1">{event.rule}@{event.version}</p>
                    </details>
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
