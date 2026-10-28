import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, type RefObject } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { clock, number } from "../../lib/format";
import { APPROACH_GATES, hardLanding, SINK_LIMIT, type Mark } from "../../lib/references";
import { alpha, cssVar, levelColor, LINE, phaseColor, REF, type Theme } from "../../lib/palette";
import { indexAt } from "../../lib/series";
import { useHover, type TimeBus, type TimeRange } from "../../lib/time";
import type { Debrief, FlightEvent, Series } from "../../types";

type Key = Exclude<keyof Series, "t_s" | "lat" | "lon">;
interface Spec {
  id: string; title: string; unit: string; height: number; digits: number;
  lines: { key: Key; label: string; dash?: boolean }[];
  /** Keep 0 on the y axis. */
  zero?: boolean;
  /** Centre the y axis on this value (1 g), so loads above and below it read symmetrically. */
  centre?: number;
  step?: boolean;
  /** A reference series drawn as a step line in the reference colour, for limits that change along the flight. */
  limit?: { key: Key; label: string; source: string };
  /** Reference lines; the y axis stretches to show them unless it is centred. */
  refs?: (flight: Debrief["flight"]) => Mark[];
}

const ONE_G: Mark = { at: 1, label: "1 g: level flight" };
// Flaps sit under speed, so flap changes read against the speed they were made at.
const SPECS: Spec[] = [
  { id: "alt", title: "Height above airport", unit: "ft", height: 150, digits: 0, refs: () => APPROACH_GATES, lines: [{ key: "aal_ft", label: "Above airport" }, { key: "alt_ft", label: "Pressure altitude", dash: true }] },
  { id: "speed", title: "Speed", unit: "kt", height: 130, digits: 0, limit: { key: "vfe_kt", label: "Flap limit (VFE)", source: "aircraft profile" }, lines: [{ key: "ias_kt", label: "Indicated airspeed" }, { key: "gs_kt", label: "Ground speed", dash: true }] },
  { id: "flap", title: "Flaps", unit: "°", height: 80, digits: 0, step: true, lines: [{ key: "flap_deg", label: "Flap angle" }] },
  { id: "vs", title: "Vertical speed", unit: "fpm", height: 120, digits: 0, zero: true, refs: () => [SINK_LIMIT], lines: [{ key: "vs_fpm", label: "Climb + / descent −" }] },
  { id: "g", title: "Normal load", unit: "g", height: 120, digits: 2, centre: 1, refs: (flight) => [ONE_G, ...[hardLanding(flight)].filter((r) => r !== null)], lines: [{ key: "nz_g", label: "Load factor" }] },
  { id: "att", title: "Attitude", unit: "°", height: 130, digits: 1, zero: true, lines: [{ key: "pitch_deg", label: "Pitch, nose up +" }, { key: "roll_deg", label: "Roll, right +", dash: true }] },
];

const AXIS_WIDTH = 58;

const Swatch = ({ color, dash, width }: { color: string; dash?: boolean; width: number }) => (
  <svg width={16} height={6} aria-hidden="true" className="shrink-0"><line x1={0} x2={16} y1={3} y2={3} stroke={color} strokeWidth={width} strokeDasharray={dash ? "4 2" : undefined} /></svg>
);

/** What every chart draws besides its lines; read at draw time so a change needs only a redraw. */
interface Overlay { theme: Theme; phases: Debrief["phases"]; touchdowns: number[]; selected: FlightEvent | null; focus: number | null }
/** State the charts share outside React: latest props, the live uPlot instances and the one under the pointer. */
interface Shared { overlay: Overlay; onView: (range: TimeRange) => void; onFocus: (t: number) => void; range: TimeRange; plots: Map<string, uPlot>; hovered: uPlot | null }

interface Props {
  debrief: Debrief;
  theme: Theme;
  view: TimeRange | null;
  selected: FlightEvent | null;
  bus: TimeBus;
  onView: (range: TimeRange) => void;
  onFocus: (t: number) => void;
}

/**
 * Stacked time-series charts on one time axis (uPlot: canvas, fast with tens of thousands of points).
 * The cursor is synced across charts, the map and the timeline; drag across a chart to zoom all of them.
 */
export default function Charts({ debrief, theme, view, selected, bus, onView, onFocus }: Props) {
  const refs = useMemo(() => SPECS.map((spec) => spec.refs?.(debrief.flight) ?? []), [debrief.flight]);
  const fullRange = useMemo<TimeRange>(() => [debrief.series.t_s[0] ?? 0, debrief.series.t_s.at(-1) ?? 1], [debrief.series.t_s]);
  const shared = useRef<Shared>({
    overlay: { theme, phases: debrief.phases, touchdowns: [], selected, focus: bus.placed },
    onView, onFocus, range: view ?? fullRange, plots: new Map(), hovered: null,
  });
  // Latest props for the uPlot hooks, which live outside React.
  useLayoutEffect(() => {
    Object.assign(shared.current, { overlay: { theme, phases: debrief.phases, touchdowns: debrief.touchdowns.map((td) => td.t_s), selected, focus: bus.placed }, onView, onFocus, range: view ?? fullRange });
  });

  // Overlay or zoom changed: redraw (cheap) or rescale every chart.
  useEffect(() => {
    const [min, max] = view ?? fullRange;
    shared.current.plots.forEach((plot) => plot.setScale("x", { min, max }));
  }, [view, fullRange]);
  useEffect(() => { shared.current.plots.forEach((plot) => plot.redraw(false, false)); }, [selected]);
  // The placed cursor (playback too) redraws the charts without re-rendering them.
  useEffect(() => bus.onPlace(() => {
    shared.current.overlay.focus = bus.placed;
    shared.current.plots.forEach((plot) => plot.redraw(false, false));
  }), [bus]);

  // A time hovered elsewhere (timeline, another chart) moves every chart's cursor.
  useEffect(() => bus.subscribe((t) => {
    if (shared.current.hovered) return; // uPlot's own sync already moves the other charts
    shared.current.plots.forEach((plot) => {
      if (t === null) plot.setCursor({ left: -10, top: -10 }, false);
      else plot.setCursor({ left: plot.valToPos(t, "x"), top: 10 }, false);
    });
  }), [bus]);

  return (
    <div className="flex flex-col">
      {SPECS.map((spec, index) => (
        <Chart key={`${spec.id}-${theme}`} spec={spec} refs={refs[index]!} last={index === SPECS.length - 1} debrief={debrief} theme={theme} bus={bus} shared={shared} />
      ))}
    </div>
  );
}

interface ChartProps {
  spec: Spec;
  refs: Mark[];
  last: boolean;
  debrief: Debrief;
  theme: Theme;
  bus: TimeBus;
  shared: RefObject<Shared>;
}

function Chart({ spec, refs, last, debrief, theme, bus, shared }: ChartProps) {
  const host = useRef<HTMLDivElement>(null);
  const values = useRef<(HTMLSpanElement | null)[]>([]);
  const { series } = debrief;
  const hasData = spec.lines.some((line) => series[line.key].some((v) => v !== null));
  const hasLimit = !!spec.limit && !!series[spec.limit.key]?.some((v) => v !== null);
  const fmt = useCallback((v: number) => number(v, spec.digits), [spec.digits]);
  // At rest a legend entry shows its range over the flight; under the cursor, the value at that time.
  const rest = useMemo(() => spec.lines.map((line) => {
    const known = series[line.key].filter((v) => v !== null);
    return known.length ? `${fmt(Math.min(...known))} to ${fmt(Math.max(...known))}` : "–";
  }), [series, spec.lines, fmt]);
  // Each value keeps the room of its range, longer than any one value with its unit, so the legend never rewraps as the cursor moves.
  const room = useMemo(() => rest.map((r) => `${r.length}ch`), [rest]);
  useHover(bus, useCallback((t: number | null) => {
    const i = t === null ? -1 : indexAt(series.t_s, t);
    spec.lines.forEach((line, n) => {
      const node = values.current[n], v = series[line.key][i];
      if (node) node.textContent = i < 0 ? rest[n]! : v == null ? "–" : `${fmt(v)}${spec.unit === "°" ? "" : " "}${spec.unit}`;
    });
  }, [series, spec, rest, fmt]));

  useEffect(() => {
    const node = host.current;
    if (!node || !hasData) return;
    const refColor = REF[theme];
    const text = cssVar("--text-muted"), grid = cssVar("--border-subtle"), axisLine = cssVar("--border"), fg = cssVar("--foreground");
    const font = `11px ${cssVar("--font-display") || "monospace"}`;
    const data: uPlot.AlignedData = [debrief.series.t_s, ...spec.lines.map((line) => debrief.series[line.key]), ...(hasLimit ? [debrief.series[spec.limit!.key]] : [])];

    const shade: uPlot.Plugin = {
      hooks: {
        // Under the lines: phase bands, then the selected event's span.
        drawClear: (u) => {
          const { ctx, bbox } = u;
          const { phases, selected, theme: t } = shared.current.overlay;
          const x = (v: number) => u.valToPos(v, "x", true);
          ctx.save();
          for (const phase of phases) {
            const x0 = Math.max(bbox.left, x(phase.start_s)), x1 = Math.min(bbox.left + bbox.width, x(phase.end_s));
            if (x1 <= x0) continue;
            ctx.fillStyle = alpha(phaseColor(phase.phase, t), t === "dark" ? 0.1 : 0.08);
            ctx.fillRect(x0, bbox.top, x1 - x0, bbox.height);
          }
          if (selected) {
            const x0 = x(selected.start_s), x1 = Math.max(x0 + 3 * devicePixelRatio, x(selected.end_s));
            ctx.fillStyle = alpha(levelColor(selected.level, t), 0.22);
            ctx.fillRect(x0, bbox.top, x1 - x0, bbox.height);
          }
          ctx.restore();
        },
        // Over the lines: reference lines, touchdowns and the focused time.
        draw: (u) => {
          const { ctx, bbox } = u;
          ctx.save();
          ctx.font = `${10 * devicePixelRatio}px ${cssVar("--font-display") || "monospace"}`;
          ctx.textAlign = "right"; ctx.textBaseline = "bottom";
          for (const ref of refs) {
            const py = Math.round(u.valToPos(ref.at, "y", true));
            if (py < bbox.top || py > bbox.top + bbox.height) continue;
            ctx.strokeStyle = refColor; ctx.lineWidth = devicePixelRatio; ctx.setLineDash([2 * devicePixelRatio, 3 * devicePixelRatio]);
            ctx.beginPath(); ctx.moveTo(bbox.left, py); ctx.lineTo(bbox.left + bbox.width, py); ctx.stroke();
            ctx.fillStyle = refColor;
            ctx.fillText(`${number(ref.at, Number.isInteger(ref.at) ? 0 : 2)} ${spec.unit}`, bbox.left + bbox.width - 4 * devicePixelRatio, py - 2 * devicePixelRatio);
          }
          ctx.restore();
          const { touchdowns, focus } = shared.current.overlay;
          const vline = (t: number, color: string, width: number, dash: number[]) => {
            const px = Math.round(u.valToPos(t, "x", true));
            if (px < bbox.left || px > bbox.left + bbox.width) return;
            ctx.strokeStyle = color; ctx.lineWidth = width * devicePixelRatio; ctx.setLineDash(dash.map((d) => d * devicePixelRatio));
            ctx.beginPath(); ctx.moveTo(px, bbox.top); ctx.lineTo(px, bbox.top + bbox.height); ctx.stroke();
          };
          ctx.save();
          touchdowns.forEach((t) => vline(t, text, 1, [3, 3]));
          if (focus !== null) vline(focus, fg, 1.5, []);
          ctx.restore();
        },
      },
    };

    let downX = 0;
    const options: uPlot.Options = {
      width: node.clientWidth,
      height: spec.height,
      plugins: [shade],
      scales: { x: { time: false, min: shared.current.range[0], max: shared.current.range[1] } },
      cursor: {
        sync: { key: "foqa-charts", setSeries: false },
        drag: { x: true, y: false, setScale: false },
        points: { size: 6 },
      },
      select: { show: true, left: 0, top: 0, width: 0, height: 0 },
      legend: { show: false },
      axes: [
        { stroke: text, font, size: last ? 28 : 6, grid: { stroke: grid, width: 1 }, ticks: { stroke: axisLine, width: 1, size: last ? 4 : 0 }, values: (_, splits) => splits.map((v) => (last ? clock(v) : "")), space: 70 },
        { stroke: text, font, size: AXIS_WIDTH, grid: { stroke: grid, width: 1 }, ticks: { show: false }, values: (_, splits) => splits.map((v) => v.toLocaleString("en-US", { maximumFractionDigits: spec.digits > 1 ? 1 : 0 })) },
      ],
      series: [
        {},
        ...spec.lines.map((line, i) => ({
          label: line.label,
          stroke: LINE[theme][i] ?? LINE[theme][0],
          width: 1.5,
          dash: line.dash ? [5, 3] : undefined,
          paths: spec.step ? uPlot.paths.stepped?.({ align: 1 }) : undefined,
          points: { show: false },
        })),
        ...(hasLimit ? [{ label: spec.limit!.label, stroke: refColor, width: 1.25, dash: [2, 3], paths: uPlot.paths.stepped?.({ align: 1 }), points: { show: false } }] : []),
      ],
      hooks: {
        setSelect: [(u) => {
          if (u.select.width > 4) shared.current.onView([u.posToVal(u.select.left, "x"), u.posToVal(u.select.left + u.select.width, "x")]);
          u.setSelect({ left: 0, top: 0, width: 0, height: 0 }, false);
        }],
        setCursor: [(u) => {
          if (shared.current.hovered !== u) return;
          const idx = u.cursor.idx;
          bus.emit(idx == null || (u.cursor.left ?? -1) < 0 ? null : debrief.series.t_s[idx] ?? null);
        }],
        ready: [(u) => {
          u.over.addEventListener("mousedown", (e) => { downX = e.clientX; });
          u.over.addEventListener("click", (e) => { if (Math.abs(e.clientX - downX) < 4) shared.current.onFocus(u.posToVal(e.offsetX, "x")); });
          u.over.addEventListener("mouseenter", () => { shared.current.hovered = u; });
          u.over.addEventListener("mouseleave", () => { shared.current.hovered = null; bus.emit(null); });
        }],
      },
    };
    const refYs = spec.centre === undefined ? refs.map((r) => r.at) : [];
    options.scales!.y = { range: (_, min, max) => {
      if (spec.centre !== undefined) { const c = spec.centre, span = Math.max(c - min, max - c, 0.25) * 1.1; return [c - span, c + span]; }
      return uPlot.rangeNum(Math.min(min, ...refYs, spec.zero ? -1 : Infinity), Math.max(max, ...refYs, spec.zero ? 1 : -Infinity), 0.1, true);
    } };

    const plot = new uPlot(options, data, node);
    const { plots } = shared.current;
    plots.set(spec.id, plot);
    const observer = new ResizeObserver(() => plot.setSize({ width: node.clientWidth, height: spec.height }));
    observer.observe(node);
    return () => { observer.disconnect(); plots.delete(spec.id); plot.destroy(); };
    // Rebuilt only when the data or theme changes; zoom and overlays update the live instance through `shared`.
  }, [debrief, theme, hasData, hasLimit, spec, refs, last, bus, shared]);

  return (
    <section className="border-b border-line-subtle py-2 last:border-b-0" aria-label={`${spec.title} chart`}>
      <div className="flex flex-wrap items-baseline gap-x-4 gap-y-0.5 px-1">
        <h3 className="eyebrow">{spec.title} <span className="normal-case tracking-normal">({spec.unit})</span></h3>
        {hasData ? (
          <ul className="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px]" aria-label={`${spec.title} legend`}>
            {spec.lines.map((line, i) => (
              <li key={line.key} className="flex items-center gap-1.5">
                <Swatch color={LINE[theme][i] ?? LINE[theme][0]} dash={line.dash} width={2} />
                <span className="text-fg-3">{line.label}</span>
                <span ref={(node) => { values.current[i] = node; }} className="readout inline-block text-fg" style={{ minWidth: room[i] }}>{rest[i]}</span>
              </li>
            ))}
            {hasLimit ? (
              <li className="flex items-center gap-1.5 text-fg-3">
                <Swatch color={REF[theme]} dash width={1.25} />
                <Tooltip content={spec.limit!.source}><button type="button" className="cursor-help underline decoration-dotted underline-offset-2">{spec.limit!.label}</button></Tooltip>
              </li>
            ) : null}
            {refs.map((ref) => (
              <li key={ref.label} className="flex items-center gap-1.5 text-fg-3">
                <Swatch color={REF[theme]} dash width={1.25} />
                {ref.source ? <Tooltip content={ref.source}><button type="button" className="cursor-help underline decoration-dotted underline-offset-2">{ref.label}</button></Tooltip> : <span>{ref.label}</span>}
              </li>
            ))}
          </ul>
        ) : null}
      </div>
      {hasData ? <div ref={host} className="foqa-chart" /> : <p className="px-1 py-4 text-xs text-fg-3">Not recorded on this flight.</p>}
    </section>
  );
}
