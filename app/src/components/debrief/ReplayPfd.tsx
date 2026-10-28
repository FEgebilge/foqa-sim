import { useState, type ReactNode } from "react";
import { clock, number } from "../../lib/format";
import type { Theme } from "../../lib/palette";
import { DERIVED, instrumentsAt, type Instruments } from "../../lib/series";
import { useHover, usePlaced, type TimeBus } from "../../lib/time";
import type { Debrief } from "../../types";

// A primary flight display rebuilt from the recorded parameters at the cursor, for the replay. Deliberately generic
// (no manufacturer's symbology): attitude, speed and altitude tapes, vertical speed, heading, configuration and automation.
// Every value comes from the debrief series; a missing one reads "no data", a derived one carries a dagger.

const W = 480, H = 356;
const CY = 154, TOP = 44, BOTTOM = 264;
const SPEED = { x: 10, w: 70, px: 4 };
const ATT = { x: 94, w: 232, cx: 210, px: 4.4 };
const ALT = { x: 340, w: 82, px: 0.4 };
const VS = { x: 430, w: 40 };
const HDG = { y: 278, h: 34, px: 4 };
const MONO = "var(--font-display)";
/** Conventional sky/ground, a little deeper at night; the magenta of a selected-speed bug. */
const COLOURS = {
  light: { sky: "#3f7fc4", ground: "#8a5a2b", bug: "#b0359f" },
  dark: { sky: "#2b5c93", ground: "#6b4422", bug: "#e27df0" },
} as const;

const DAGGER = "†";
const NO_DATA = "no data";

/** Text in the instrument font. */
function T({ x, y, children, size = 11, anchor = "middle", fill = "var(--foreground)", weight, title }: { x: number; y: number; children: ReactNode; size?: number; anchor?: "start" | "middle" | "end"; fill?: string; weight?: number; title?: string }) {
  return (
    <text x={x} y={y} fontSize={size} textAnchor={anchor} dominantBaseline="central" fill={fill} fontWeight={weight} style={{ fontFamily: MONO, fontVariantNumeric: "tabular-nums" }}>
      {title ? <title>{title}</title> : null}
      {children}
    </text>
  );
}

/** A tape's or instrument's "no data" fill. */
const Blank = ({ x, y, w, h, label }: { x: number; y: number; w: number; h: number; label: string }) => (
  <g>
    <rect x={x} y={y} width={w} height={h} fill="var(--surface-muted)" opacity={0.6} />
    <T x={x + w / 2} y={y + h / 2 - 8} size={10} fill="var(--text-muted)">{label}</T>
    <T x={x + w / 2} y={y + h / 2 + 7} size={10} fill="var(--text-muted)">{NO_DATA}</T>
  </g>
);

/** Ticks of a vertical tape around `value`: every `minor`, labelled every `major`, `px` pixels per unit. */
function ticks(value: number, half: number, minor: number, min = -Infinity) {
  const out: number[] = [];
  for (let v = Math.ceil((value - half) / minor) * minor; v <= value + half; v += minor) if (v >= min) out.push(v);
  return out;
}

function SpeedTape({ i, bug }: { i: Instruments; bug: string }) {
  const { x, w, px } = SPEED;
  if (i.ias === null) return <Blank x={x} y={TOP} w={w} h={BOTTOM - TOP} label="IAS" />;
  const ias = i.ias;
  const y = (kt: number) => CY - (kt - ias) * px;
  const clampY = (v: number) => Math.min(BOTTOM, Math.max(TOP, v));
  const half = (BOTTOM - TOP) / 2 / px;
  const band = (from: number, to: number, fill: string, label: string) => {
    const y0 = clampY(y(to)), y1 = clampY(y(from));
    return y1 > y0 ? <rect x={x + w - 7} y={y0} width={7} height={y1 - y0} fill={fill}><title>{label}</title></rect> : null;
  };
  const vapp = i.speeds?.vapp_kt;
  return (
    <g>
      <rect x={x} y={TOP} width={w} height={BOTTOM - TOP} fill="var(--surface-muted)" />
      {i.vmo !== null ? band(i.vmo, ias + half + 5, "var(--danger)", `Above ${i.vmo} kt: the type's maximum speed (profile)`) : null}
      {i.vfe !== null ? band(i.vfe, Math.min(i.vmo ?? Infinity, ias + half + 5), "var(--warning)", `Above ${i.vfe} kt: flap limit (VFE) at ${number(i.flap ?? 0)}° (profile)`) : null}
      {ticks(ias, half, 5, 0).map((kt) => (
        <g key={kt}>
          <line x1={x + w - 7} x2={x + w - (kt % 10 ? 13 : 17)} y1={y(kt)} y2={y(kt)} stroke="var(--foreground)" strokeWidth={1} />
          {kt % 10 === 0 ? <T x={x + w - 22} y={y(kt)} anchor="end" size={11}>{kt}</T> : null}
        </g>
      ))}
      {vapp !== undefined && y(vapp) > TOP && y(vapp) < BOTTOM ? (
        <path d={`M${x + w} ${y(vapp)} l8 -6 v12 z`} fill={bug}><title>{`Vapp ${vapp} kt`}</title></path>
      ) : null}
      <rect x={x + 2} y={CY - 13} width={w - 12} height={26} fill="var(--surface)" stroke="var(--foreground)" strokeWidth={1.2} rx={2} />
      <T x={x + w - 14} y={CY} anchor="end" size={15} weight={600}>{number(ias)}</T>
      <T x={x + w / 2} y={TOP - 9} size={9} fill="var(--text-muted)">IAS kt</T>
    </g>
  );
}

function AltitudeTape({ i }: { i: Instruments }) {
  const { x, w, px } = ALT;
  if (i.alt === null) return <Blank x={x} y={TOP} w={w} h={BOTTOM - TOP} label="ALT" />;
  const alt = i.alt;
  const y = (ft: number) => CY - (ft - alt) * px;
  const half = (BOTTOM - TOP) / 2 / px;
  return (
    <g>
      <rect x={x} y={TOP} width={w} height={BOTTOM - TOP} fill="var(--surface-muted)" />
      {ticks(alt, half, 20).map((ft) => (
        <g key={ft}>
          <line x1={x} x2={x + (ft % 100 ? 6 : 10)} y1={y(ft)} y2={y(ft)} stroke="var(--foreground)" strokeWidth={1} />
          {ft % 100 === 0 ? <T x={x + 14} y={y(ft)} anchor="start" size={11}>{number(ft)}</T> : null}
        </g>
      ))}
      <rect x={x + 2} y={CY - 13} width={w - 4} height={26} fill="var(--surface)" stroke="var(--foreground)" strokeWidth={1.2} rx={2} />
      <T x={x + w - 6} y={CY} anchor="end" size={15} weight={600}>{number(alt)}</T>
      <T x={x + w / 2} y={TOP - 9} size={9} fill="var(--text-muted)">ALT ft (press.)</T>
      {i.ra !== null ? <T x={x + w / 2} y={BOTTOM + 12} size={12} weight={600} title={DERIVED.ra}>RA {number(i.ra)}</T> : null}
    </g>
  );
}

/** Vertical speed on a square-root scale to ±2000 fpm, so small rates near level still read. */
function VerticalSpeed({ i }: { i: Instruments }) {
  const { x, w } = VS;
  const r = (BOTTOM - TOP) / 2 - 10;
  const off = (fpm: number) => Math.sign(fpm) * Math.sqrt(Math.min(2000, Math.abs(fpm)) / 2000) * r;
  const tip = `Vertical speed. ${DERIVED.vs}`;
  return (
    <g>
      <rect x={x} y={TOP} width={w} height={BOTTOM - TOP} fill="var(--surface-muted)" opacity={0.6} />
      {[-2000, -1000, -500, 0, 500, 1000, 2000].map((m) => (
        <g key={m}>
          <line x1={x} x2={x + (m === 0 ? 10 : 6)} y1={CY - off(m)} y2={CY - off(m)} stroke="var(--foreground)" strokeWidth={1} />
          {m && Math.abs(m) !== 500 ? <T x={x + 9} y={CY - off(m)} anchor="start" size={9}>{Math.abs(m) / 1000}</T> : null}
        </g>
      ))}
      <T x={x + w / 2} y={TOP - 9} size={9} fill="var(--text-muted)" title={tip}>{`VS${DAGGER}`}</T>
      {i.vs === null ? <T x={x + w / 2} y={CY} size={9} fill="var(--text-muted)">{NO_DATA}</T> : (
        <g>
          <line x1={x + w} y1={CY} x2={x + 2} y2={CY - off(i.vs)} stroke="var(--foreground)" strokeWidth={2.2}><title>{tip}</title></line>
          <T x={x + w / 2} y={BOTTOM + 12} size={11} weight={600} title={tip}>{`${i.vs > 0 ? "+" : ""}${number(Math.round(i.vs / 10) * 10)}`}</T>
        </g>
      )}
    </g>
  );
}

function Attitude({ i, theme }: { i: Instruments; theme: Theme }) {
  const { x, w, cx, px } = ATT;
  if (i.pitch === null || i.roll === null) return <Blank x={x} y={TOP} w={w} h={BOTTOM - TOP} label="Attitude" />;
  const colour = COLOURS[theme];
  const ladder = [];
  for (let p = -30; p <= 30; p += 2.5) {
    if (p === 0 || Math.abs(p - i.pitch) > 22) continue;
    const half = p % 10 === 0 ? 30 : p % 5 === 0 ? 16 : 7;
    ladder.push(
      <g key={p}>
        <line x1={cx - half} x2={cx + half} y1={CY - p * px} y2={CY - p * px} stroke="#fff" strokeWidth={1.2} />
        {p % 10 === 0 ? <><T x={cx - half - 10} y={CY - p * px} size={10} fill="#fff">{Math.abs(p)}</T><T x={cx + half + 10} y={CY - p * px} size={10} fill="#fff">{Math.abs(p)}</T></> : null}
      </g>,
    );
  }
  const R = 96;
  const tick = (deg: number, long: boolean) => {
    const a = ((deg - 90) * Math.PI) / 180, r2 = R + (long ? 10 : 6);
    return <line key={deg} x1={cx + R * Math.cos(a)} y1={CY + R * Math.sin(a)} x2={cx + r2 * Math.cos(a)} y2={CY + r2 * Math.sin(a)} stroke="#fff" strokeWidth={1.4} />;
  };
  const bank = Math.round(Math.abs(i.roll));
  return (
    <g>
      <defs><clipPath id="pfd-att"><rect x={x} y={TOP} width={w} height={BOTTOM - TOP} rx={6} /></clipPath></defs>
      <g clipPath="url(#pfd-att)">
        <g transform={`rotate(${-i.roll} ${cx} ${CY}) translate(0 ${i.pitch * px})`}>
          <rect x={cx - 400} y={CY - 800} width={800} height={800} fill={colour.sky} />
          <rect x={cx - 400} y={CY} width={800} height={800} fill={colour.ground} />
          <line x1={cx - 400} x2={cx + 400} y1={CY} y2={CY} stroke="#fff" strokeWidth={1.6} />
          {ladder}
        </g>
        {/* Roll scale (fixed) and the bank pointer, which turns with the horizon. */}
        {[-60, -45, -30, -20, -10, 10, 20, 30, 45, 60].map((d) => tick(d, Math.abs(d) === 30 || Math.abs(d) === 60))}
        <path d={`M${cx} ${CY - R} l-6 -9 h12 z`} fill="#fff" />
        <g transform={`rotate(${-i.roll} ${cx} ${CY})`}>
          <path d={`M${cx} ${CY - R + 2} l-7 11 h14 z`} fill="#fff" stroke="#000" strokeWidth={0.6}><title>{`Bank ${bank}° ${i.roll > 0 ? "right" : i.roll < 0 ? "left" : ""}`}</title></path>
        </g>
      </g>
      {/* The aircraft: fixed. */}
      <g fill="#f2c94c" stroke="#111" strokeWidth={1.2}>
        <path d={`M${cx - 70} ${CY - 3} h44 v12 h-6 v-6 h-38 z`} />
        <path d={`M${cx + 70} ${CY - 3} h-44 v12 h6 v-6 h38 z`} />
        <rect x={cx - 4} y={CY - 4} width={8} height={8} />
      </g>
      <T x={x + 8} y={BOTTOM - 12} anchor="start" size={10} fill="#fff">{`P ${i.pitch >= 0 ? "+" : ""}${number(i.pitch, 1)}°`}</T>
      <T x={x + w - 8} y={BOTTOM - 12} anchor="end" size={10} fill="#fff">{`B ${bank}°${bank ? (i.roll > 0 ? " R" : " L") : ""}`}</T>
    </g>
  );
}

function Heading({ i }: { i: Instruments }) {
  const { x, w, cx } = ATT, { y, h, px } = HDG;
  if (i.hdg === null) return <Blank x={x} y={y} w={w} h={h} label="HDG" />;
  const hdg = i.hdg;
  const half = w / 2 / px;
  const marks = ticks(hdg, half, 5);
  return (
    <g>
      <defs><clipPath id="pfd-hdg"><rect x={x} y={y} width={w} height={h} /></clipPath></defs>
      <rect x={x} y={y} width={w} height={h} fill="var(--surface-muted)" />
      <g clipPath="url(#pfd-hdg)">
        {marks.map((d) => {
          const at = cx + (d - hdg) * px, deg = ((d % 360) + 360) % 360;
          return (
            <g key={d}>
              <line x1={at} x2={at} y1={y} y2={y + (deg % 10 ? 5 : 9)} stroke="var(--foreground)" strokeWidth={1} />
              {deg % 30 === 0 ? <T x={at} y={y + 21} size={10}>{String(deg / 10).padStart(2, "0")}</T> : null}
            </g>
          );
        })}
      </g>
      <rect x={cx - 30} y={y - 12} width={60} height={20} fill="var(--surface)" stroke="var(--foreground)" strokeWidth={1.2} rx={2} />
      <T x={cx} y={y - 2} size={13} weight={600} title="Magnetic heading, as recorded">{`${String(Math.round(hdg) % 360).padStart(3, "0")}°M`}</T>
    </g>
  );
}

/** One annunciator box: lit when engaged, dim when off, dashed when not recorded. */
function Annunciator({ x, label, on, what }: { x: number; label: string; on: boolean | null; what: string }) {
  const state = on === null ? "not recorded" : on ? "engaged" : "off";
  return (
    <g>
      <title>{`${what}: ${state}`}</title>
      <rect x={x} y={6} width={58} height={20} rx={3} fill={on ? "color-mix(in oklab, var(--success) 18%, var(--surface))" : "none"} stroke={on ? "var(--success)" : "var(--border-strong)"} strokeDasharray={on === null ? "3 3" : undefined} />
      <T x={x + 29} y={16} size={11} weight={600} fill={on ? "var(--success)" : "var(--text-muted)"}>{on === null ? `${label} –` : on ? label : `${label} off`}</T>
    </g>
  );
}

const gearText = (g: Instruments["gear"]) => (g === null ? NO_DATA : g.state === "transit" ? `transit ${number(g.extended * 100)}%` : g.state === "down" ? "DOWN" : "UP");

interface Props { debrief: Debrief; theme: Theme; bus: TimeBus }

/** The reconstructed PFD at the hovered time, else at the cursor. */
export function ReplayPfd({ debrief, theme, bus }: Props) {
  const [hover, setHover] = useState<number | null>(null);
  useHover(bus, setHover);
  const focus = usePlaced(bus);
  const t = hover ?? focus;
  const i = t === null ? null : instrumentsAt(debrief, t);
  const bug = COLOURS[theme].bug;
  const speeds = i?.speeds;

  return (
    <figure className="panel flex h-full min-w-0 flex-col overflow-hidden" aria-labelledby="pfd-title">
      <figcaption className="flex items-baseline justify-between gap-2 border-b border-line-subtle px-3 py-2">
        <span id="pfd-title" className="eyebrow">Flight display</span>
        <span className="readout text-xs text-fg-3">{t === null ? "–" : `T+${clock(t)}${hover !== null ? " · hover" : ""}`}</span>
      </figcaption>
      <div className="min-h-0 flex-1 p-2">
        {i === null ? (
          <div className="flex h-full items-center justify-center text-center text-xs text-fg-3">Place the cursor on the timeline, a chart or the track to replay the instruments.</div>
        ) : (
          <svg viewBox={`0 0 ${W} ${H}`} className="h-full w-full" role="img" aria-label={describe(i)} preserveAspectRatio="xMidYMid meet">
            <Annunciator x={ATT.cx - 64} label="AP" on={i.ap} what="Autopilot" />
            <Annunciator x={ATT.cx + 6} label="A/T" on={i.at} what="Autothrottle" />
            <Attitude i={i} theme={theme} />
            <SpeedTape i={i} bug={bug} />
            <AltitudeTape i={i} />
            <VerticalSpeed i={i} />
            <Heading i={i} />
            {speeds ? (
              <T x={SPEED.x + SPEED.w / 2} y={BOTTOM + 12} size={11} weight={600} fill={bug} title={`Landing ${speeds.landing + 1}: Vref ${speeds.vref_kt} kt, Vapp ${speeds.vapp_kt} kt (${speeds.source}${speeds.approximate ? ", approximate" : ""}). ${speeds.basis}`}>{`Vapp ${speeds.vapp_kt}`}</T>
            ) : null}
            <g>
              <T x={10} y={H - 14} anchor="start" size={11}>{`FLAPS ${i.flap === null ? NO_DATA : `${number(i.flap)}°`}`}</T>
              <T x={150} y={H - 14} anchor="start" size={11}>{`GEAR ${gearText(i.gear)}`}</T>
              <T x={W - 10} y={H - 14} anchor="end" size={11} title={DERIVED.trk}>{`TRK ${i.trk === null ? NO_DATA : `${String(Math.round(i.trk) % 360).padStart(3, "0")}°T${DAGGER}`}`}</T>
            </g>
          </svg>
        )}
      </div>
      <p className="border-t border-line-subtle px-3 py-1.5 text-[11px] leading-snug text-fg-3">
        Rebuilt from the recording, not a real display. {DAGGER} derived: vertical speed from pressure altitude and load, track from GPS positions. Bands: <span className="text-[var(--warning)]">flap limit</span> and <span className="text-[var(--danger)]">maximum speed</span> from the type profile; <span style={{ color: bug }}>Vapp</span> of the next landing as graded.
      </p>
    </figure>
  );
}

/** The PFD in words, for screen readers. */
function describe(i: Instruments) {
  const v = (x: number | null, unit: string) => (x === null ? NO_DATA : `${number(x)} ${unit}`);
  return [
    `Pitch ${v(i.pitch, "degrees")}, bank ${v(i.roll, "degrees")}`,
    `airspeed ${v(i.ias, "knots")}`,
    `altitude ${v(i.alt, "feet")}`,
    `vertical speed ${v(i.vs, "feet per minute")} (derived)`,
    `heading ${v(i.hdg, "degrees magnetic")}`,
    i.ra !== null ? `radio height ${v(i.ra, "feet")}` : "",
    `flaps ${v(i.flap, "degrees")}, gear ${gearText(i.gear)}`,
  ].filter(Boolean).join("; ");
}
