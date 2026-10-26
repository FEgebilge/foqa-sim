import { useCallback, useRef, type MouseEvent, type ReactNode } from "react";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { clock, LEVEL_LABEL, phaseName } from "../../lib/format";
import { tickStep, useElementWidth, useHover, type TimeBus, type TimeRange } from "../../lib/time";
import type { FlightEvent, PhaseSpan, Touchdown } from "../../types";

const CODES: Record<string, string> = { Standing: "STD", Taxi: "TXI", Takeoff: "TOF", InitialClimb: "ICL", EnRoute: "ENR", Approach: "APR", Landing: "LDG", GoAround: "GA" };

interface Props {
  phases: PhaseSpan[];
  events: FlightEvent[];
  touchdowns: Touchdown[];
  range: TimeRange;
  focus: number | null;
  view: TimeRange | null;
  selected: number | null;
  bus: TimeBus;
  onFocus: (t: number) => void;
  onZoom: (range: TimeRange) => void;
  onSelectEvent: (index: number) => void;
  /** Span and playback controls, in the header row. */
  controls?: ReactNode;
}

/**
 * The flight at a glance: contiguous CICTT phases on one bar, events above it, touchdowns below.
 * Click the bar to put the cursor at a time; click a phase name to zoom the charts to it.
 */
export function PhaseTimeline({ phases, events, touchdowns, range, focus, view, selected, bus, onFocus, onZoom, onSelectEvent, controls }: Props) {
  const [barRef, width] = useElementWidth<HTMLDivElement>();
  const hoverRef = useRef<HTMLDivElement>(null);
  const [t0, t1] = range;
  const span = Math.max(1, t1 - t0);
  const pct = (t: number) => `${((Math.min(t1, Math.max(t0, t)) - t0) / span) * 100}%`;
  const share = (seconds: number) => `${(seconds / span) * 100}%`;
  const px = (seconds: number) => (seconds / span) * width;

  useHover(bus, useCallback((t: number | null) => {
    const node = hoverRef.current;
    if (!node) return;
    node.style.display = t === null ? "none" : "block";
    if (t !== null) node.style.left = `${((t - t0) / span) * 100}%`;
  }, [t0, span]));

  const timeAt = (event: MouseEvent<HTMLElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    return t0 + ((event.clientX - box.left) / box.width) * span;
  };

  const step = tickStep(span, Math.max(2, Math.floor(width / 90)));
  const ticks: number[] = [];
  for (let t = Math.ceil(t0 / step) * step; t <= t1; t += step) ticks.push(t);
  const selectedEvent = selected === null ? null : events[selected];

  return (
    <section className="panel px-4 pt-3 pb-2" aria-labelledby="timeline-title">
      <div className="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
        <h2 id="timeline-title" className="eyebrow">Phases and events</h2>
        <p className="text-xs text-fg-3">Click the bar to place the cursor, a phase name to zoom to it.</p>
        {controls}
      </div>

      {/* Event lane */}
      <div className="relative mt-2 h-5" aria-label="Events on the timeline">
        {events.map((event, index) => {
          const isSelected = index === selected;
          return (
            <Tooltip key={`${event.rule}-${index}`} content={`${event.title} · ${LEVEL_LABEL[event.level]} · ${clock(event.start_s)}`}>
              <button
                type="button"
                aria-label={`${event.title}, ${LEVEL_LABEL[event.level]}, at ${clock(event.start_s)}`}
                aria-pressed={isSelected}
                onClick={() => onSelectEvent(index)}
                className="absolute bottom-0 -translate-x-1/2 cursor-pointer"
                style={{ left: pct(event.start_s), zIndex: isSelected ? 2 : 1 }}
              >
                <svg width={isSelected ? 14 : 11} height={isSelected ? 14 : 11} viewBox="0 0 10 10" aria-hidden="true">
                  <path d="M5 10 L0 1 L10 1 Z" fill={`var(--level-${event.level})`} stroke={isSelected ? "var(--foreground)" : "var(--surface)"} strokeWidth={1} />
                </svg>
              </button>
            </Tooltip>
          );
        })}
      </div>

      {/* Phase bar */}
      <div
        ref={barRef}
        className="relative h-11 cursor-crosshair overflow-hidden rounded-[var(--radius-control)] border border-line"
        onClick={(event) => onFocus(timeAt(event))}
        onMouseMove={(event) => bus.emit(timeAt(event))}
        onMouseLeave={() => bus.emit(null)}
        role="img"
        aria-label={`Phases: ${phases.map((p) => `${phaseName(p.phase)} ${clock(p.start_s)} to ${clock(p.end_s)}`).join("; ")}`}
      >
        {phases.map((phase, index) => {
          const w = px(phase.end_s - phase.start_s);
          return (
            <div key={index} className="absolute inset-y-0 flex items-center overflow-hidden border-r border-[var(--surface)] px-1.5"
              style={{ left: pct(phase.start_s), width: share(phase.end_s - phase.start_s), background: `color-mix(in oklab, var(--phase-${phase.phase}, var(--phase-unknown)) 78%, var(--surface))` }}>
              <span className="readout truncate text-[11px] font-semibold text-[oklch(18%_.03_258)]">{w > 90 ? phaseName(phase.phase) : w > 30 ? CODES[phase.phase] ?? "" : ""}</span>
            </div>
          );
        })}
        {view ? <div className="pointer-events-none absolute inset-y-0 border-x-2 border-[var(--foreground)] bg-[color-mix(in_oklab,var(--foreground)_10%,transparent)]" style={{ left: pct(view[0]), width: share(view[1] - view[0]) }} /> : null}
        {selectedEvent ? (
          <div className="pointer-events-none absolute inset-y-0 min-w-[3px]" style={{ left: pct(selectedEvent.start_s), width: share(selectedEvent.end_s - selectedEvent.start_s), background: `var(--level-${selectedEvent.level})`, opacity: 0.85 }} />
        ) : null}
        {focus !== null ? <div className="pointer-events-none absolute inset-y-0 w-0.5 -translate-x-1/2 bg-[var(--foreground)]" style={{ left: pct(focus) }} /> : null}
        <div ref={hoverRef} className="pointer-events-none absolute inset-y-0 hidden w-px bg-[var(--foreground)] opacity-60" />
      </div>

      {/* Touchdowns and time axis */}
      <div className="relative h-9">
        {ticks.map((t) => (
          <span key={t} className="readout absolute top-4 -translate-x-1/2 text-[10px] text-fg-3" style={{ left: pct(t) }}>{clock(t)}</span>
        ))}
        {touchdowns.map((td, index) => (
          <button key={td.t_s} type="button" onClick={() => onFocus(td.t_s)} className="absolute top-0 -translate-x-1/2 cursor-pointer" style={{ left: pct(td.t_s) }} aria-label={`Touchdown ${index + 1} at ${clock(td.t_s)}`}>
            <span className="readout flex items-center gap-0.5 rounded-sm bg-fg px-1 text-[10px] leading-4 font-semibold text-bg">TD{index + 1}</span>
          </button>
        ))}
      </div>

      {/* Phase legend doubles as zoom buttons */}
      <div className="flex flex-wrap gap-1.5 border-t border-line-subtle pt-2">
        {phases.map((phase, index) => (
          <button key={index} type="button" onClick={() => onZoom([phase.start_s, phase.end_s])}
            className="inline-flex items-center gap-1.5 rounded-full border border-line px-2 py-0.5 text-xs text-fg-2 hover:border-line-strong hover:text-fg"
            title={`Zoom the charts to ${phaseName(phase.phase)} (${clock(phase.start_s)}–${clock(phase.end_s)})`}>
            <span className="size-2 rounded-full" style={{ background: `var(--phase-${phase.phase}, var(--phase-unknown))` }} aria-hidden="true" />
            {phaseName(phase.phase)}
            <span className="readout text-fg-3">{clock(phase.end_s - phase.start_s)}</span>
          </button>
        ))}
      </div>
    </section>
  );
}
