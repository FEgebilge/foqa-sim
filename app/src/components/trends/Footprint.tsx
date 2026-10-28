import { useEffect, useMemo, useRef, useState } from "react";
import { aircraftName, flightDate, lengthM, number } from "../../lib/format";
import { openFlight } from "../../lib/route";
import { GRADE_VAR } from "../../lib/trends";
import type { TrendLanding } from "../../types";

const HEIGHT = 220;
const PAD = { l: 44, r: 14, t: 12, b: 30 };
/** Touchdown zone length and aiming point past the threshold when no landing names its runway's (ICAO Annex 14). */
const DEFAULT_TDZ = 900;
const DEFAULT_AIMING = 400;
const HALF_WIDTH = 22.5;

const step = (span: number) => [50, 100, 200, 250, 500].find((s) => span / s <= 9) ?? 1000;

/** The middle value of the runways' own figures, so one runway draws as it is and a mix as its typical one. */
const median = (values: (number | undefined)[], fallback: number) => {
  const v = values.filter((x): x is number => x !== undefined).sort((a, b) => a - b);
  return v.length ? v[Math.floor(v.length / 2)]! : fallback;
};

/** Touchdown dispersion in runway coordinates: runway drawn horizontally, threshold at the left, landing direction left to right.
 * The across-runway scale is stretched so the spread stays readable. */
export function Footprint({ landings }: { landings: TrendLanding[] }) {
  const box = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(600);
  useEffect(() => {
    const node = box.current;
    if (!node) return;
    setWidth(node.clientWidth);
    const observer = new ResizeObserver(() => setWidth(node.clientWidth));
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  const rows = useMemo(() => landings.filter((l) => l.along_m !== undefined && l.cross_m !== undefined), [landings]);
  const tdz = median(rows.map((l) => l.tdz_m), DEFAULT_TDZ), aiming = median(rows.map((l) => l.aiming_point_m), DEFAULT_AIMING);
  const along = rows.map((l) => l.along_m!), cross = rows.map((l) => l.cross_m!);
  const x0 = Math.min(0, Math.floor((Math.min(...along, 0) - 40) / 100) * 100), x1 = Math.max(1200, Math.ceil((Math.max(...along, 0) + 80) / 100) * 100);
  const ymax = Math.max(15, Math.ceil(Math.max(...cross.map(Math.abs), 0) + 3));
  const sx = (v: number) => PAD.l + ((v - x0) / (x1 - x0)) * (width - PAD.l - PAD.r);
  const sy = (v: number) => PAD.t + ((v + ymax) / (2 * ymax)) * (HEIGHT - PAD.t - PAD.b);
  const xTicks = Array.from({ length: Math.floor((x1 - x0) / step(x1 - x0)) + 1 }, (_, i) => Math.ceil(x0 / step(x1 - x0)) * step(x1 - x0) + i * step(x1 - x0)).filter((v) => v <= x1);
  const yTicks = [-10, 0, 10].filter((v) => Math.abs(v) <= ymax);
  const surfaceTop = sy(Math.max(-HALF_WIDTH, -ymax)), surfaceBottom = sy(Math.min(HALF_WIDTH, ymax));
  const inTdz = rows.filter((l) => l.zone === "In TDZ").length;
  const count = (zone: string) => rows.filter((l) => l.zone === zone).length;
  const meanCross = cross.length ? cross.reduce((a, b) => a + b, 0) / cross.length : 0;

  return (
    <div className="px-2 pb-3 pt-3">
      <div ref={box} className="w-full">
        <svg width={width} height={HEIGHT} role="img" aria-label={`Touchdown footprint, ${rows.length} landings`} className="block">
          {x0 < 0 ? <rect x={PAD.l} y={PAD.t} width={sx(0) - PAD.l} height={HEIGHT - PAD.t - PAD.b} fill="var(--surface-muted)" opacity={0.6} /> : null}
          <rect x={sx(0)} y={surfaceTop} width={sx(x1) - sx(0)} height={surfaceBottom - surfaceTop} fill="var(--surface-muted)" />
          <rect x={sx(0)} y={PAD.t} width={sx(tdz) - sx(0)} height={HEIGHT - PAD.t - PAD.b} fill="var(--accent-subtle)" />
          <text x={sx(0) + 6} y={PAD.t + 11} className="fill-fg-3" fontSize={10}>Touchdown zone</text>
          {yTicks.map((v) => <line key={v} x1={PAD.l} x2={width - PAD.r} y1={sy(v)} y2={sy(v)} stroke="var(--border-subtle)" strokeWidth={1} />)}
          <line x1={sx(0)} x2={sx(x1)} y1={sy(0)} y2={sy(0)} stroke="var(--text-muted)" strokeWidth={1} strokeDasharray="8 6" />
          <line x1={sx(aiming)} x2={sx(aiming)} y1={PAD.t} y2={HEIGHT - PAD.b} stroke="var(--text-muted)" strokeWidth={1} strokeDasharray="2 3" />
          <text x={sx(aiming) + 4} y={HEIGHT - PAD.b - 4} className="fill-fg-3" fontSize={10}>Aiming point</text>
          <line x1={sx(0)} x2={sx(0)} y1={PAD.t} y2={HEIGHT - PAD.b} stroke="var(--foreground)" strokeWidth={3} />
          {x0 < -40 ? <text x={sx(0) - 6} y={PAD.t + 11} textAnchor="end" className="fill-fg-2" fontSize={10}>Threshold</text> : null}
          {xTicks.map((v) => (
            <g key={v}>
              <line x1={sx(v)} x2={sx(v)} y1={HEIGHT - PAD.b} y2={HEIGHT - PAD.b + 4} stroke="var(--border)" />
              <text x={sx(v)} y={HEIGHT - PAD.b + 16} textAnchor="middle" className="readout fill-fg-3" fontSize={11}>{v}</text>
            </g>
          ))}
          {yTicks.map((v) => <text key={v} x={PAD.l - 8} y={sy(v) + 4} textAnchor="end" className="readout fill-fg-3" fontSize={11}>{v > 0 ? `R ${v}` : v < 0 ? `L ${-v}` : "0"}</text>)}
          {rows.map((l) => (
            <g key={`${l.flight_id}-${l.t_s}`} tabIndex={0} role="link" aria-label={`Open flight of ${flightDate(l.started_utc)}`} className="cursor-pointer outline-none [&:focus-visible>circle]:stroke-[var(--foreground)]"
              onClick={() => openFlight(l.flight_id)} onKeyDown={(e) => { if (e.key === "Enter") openFlight(l.flight_id); }}>
              <circle cx={sx(l.along_m!)} cy={sy(l.cross_m!)} r={5.5} fill={l.grade ? `var(${GRADE_VAR[l.grade]})` : "var(--text-muted)"} fillOpacity={0.9} stroke="var(--surface)" strokeWidth={1.5} />
              <title>{`${aircraftName(l.aircraft ? { icao: l.aircraft, title: "" } : undefined)} · ${flightDate(l.started_utc)}\n${lengthM(l.along_m!)} past the threshold, ${lengthM(Math.abs(l.cross_m!), 1)} ${l.cross_m! >= 0 ? "right" : "left"}${l.score !== undefined ? `\nScore ${number(l.score, 1)}` : ""}`}</title>
            </g>
          ))}
        </svg>
      </div>
      <dl className="mt-1 flex flex-wrap items-center gap-x-5 gap-y-1 px-2 text-[11px] text-fg-3">
        <div className="flex gap-1.5"><dt>In zone</dt><dd className="readout text-fg">{rows.length ? `${Math.round((100 * inTdz) / rows.length)}%` : "–"}</dd></div>
        <div className="flex gap-1.5"><dt>Long</dt><dd className="readout text-fg">{count("Long")}</dd></div>
        <div className="flex gap-1.5"><dt>Short</dt><dd className="readout text-fg">{count("Short")}</dd></div>
        <div className="flex gap-1.5"><dt>Mean offset</dt><dd className="readout text-fg">{lengthM(Math.abs(meanCross), 1)} {meanCross >= 0 ? "right" : "left"}</dd></div>
        <p className="ml-auto">Metres past the threshold; across-runway scale stretched</p>
      </dl>
    </div>
  );
}
