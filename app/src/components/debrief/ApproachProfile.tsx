import { useCallback, useMemo, useState, type ReactNode } from "react";
import { Crosshair } from "lucide-react";
import { ZONE_COLOR, ZONES } from "../../lib/assessment";
import { departures, crossing, halves, linesOf, pathHeight, segments, type Segment } from "../../lib/approach";
import { LINE, REF } from "../../lib/palette";
import { number } from "../../lib/format";
import { useTheme } from "../../lib/context";
import { useElementWidth, useHover, usePlaced, type TimeBus } from "../../lib/time";
import type { Debrief, LandingAssessment } from "../../types";

const LEFT = 52, RIGHT = 10;
type Num = number | null;

interface Props {
  debrief: Debrief;
  landing: LandingAssessment;
  index: number;
  bus: TimeBus;
  /** Puts the replay cursor at a time. */
  onFocus: (t: number) => void;
}

/** A few round ticks inside [lo, hi]. */
function ticks(lo: number, hi: number, count = 3) {
  const raw = (hi - lo) / count, mag = 10 ** Math.floor(Math.log10(raw || 1));
  const step = [1, 2, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? raw;
  const out: number[] = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi + 1e-9; v += step) out.push(Math.round(v / step) * step || 0);
  return out;
}

/** A step line: each value holds until the next point. */
const steps = (xs: Num[], ys: Num[], x: (v: number) => number, y: (v: number) => number) => {
  let prev: number | null = null;
  return xs.flatMap((d, i) => {
    const v = ys[i] ?? null;
    if (d === null || v === null) { prev = null; return []; }
    const out = prev === null ? [`M${x(d).toFixed(1)},${y(v).toFixed(1)}`] : [`L${x(d).toFixed(1)},${y(prev).toFixed(1)}`, `L${x(d).toFixed(1)},${y(v).toFixed(1)}`];
    prev = v;
    return out;
  }).join(" ");
};

const path = (xs: Num[], ys: Num[], x: (v: number) => number, y: (v: number) => number) =>
  xs.map((d, i) => (d === null || ys[i] == null ? null : `${x(d).toFixed(1)},${y(ys[i]!).toFixed(1)}`)).reduce<string[]>((acc, pt, i, all) => {
    if (pt !== null) acc.push(`${i === 0 || all[i - 1] === null ? "M" : "L"}${pt}`);
    return acc;
  }, []).join(" ");

/** Where the first or last entry of a criterion's bands put the strip's ends: the data and the anchors up to level 1. */
function domain(values: Num[], landing: LandingAssessment, id: string, side: "above" | "below", symmetric = false): [number, number] {
  const anchors = halves(landing, id).flatMap((c) => c.bands[side].slice(0, 3)).filter((a): a is number => a !== null);
  const all = [...values.filter((v): v is number => v !== null), ...anchors];
  let lo = Math.min(...all), hi = Math.max(...all);
  if (symmetric) { const m = Math.max(Math.abs(lo), Math.abs(hi)); lo = -m; hi = m; }
  const pad = (hi - lo) * 0.08 || 1;
  return [lo - pad, hi + pad];
}

export function ApproachProfile({ debrief, landing, index, bus, onFocus }: Props) {
  const { theme } = useTheme();
  const p = landing.profile!;
  const lines = useMemo(() => linesOf(debrief.series, p, landing.speeds?.vapp_kt ?? null), [debrief.series, p, landing.speeds]);
  const left = useMemo(() => departures(landing, lines), [landing, lines]);
  const [host, width] = useElementWidth<HTMLDivElement>();
  const [hover, setHover] = useState<number | null>(null);
  useHover(bus, setHover);
  const focus = usePlaced(bus);

  const known = (v: Num[]) => v.filter((x): x is number => x !== null);
  const dists = known(p.dist_nm);
  const dMax = Math.max(...dists), dMin = Math.min(0, ...dists);
  const plotW = Math.max(60, width - LEFT - RIGHT);
  const x = (d: number) => LEFT + ((dMax - d) / (dMax - dMin || 1)) * plotW;
  const hasIls = p.gs_dot.some((v) => v !== null) || p.loc_dot.some((v) => v !== null);
  const gates = [
    landing.stable_from_ft != null && landing.stable_from_ft !== landing.split_ft ? { ft: landing.stable_from_ft, label: `stable from ${number(landing.stable_from_ft)} ft` } : null,
    landing.split_ft != null ? { ft: landing.split_ft, label: `${number(landing.split_ft)} ft gate` } : null,
  ].flatMap((g) => { const d = g ? crossing(p, g.ft) : null; return g && d !== null ? [{ ...g, d }] : []; });

  /** The point nearest a time or a pointer position, for the cursor and the readout. */
  const nearest = (test: (i: number) => number) => {
    let best = -1, gap = Infinity;
    p.dist_nm.forEach((d, i) => { if (d === null) return; const g = Math.abs(test(i)); if (g < gap) { gap = g; best = i; } });
    return best;
  };
  const atTime = (t: number | null) => (t === null || t < p.t_s[0]! - 1 || t > p.t_s.at(-1)! + 1 ? -1 : nearest((i) => p.t_s[i]! - t));
  const hoverIndex = atTime(hover), focusIndex = atTime(focus);
  const shown = hoverIndex >= 0 ? hoverIndex : focusIndex;

  const move = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    const px = e.clientX - e.currentTarget.getBoundingClientRect().left;
    const d = dMax - ((px - LEFT) / plotW) * (dMax - dMin);
    const i = nearest((k) => p.dist_nm[k]! - d);
    if (i >= 0) bus.emit(p.t_s[i]!);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bus, p, dMax, dMin, plotW]);

  const ref = REF[theme], c1 = LINE[theme][0], c2 = LINE[theme][1];
  const cursor = (i: number, solid: boolean) => i >= 0 && p.dist_nm[i] != null
    ? <div key={solid ? "f" : "h"} className="pointer-events-none absolute inset-y-0 w-px" style={{ left: x(p.dist_nm[i]!), background: solid ? "var(--foreground)" : "var(--text-muted)", opacity: solid ? 0.9 : 0.7 }} aria-hidden="true" /> : null;

  // The strips share these: the x axis, the gates and the shaded bands.
  const Frame = ({ id, height, lo, hi, fmt = (v: number) => number(v), bands, last, children }: { id: string; height: number; lo: number; hi: number; fmt?: (v: number) => string; bands?: Segment[]; last?: boolean; children: (y: (v: number) => number) => ReactNode }) => {
    const pad = 6, h = height - pad * 2 - (last ? 18 : 0);
    const y = (v: number) => pad + (1 - (v - lo) / (hi - lo)) * h;
    return (
      <svg width={width} height={height} className="block" role="img" aria-label={`${id} strip`}>
        {ticks(lo, hi).map((v) => (
          <g key={v}>
            <line x1={LEFT} x2={LEFT + plotW} y1={y(v)} y2={y(v)} stroke="var(--border-subtle)" />
            <text x={LEFT - 6} y={y(v) + 3} textAnchor="end" className="readout" fontSize="10" fill="var(--text-muted)">{fmt(v)}</text>
          </g>
        ))}
        {ticks(dMin, dMax, Math.max(2, Math.floor(plotW / 90))).map((d) => (
          <g key={d}>
            <line x1={x(d)} x2={x(d)} y1={pad} y2={pad + h} stroke="var(--border-subtle)" />
            {last ? <text x={x(d)} y={pad + h + 13} textAnchor="middle" className="readout" fontSize="10" fill="var(--text-muted)">{number(d, d % 1 ? 1 : 0)}</text> : null}
          </g>
        ))}
        {bands?.flatMap((seg, s) => seg.zones.flatMap((z) => {
          const a = Math.max(lo, z.from), b = Math.min(hi, z.to), x0 = p.dist_nm[seg.from], x1 = p.dist_nm[Math.min(seg.to + 1, p.dist_nm.length - 1)];
          return b > a && x0 != null && x1 != null ? [<rect key={`${s}-${z.zone}-${z.from}`} x={x(x0)} width={Math.max(1, x(x1) - x(x0))} y={y(b)} height={Math.max(0, y(a) - y(b))} fill={ZONE_COLOR[z.zone]} />] : [];
        }))}
        {gates.map((g) => <line key={g.ft} x1={x(g.d)} x2={x(g.d)} y1={pad} y2={pad + h} stroke={ref} strokeDasharray="3 3" />)}
        {children(y)}
      </svg>
    );
  };

  const speedBands = segments(landing, p, "A1"), sinkBands = segments(landing, p, "A2");
  const glide = segments(landing, p, "A5G"), loc = segments(landing, p, "A5L");
  const hasBands = [speedBands, sinkBands, glide, loc].some((s) => s.length);
  const [sLo, sHi] = domain(lines.dev, landing, "A1", "above"), [sLo2] = domain(lines.dev, landing, "A1", "below");
  const [kLo, kHi] = domain([...lines.sink, ...lines.need], landing, "A2", "above");
  const hMax = Math.max(...known(p.height_ft), pathHeight(p, dMax)) * 1.05;
  const flapMax = Math.max(1, ...known(lines.flap));
  const gearColor = (v: Num) => (v === null ? "transparent" : v >= 99 ? ZONE_COLOR[1] : v <= 1 ? ZONE_COLOR[2] : ZONE_COLOR[4]);
  const dev = (i: number) => (lines.dev[i] == null ? "–" : `${lines.dev[i]! > 0 ? "+" : ""}${number(lines.dev[i]!)} kt`);

  const title = (heading: string, text: string) => (
    <div className="px-1 pt-2"><h4 className="eyebrow">{heading}</h4><p className="text-[11px] text-fg-3">{text}</p></div>
  );
  const dash = (pts: string, color: string, w = 1.5, d?: string) => <path d={pts} fill="none" stroke={color} strokeWidth={w} strokeDasharray={d} strokeLinejoin="round" />;

  return (
    <article className="panel flex flex-col gap-2 p-4" aria-labelledby={`profile-${index}`}>
      <header className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <h3 id={`profile-${index}`} className="font-display text-base font-semibold">Landing {index + 1}: approach profile</h3>
        <span className="text-xs text-fg-3">Distance to the threshold, {number(dMax, 1)} nm to 0. The shaded bands are the ones the landing score uses.</span>
      </header>
      <ul className="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] text-fg-3" aria-label="Band colours">
        {hasBands ? ZONES.map((z, k) => <li key={z} className="flex items-center gap-1"><span className="size-2.5 rounded-sm border border-line-subtle" style={{ background: ZONE_COLOR[k] }} />{z}</li>) : null}
        <li className="flex items-center gap-1"><svg width="16" height="6" aria-hidden="true"><line x1="0" x2="16" y1="3" y2="3" stroke={ref} strokeWidth="1.25" strokeDasharray="3 3" /></svg>reference and gates</li>
      </ul>

      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_17rem]">
        <div className="min-w-0">
          <p className="readout min-h-4 px-1 text-[11px] text-fg-2" aria-live="off">
            {shown >= 0 ? `${number(Math.max(0, p.dist_nm[shown] ?? 0), 1)} nm · ${number(p.height_ft[shown] ?? 0)} ft (path ${number(pathHeight(p, p.dist_nm[shown] ?? 0))}) · IAS ${number(lines.ias[shown] ?? 0)} kt (${dev(shown)}) · sink ${number(lines.sink[shown] ?? 0)} fpm (path ${number(lines.need[shown] ?? 0)})`
              : "Point at the profile to move the cursor in the replay and the charts; click to place it."}
          </p>
          <div ref={host} className="relative" onPointerMove={move} onPointerLeave={() => bus.emit(null)} onClick={() => { if (hoverIndex >= 0) onFocus(p.t_s[hoverIndex]!); }} style={{ touchAction: "pan-y" }}>
            {title("Vertical path (ft)", `Height above the runway against the standard ${number(p.path_deg)}° path to ${number(p.tch_ft)} ft over the threshold.${hasIls ? " The dots below are the ILS you received." : " No ILS was received: the 3° line is the standard path; this runway's may differ."}`)}
            <Frame id="Vertical path" height={124} lo={0} hi={hMax} bands={undefined}>
              {(y) => <>
                {dash(`M${x(dMax)},${y(pathHeight(p, dMax))} L${x(0)},${y(p.tch_ft)}`, ref, 1.25, "4 3")}
                {dash(path(p.dist_nm, p.height_ft, x, y), c1)}
                {gates.map((g) => <text key={g.ft} x={x(g.d) + (x(g.d) > LEFT + plotW - 90 ? -4 : 4)} y={14} textAnchor={x(g.d) > LEFT + plotW - 90 ? "end" : "start"} fontSize="10" fill="var(--text-muted)">{g.label}</text>)}
              </>}
            </Frame>
            {title("Speed (kt over Vapp)", `Indicated speed above or below the approach speed${landing.speeds ? ` (Vapp ${number(landing.speeds.vapp_kt)} kt)` : ""}.`)}
            <Frame id="Speed" height={96} lo={Math.min(sLo, sLo2)} hi={sHi} fmt={(v) => `${v > 0 ? "+" : ""}${number(v)}`} bands={speedBands}>
              {(y) => <>{dash(`M${LEFT},${y(0)} L${LEFT + plotW},${y(0)}`, ref, 1, "2 3")}{dash(path(p.dist_nm, lines.dev, x, y), c1)}</>}
            </Frame>
            {title("Sink (fpm)", "Descent rate against the rate the 3° path needs at your ground speed (dashed).")}
            <Frame id="Sink" height={96} lo={Math.min(0, kLo)} hi={kHi} bands={sinkBands}>
              {(y) => <>{dash(path(p.dist_nm, lines.need, x, y), ref, 1.25, "4 3")}{dash(path(p.dist_nm, lines.sink, x, y), c1)}</>}
            </Frame>
            {hasIls ? <>
              {title("ILS (dots)", "Glideslope (fly up +) and localizer (right of course +), 1,000 to 500 ft.")}
              <Frame id="ILS" height={84} lo={-2.2} hi={2.2} fmt={(v) => number(v, v % 1 ? 1 : 0)} bands={glide.length ? glide : loc}>
                {(y) => <>{dash(path(p.dist_nm, p.gs_dot, x, y), c1)}{dash(path(p.dist_nm, p.loc_dot, x, y), c2, 1.5, "5 3")}</>}
              </Frame>
              <p className="flex gap-3 px-1 text-[11px] text-fg-3"><span style={{ color: c1 }}>— glideslope</span><span style={{ color: c2 }}>- - localizer</span></p>
            </> : null}
            {title("Configuration", "Gear and flap angle along the approach; the dashed line is the gate.")}
            <Frame id="Configuration" height={72} lo={0} hi={flapMax * 1.3} last bands={undefined}>
              {(y) => <>
                {p.dist_nm.map((d, i) => {
                  const next = p.dist_nm[i + 1];
                  return d !== null && next != null ? <rect key={i} x={x(d)} width={Math.max(1, x(next) - x(d))} y={y(flapMax * 1.3)} height={7} fill={gearColor(lines.gear[i] ?? null)}><title>{`Gear ${lines.gear[i] != null && lines.gear[i]! >= 99 ? "down" : "not down"}`}</title></rect> : null;
                })}
                {dash(steps(p.dist_nm, lines.flap, x, y), c1)}
                {p.dist_nm.map((d, i) => d !== null && (i === 0 || lines.flap[i] !== lines.flap[i - 1]) && lines.flap[i] != null
                  ? <text key={`f${i}`} x={x(d) + 3} y={y(lines.flap[i]!) - 4} fontSize="10" className="readout" fill="var(--text-muted)">{number(lines.flap[i]!)}°</text> : null)}
              </>}
            </Frame>
            <p className="flex flex-wrap gap-x-3 px-1 text-[11px] text-fg-3"><span>Bar: gear (green down, grey up, orange moving)</span><span>Line: flap angle</span></p>
            {cursor(focusIndex, true)}
            {cursor(hoverIndex, false)}
          </div>
        </div>

        <section className="flex min-w-0 flex-col gap-2" aria-label="Where you left the profile">
          <h4 className="eyebrow">Where you left the profile</h4>
          {left.length ? (
            <ul className="flex flex-col gap-1.5">
              {left.map((d) => (
                <li key={d.id}>
                  <button type="button" onClick={() => onFocus(p.t_s[d.index]!)} className="flex w-full flex-col rounded-[var(--radius-control)] border border-line-subtle px-2.5 py-1.5 text-left text-xs hover:bg-muted">
                    <span className="eyebrow">{d.label}</span>
                    <span className="readout flex items-center gap-1.5 text-fg"><Crosshair size={12} aria-hidden="true" className="shrink-0 text-fg-3" />{d.text}</span>
                  </button>
                </li>
              ))}
            </ul>
          ) : <p className="text-xs text-fg-2">Speed, sink, ILS and configuration stayed within tolerance, or were not graded.</p>}
          <p className="text-[11px] text-fg-3">The first point each graded criterion left its tolerance band, held for its sustain time. Click to put the cursor there.</p>
        </section>
      </div>
    </article>
  );
}
