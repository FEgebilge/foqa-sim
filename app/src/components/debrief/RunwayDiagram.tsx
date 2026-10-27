import { M_PER_FT, number } from "../../lib/format";
import type { RunwayFix } from "../../types";

const W = 360, X0 = 30, X1 = W - 6;
// Top view rows: labels, runway, touchdown zone bracket, distance bracket.
const LABEL_Y = 10, TOP = 16, HALF = 13, MID = TOP + HALF, ZONE_Y = TOP + 2 * HALF + 8, DIST_Y = ZONE_Y + 22, H = DIST_Y + 14;
const STRIP_H = 30;

/**
 * The landing runway, threshold on the left. Top view: length to scale (touchdown zone, aiming point, touchdown,
 * runway left ahead); width exaggerated so the marks fit. Strip below: the width to true scale, for the centreline offset.
 */
export function RunwayDiagram({ fix, widthM }: { fix: RunwayFix; widthM: number }) {
  const length = Math.max(fix.lda_m, fix.along_m + 50);
  const x = (along: number) => X0 + (Math.max(-0.05 * length, along) / length) * (X1 - X0);
  const half = widthM / 2;
  const mark = fix.in_tdz ? "var(--success)" : fix.short_landing ? "var(--level-high)" : "var(--level-medium)";
  const ap = fix.aiming_point_m, tdz = Math.min(fix.tdz_m, fix.lda_m), remaining = fix.lda_m - fix.along_m;
  const tx = x(fix.along_m), ty = MID + (Math.max(-1.4 * half, Math.min(1.4 * half, fix.cross_m)) / half) * HALF;
  const side = fix.cross_m === 0 ? "on the centreline" : `${number(Math.abs(fix.cross_m), 1)} m ${fix.cross_m > 0 ? "right" : "left"} of the centreline`;
  const apLen = Math.max(3, x(ap + 45) - x(ap));
  // Strip: the runway width across the full drawing width, so 1 m reads the same left and right.
  const sx = (cross: number) => X0 + ((Math.max(-1.3 * half, Math.min(1.3 * half, cross)) + 1.3 * half) / (2.6 * half)) * (X1 - X0);

  return (
    <figure className="flex max-w-[480px] flex-col gap-1">
      <svg viewBox={`0 0 ${W} ${H}`} className="w-full" role="img" aria-label={`Runway ${fix.runway}, top view: touchdown ${number(fix.along_m)} m past the threshold, ${number(remaining)} m of runway left ahead; touchdown zone ${number(fix.tdz_m)} m; aiming point ${ap} m.`}>
        {/* Runway to the landing distance available, touchdown zone tinted */}
        <rect x={X0} y={TOP} width={X1 - X0} height={HALF * 2} rx={2} fill="var(--surface-muted)" stroke="var(--border-strong)" />
        <rect x={X0} y={TOP} width={x(tdz) - X0} height={HALF * 2} fill="color-mix(in oklab, var(--success) 14%, transparent)" />
        {Array.from({ length: 5 }, (_, i) => <rect key={i} x={X0 + 2} y={TOP + 3 + i * 4.8} width={7} height={2.4} fill="var(--text-muted)" />)}
        <text x={X0 - 5} y={MID + 4} textAnchor="end" className="readout" fontSize={11} fontWeight={600} fill="var(--foreground)">{fix.runway}</text>
        <line x1={X0 + 12} x2={X1 - 3} y1={MID} y2={MID} stroke="var(--text-muted)" strokeDasharray="7 5" strokeWidth={1} />
        {/* Aiming point markings (ICAO Annex 14), both sides of the centreline */}
        <rect x={x(ap)} y={TOP + 3} width={apLen} height={5} fill="var(--foreground)" opacity={0.75} />
        <rect x={x(ap)} y={TOP + 2 * HALF - 8} width={apLen} height={5} fill="var(--foreground)" opacity={0.75} />
        <text x={x(ap)} y={LABEL_Y} className="readout" fontSize={9.5} fill="var(--text-secondary)">Aiming point {ap} m</text>
        <text x={X1} y={LABEL_Y} textAnchor="end" className="readout" fontSize={9.5} fill="var(--text-muted)">Runway {number(fix.lda_m)} m</text>
        {/* Touchdown point */}
        <line x1={tx} x2={tx} y1={ty} y2={DIST_Y} stroke={mark} strokeDasharray="2 2" />
        <circle cx={tx} cy={ty} r={5} fill={mark} stroke="var(--surface)" strokeWidth={2} />
        {/* Touchdown zone */}
        <Bracket x0={X0} x1={x(tdz)} y={ZONE_Y} color="var(--success)" label={`Touchdown zone, first ${number(fix.tdz_m)} m`} />
        {/* Threshold to touchdown, touchdown to the end */}
        <Bracket x0={X0} x1={tx} y={DIST_Y} color="var(--foreground)" label={`${number(fix.along_m)} m`} />
        {remaining > 0 ? <Bracket x0={tx} x1={X1} y={DIST_Y} color="var(--text-muted)" label={`${number(remaining)} m left`} /> : null}
      </svg>

      <svg viewBox={`0 0 ${W} ${STRIP_H}`} className="w-full" role="img" aria-label={`Across the runway, to scale: touchdown ${side}; the runway is ${number(widthM)} m wide.`}>
        <rect x={sx(-half)} y={6} width={sx(half) - sx(-half)} height={12} fill="var(--surface-muted)" stroke="var(--border-strong)" />
        <line x1={sx(0)} x2={sx(0)} y1={4} y2={20} stroke="var(--text-muted)" strokeDasharray="3 2" />
        <circle cx={sx(fix.cross_m)} cy={12} r={4.5} fill={mark} stroke="var(--surface)" strokeWidth={1.5} />
        <text x={sx(-half) - 4} y={15} textAnchor="end" className="readout" fontSize={9.5} fill="var(--text-muted)">L</text>
        <text x={sx(half) + 4} y={15} className="readout" fontSize={9.5} fill="var(--text-muted)">R</text>
        <text x={W / 2} y={STRIP_H - 1} textAnchor="middle" className="readout" fontSize={9.5} fill="var(--text-secondary)">
          {side} · {number(Math.abs(fix.cross_m) / M_PER_FT)} ft · runway {number(widthM)} m wide
        </text>
      </svg>
      <figcaption className="text-[10.5px] text-fg-3">Top view: length to scale, width exaggerated. Strip: across the runway, to scale.</figcaption>
    </figure>
  );
}

/** A dimension line with end ticks and a centred label; the label moves outside when the span is too short. */
function Bracket({ x0, x1, y, color, label }: { x0: number; x1: number; y: number; color: string; label: string }) {
  const wide = x1 - x0 > label.length * 5.6 + 8;
  return (
    <g>
      <line x1={x0} x2={x1} y1={y} y2={y} stroke={color} strokeWidth={1} />
      <line x1={x0} x2={x0} y1={y - 3} y2={y + 3} stroke={color} />
      <line x1={x1} x2={x1} y1={y - 3} y2={y + 3} stroke={color} />
      <text x={wide ? (x0 + x1) / 2 : x1 + 4} y={y + 11} textAnchor={wide ? "middle" : "start"} className="readout" fontSize={9.5} fill="var(--text-secondary)">{label}</text>
    </g>
  );
}
