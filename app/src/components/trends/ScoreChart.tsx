import { useEffect, useMemo, useRef, useState } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import { aircraftName, flightDate, number } from "../../lib/format";
import { cssVar, type Theme } from "../../lib/palette";
import { openFlight } from "../../lib/route";
import { GRADE_VAR, rollingMean, scoredLandings } from "../../lib/trends";
import type { TrendLanding } from "../../types";

const WINDOW = 5;
const HEIGHT = 240;
const HIT = 12;

/** Scored landings over time: a dot per landing coloured by grade, the rolling mean as a line; click a dot to open its flight. */
export function ScoreChart({ landings, theme }: { landings: TrendLanding[]; theme: Theme }) {
  const host = useRef<HTMLDivElement>(null);
  const rows = useMemo(() => scoredLandings(landings).filter((l) => l.started_utc), [landings]);
  const [hover, setHover] = useState<{ row: TrendLanding; x: number; y: number; width: number } | null>(null);

  useEffect(() => {
    const node = host.current;
    if (!node || !rows.length) return;
    const xs = rows.map((l) => Date.parse(l.started_utc!) / 1000 + l.t_s);
    const ys = rows.map((l) => l.score!);
    const mean = rollingMean(ys, WINDOW);
    const text = cssVar("--text-muted"), grid = cssVar("--border-subtle"), axisLine = cssVar("--border"), line = cssVar("--accent");
    const font = `11px ${cssVar("--font-display") || "monospace"}`;
    const dot = (row: TrendLanding) => cssVar(GRADE_VAR[row.grade ?? "fair"]);
    const nearest = (u: uPlot, left: number, top: number) => {
      let best = -1, bestD = HIT;
      xs.forEach((x, i) => {
        const d = Math.hypot(u.valToPos(x, "x") - left, u.valToPos(ys[i]!, "y") - top);
        if (d < bestD) { best = i; bestD = d; }
      });
      return best;
    };
    let downX = 0;
    const plot = new uPlot({
      width: node.clientWidth, height: HEIGHT,
      scales: { x: { time: true }, y: { range: (_, min, max) => [Math.max(0, Math.floor((min - 6) / 10) * 10), Math.min(100, Math.ceil((max + 3) / 10) * 10)] } },
      legend: { show: false },
      cursor: { drag: { x: false, y: false }, points: { show: false }, x: false, y: false },
      axes: [
        { stroke: text, font, grid: { stroke: grid, width: 1 }, ticks: { stroke: axisLine, width: 1, size: 4 } },
        { stroke: text, font, size: 40, grid: { stroke: grid, width: 1 }, ticks: { show: false } },
      ],
      series: [{}, { label: `Rolling mean of ${WINDOW}`, stroke: line, width: 2, points: { show: false }, spanGaps: true }],
      hooks: {
        draw: [(u) => {
          const { ctx } = u;
          ctx.save();
          xs.forEach((x, i) => {
            const px = u.valToPos(x, "x", true), py = u.valToPos(ys[i]!, "y", true), r = 5 * devicePixelRatio;
            ctx.beginPath(); ctx.arc(px, py, r, 0, 2 * Math.PI);
            ctx.fillStyle = dot(rows[i]!); ctx.fill();
            ctx.lineWidth = 1.5 * devicePixelRatio; ctx.strokeStyle = cssVar("--surface"); ctx.stroke();
          });
          ctx.restore();
        }],
        ready: [(u) => {
          u.over.addEventListener("mousedown", (e) => { downX = e.clientX; });
          u.over.addEventListener("click", (e) => {
            const i = Math.abs(e.clientX - downX) < 4 ? nearest(u, e.offsetX, e.offsetY) : -1;
            if (i >= 0) openFlight(rows[i]!.flight_id);
          });
          u.over.addEventListener("mousemove", (e) => {
            const i = nearest(u, e.offsetX, e.offsetY);
            u.over.style.cursor = i >= 0 ? "pointer" : "default";
            setHover(i >= 0 ? { row: rows[i]!, x: u.valToPos(xs[i]!, "x"), y: u.valToPos(ys[i]!, "y"), width: u.over.clientWidth } : null);
          });
          u.over.addEventListener("mouseleave", () => setHover(null));
        }],
      },
    }, [xs, mean], node);
    const observer = new ResizeObserver(() => plot.setSize({ width: node.clientWidth, height: HEIGHT }));
    observer.observe(node);
    return () => { observer.disconnect(); plot.destroy(); };
  }, [rows, theme]);

  return (
    <div className="px-2 pb-3 pt-3">
      <div className="relative">
        <div ref={host} className="foqa-chart" role="img" aria-label={`Landing score over time, ${rows.length} landings`} />
        {hover ? (
          <div className="pointer-events-none absolute z-10 -translate-x-1/2 -translate-y-full whitespace-nowrap rounded-md border border-line bg-surface-raised px-2.5 py-1.5 text-xs shadow-lg"
            style={{ left: Math.min(Math.max(hover.x + 48, 80), hover.width - 80), top: hover.y + 4 }}>
            <span className="readout font-medium">{number(hover.row.score!, 1)}</span>
            <span className="ml-2 text-fg-3">{aircraftName(hover.row.aircraft ? { icao: hover.row.aircraft, title: "" } : undefined)} · {hover.row.icao ?? "no runway"}{hover.row.runway ? ` ${hover.row.runway}` : ""} · {flightDate(hover.row.started_utc)}</span>
          </div>
        ) : null}
      </div>
      <ul className="mt-1 flex flex-wrap items-center gap-x-4 gap-y-1 px-2 text-[11px] text-fg-3" aria-label="Chart legend">
        {(["good", "fair", "poor"] as const).map((g) => (
          <li key={g} className="flex items-center gap-1.5"><span className="size-2.5 rounded-full" style={{ background: `var(${GRADE_VAR[g]})` }} aria-hidden="true" />{g[0]!.toUpperCase() + g.slice(1)}</li>
        ))}
        <li className="flex items-center gap-1.5"><span className="h-0.5 w-4 rounded bg-accent" aria-hidden="true" />Rolling mean of {WINDOW}</li>
        <li className="ml-auto">Click a dot to open its flight</li>
      </ul>
    </div>
  );
}

