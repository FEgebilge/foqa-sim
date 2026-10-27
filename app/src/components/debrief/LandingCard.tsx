import type { ReactNode } from "react";
import { Crosshair, Info, MapPinOff } from "lucide-react";
import { Button } from "@ui/components/button/button";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { bySeverity, eventsIn, landingWindow } from "../../lib/explain";
import { clock, LEVEL_LABEL, levelRank, M_PER_FT, number } from "../../lib/format";
import { contactLabel, type Landing } from "../../lib/landings";
import { levelVar } from "../../lib/palette";
import { distanceScale, gScale, LIGHT_PITCH, pitchScale, sinkScale, type Scale } from "../../lib/references";
import type { Debrief, FlightEvent, Level, TouchdownWind } from "../../types";
import { LevelBadge } from "../level";
import { RunwayDiagram } from "./RunwayDiagram";

/** Which touchdown value each rule name reads, so the card can flag the value that raised an event. */
const MEASURE_OF: Record<string, string> = { "td.sink": "sink", "td.peak_g": "g", "td.bounce": "bounce", "abs(td.cross)": "cross", "td.long_landing": "along", "td.short_landing": "along", "td.pitch": "pitch" };

interface Props {
  index: number;
  /** The first contact is graded; the bounces are listed. */
  landing: Landing;
  /** Events at the landing's contacts; they flag the measure they judged. */
  events: FlightEvent[];
  runwayWidth?: number;
  /** The whole debrief: the approach's events, the class and the type profile's references. */
  debrief: Debrief;
  onShow: (t: number) => void;
}

export function LandingCard({ index, landing: { touchdown: td, bounces }, events, runwayWidth, debrief, onShow }: Props) {
  // A measure is flagged with the most severe event that judged it.
  const flags = new Map<string, Level>();
  for (const event of events) {
    const key = Object.entries(MEASURE_OF).find(([measure]) => event.measure.includes(measure))?.[1];
    if (key && levelRank(event.level) > levelRank(flags.get(key))) flags.set(key, event.level);
  }
  const { flight } = debrief;
  // The rules grade the landing: its events from the start of the approach to the rollout, the worst first.
  const judged = bySeverity(eventsIn(debrief.events, landingWindow(td.t_s, debrief.phases)));
  const worst = judged[0]?.level;
  const fix = td.runway;
  const width = runwayWidth ?? (fix && debrief.airports.find((a) => a.icao === fix.icao)?.runways.find((r) => r.ends.some((e) => e.runway === fix.runway))?.width_m);
  const sink = sinkScale(flight), g = gScale(flight), pitch = pitchScale(flight);

  return (
    <article className="panel flex flex-col gap-3 p-4" aria-labelledby={`landing-${index}`}>
      <header className="flex flex-wrap items-center gap-2">
        <h3 id={`landing-${index}`} className="font-display text-base font-semibold">Landing {index + 1}</h3>
        <span className="readout text-xs text-fg-3">T+{clock(td.t_s)}</span>
        {bounces.length ? <span className="readout rounded-sm bg-muted px-1.5 text-xs" style={flags.has("bounce") ? { color: levelVar(flags.get("bounce")!) } : undefined}>Bounced ×{bounces.length}</span> : null}
        {fix ? <span className="readout rounded-sm bg-muted px-1.5 text-xs">{fix.icao} {fix.runway}</span> : null}
        <span className="ml-auto"><LevelBadge level={worst} /></span>
      </header>

      <div className="rounded-[var(--radius-control)] border px-3 py-2 text-xs" style={{ borderColor: worst ? `color-mix(in oklab, ${levelVar(worst)} 45%, var(--border))` : "var(--border)" }}>
        {judged.length ? (
          <ul className="flex flex-col gap-1">
            {judged.map((event) => (
              <li key={`${event.rule}-${event.start_s}`} className="flex items-start gap-2">
                <Dot level={event.level} className="mt-1" />
                <span className="text-fg-2"><span className="text-fg">{event.title}</span>{event.detail ? `: ${event.detail}` : ""}</span>
              </li>
            ))}
          </ul>
        ) : <p className="text-fg-2">No rule exceeded from the start of the approach to 10 s into the rollout.</p>}
        <p className="mt-1.5 text-[11px] text-fg-3">The verdict comes from the rules ({flight.profile}). The gauges are context: published references and sim comfort bands (a flight-sim community convention, not a limit); ⓘ lists their sources.</p>
      </div>

      <dl className="flex flex-col gap-3">
        <Metric label="Sink rate" value={`${number(td.sink_fpm)} fpm`} sub={td.sim_sink_fpm !== undefined ? `sim reports ${number(td.sim_sink_fpm)}` : undefined} flag={flags.get("sink")}>
          {sink ? <Gauge scale={sink} value={td.sink_fpm} unit="fpm" flag={flags.get("sink")} /> : null}
        </Metric>
        <Metric label="Peak load" value={`${number(td.peak_g, 2)} g`} sub="1 g = level flight" flag={flags.get("g")}>
          {g ? <Gauge scale={g} value={td.peak_g} unit="g" digits={2} flag={flags.get("g")} /> : null}
        </Metric>
        <Metric label="Pitch" value={td.pitch_deg !== undefined ? `${number(td.pitch_deg, 1)}°` : "–"} sub="nose up" flag={flags.get("pitch")}>
          {pitch && td.pitch_deg !== undefined ? <Gauge scale={pitch} value={td.pitch_deg} unit="°" digits={1} flag={flags.get("pitch")} />
            : flight.class === "light" ? <Note text={LIGHT_PITCH.text} source={LIGHT_PITCH.source} /> : null}
        </Metric>
        <Metric label="Roll" value={td.roll_deg !== undefined ? `${number(Math.abs(td.roll_deg), 1)}° ${td.roll_deg > 0 ? "R" : td.roll_deg < 0 ? "L" : ""}` : "–"} sub="bank">
          <Note text="No numeric reference: some bank into the wind is normal in a crosswind." />
        </Metric>
        <Metric label="Speed" value={td.ias_kt !== undefined ? `${number(td.ias_kt)} kt` : "–"} sub="indicated">
          <Note text="Compare with your reference speed (Vref); it is not recorded." />
        </Metric>
        {td.wind ? (
          <Metric label="Wind" value={windValue(td.wind)} sub="at touchdown">
            <Note text={windComponents(td.wind)} />
          </Metric>
        ) : null}
        <Metric label="Bounce" value={bounces.length ? `Yes, ×${bounces.length}` : "No"} flag={flags.get("bounce")}>
          <p className="text-[11px] text-fg-2">{bounces.length ? "One landing: the aircraft left the ground again after the first contact. The values above are the first contact's." : "Stayed on the ground after the first contact"}</p>
          {bounces.length ? (
            <ul className="mt-1 flex flex-col gap-0.5">
              {bounces.map((b, k) => (
                <li key={b.t_s} className="readout flex flex-wrap items-baseline gap-x-3 text-[11px] text-fg-2">
                  <span className="font-semibold text-fg">{contactLabel(index, k + 1)}</span>
                  <span>T+{clock(b.t_s)}</span><span>{number(b.sink_fpm)} fpm</span><span>{number(b.peak_g, 2)} g</span>
                </li>
              ))}
            </ul>
          ) : null}
        </Metric>
      </dl>

      {fix ? (
        <div className="flex flex-col gap-3 rounded-[var(--radius-control)] border border-line-subtle bg-bg px-3 py-2.5">
          <Metric label="Touchdown" value={`${number(fix.along_m)} m`} sub={`${number(fix.along_m / M_PER_FT)} ft past the threshold · ${fix.zone}`} flag={flags.get("along")}>
            <Gauge scale={distanceScale(fix, flight)} value={fix.along_m} unit="m" flag={flags.get("along")} />
          </Metric>
          <Metric label="Centreline" value={fix.cross_m === 0 ? "0 m" : `${number(Math.abs(fix.cross_m), 1)} m ${fix.cross_m > 0 ? "R" : "L"}`} sub={`${number(Math.abs(fix.cross_m) / M_PER_FT)} ft off`} flag={flags.get("cross")}>
            <Note text={width ? `Target 0 m. The runway edge is ${number(width / 2, 1)} m from the centreline (measured at the aircraft's centre, not its wheels).` : "Target 0 m."} />
          </Metric>
          <RunwayDiagram fix={fix} widthM={width ?? 45} />
        </div>
      ) : (
        <p className="flex items-start gap-2 rounded-[var(--radius-control)] border border-dashed border-line px-3 py-2 text-xs text-fg-2">
          <MapPinOff size={14} className="mt-0.5 shrink-0" aria-hidden="true" />
          No runway data: the airport's runways were not recorded in this flight, so the touchdown point, touchdown zone and centreline checks are not available.
        </p>
      )}

      <div className="flex items-center justify-between gap-2">
        <span className="text-xs text-fg-3">{judged.length ? `${judged.length} event${judged.length > 1 ? "s" : ""} from approach to rollout` : "No events from approach to rollout"}</span>
        <Button size="xs" variant="ghost" onClick={() => onShow(td.t_s)}><Crosshair size={14} aria-hidden="true" />Show in charts</Button>
      </div>
    </article>
  );
}

/** "290° 17 kt · 3 kt tail · 17 kt from the right", on the runway. */
/** "270° 3 kt": where it blew from at contact. */
function windValue({ from_deg, speed_kt }: TouchdownWind) {
  return `${String(Math.round(from_deg) % 360).padStart(3, "0")}° ${number(speed_kt)} kt`;
}

/** The wind on the runway: "0 kt head, 3 kt crosswind from the right". */
function windComponents({ headwind_kt, crosswind_kt }: TouchdownWind) {
  const head = Math.round(headwind_kt), cross = Math.round(crosswind_kt);
  return `On the runway: ${Math.abs(head)} kt ${head < 0 ? "tailwind" : "headwind"}, ${Math.abs(cross)} kt crosswind${cross > 0 ? " from the right" : cross < 0 ? " from the left" : ""}.`;
}

function Metric({ label, value, sub, flag, children }: { label: string; value: string; sub?: string; flag?: Level; children?: ReactNode }) {
  return (
    <div className="grid grid-cols-[6.25rem_minmax(0,1fr)] items-start gap-x-3">
      <div className="min-w-0">
        <dt className="eyebrow flex items-center gap-1">
          {label}
          {flag ? <span className="size-1.5 rounded-full" style={{ background: levelVar(flag) }} aria-label={`${flag} event`} /> : null}
        </dt>
        <dd className="readout text-base leading-tight" style={flag ? { color: levelVar(flag) } : undefined}>{value}</dd>
        {sub ? <dd className="text-[11px] leading-tight text-fg-3">{sub}</dd> : null}
      </div>
      <dd className="min-w-0 pt-0.5">{children}</dd>
    </div>
  );
}

const Dot = ({ level, className = "" }: { level: Level; className?: string }) => <span className={`size-2 shrink-0 rounded-full ${className}`} style={{ background: levelVar(level) }} aria-label={LEVEL_LABEL[level]} />;

function Note({ text, source }: { text: string; source?: string }) {
  return <p className="flex items-start gap-1 text-[11px] text-fg-2">{text}{source ? <Sources>{source}</Sources> : null}</p>;
}

/** Info button whose tooltip names where the numbers come from. */
function Sources({ children }: { children: ReactNode }) {
  return (
    <Tooltip content={<span className="flex max-w-[340px] flex-col gap-1 text-left text-xs">{children}</span>}>
      <button type="button" className="shrink-0 rounded-sm text-fg-3 hover:text-fg" aria-label="Sources"><Info size={12} aria-hidden="true" /></button>
    </Tooltip>
  );
}

const pct = (scale: Scale, v: number) => Math.min(100, Math.max(0, ((v - scale.min) / (scale.max - scale.min)) * 100));

/**
 * The value as a needle in its event's colour. Sim comfort bands shade the track, darker as they get firmer, the one
 * holding the value brightest; published references sit on top, ranges as tinted segments and points as dashed ticks.
 */
function Gauge({ scale, value, unit, digits = 0, flag }: { scale: Scale; value: number; unit: string; digits?: number; flag?: Level }) {
  const fmt = (v: number) => number(v, digits);
  const beyond = value > scale.max ? "›" : value < scale.min ? "‹" : null;
  const bands = scale.comfort?.bands.map((band, i, all) => ({ ...band, from: all[i - 1]?.to ?? scale.min })) ?? [];
  const current = bands.find((b) => value <= b.to);
  // Edges inside the scale; one too close to the previous label is left to the ⓘ list.
  const edges = [...new Set([...bands.map((b) => b.to), ...scale.marks.flatMap((m) => (m.to === undefined ? [m.at] : [m.at, m.to]))])].sort((a, b) => a - b)
    .filter((v) => v > scale.min && v < scale.max).filter((v, i, all) => !i || pct(scale, v) - pct(scale, all[i - 1]!) >= 9);

  return (
    <div className="flex flex-col gap-0.5">
      <div className="relative mt-2 h-2 rounded-full bg-muted">
        {bands.map((b, i) => (
          <span key={b.to} className="absolute inset-y-0 border-r border-surface first:rounded-l-full last:rounded-r-full last:border-r-0" title={`${b.label} (sim comfort band)`}
            style={{ left: `${pct(scale, b.from)}%`, width: `${pct(scale, b.to) - pct(scale, b.from)}%`, background: `color-mix(in oklab, var(--foreground) ${14 + i * 10}%, transparent)`, opacity: b === current ? 1 : 0.6 }} />
        ))}
        {scale.marks.map((mark) => mark.to === undefined
          ? <span key={mark.label} className="absolute -top-1.5 h-5 w-0 border-l border-dashed border-fg-2" style={{ left: `${pct(scale, mark.at)}%` }} title={mark.label} aria-hidden="true" />
          : <span key={mark.label} className="absolute inset-y-0" style={{ left: `${pct(scale, mark.at)}%`, width: `${pct(scale, mark.to) - pct(scale, mark.at)}%`, background: "color-mix(in oklab, var(--success) 40%, transparent)" }} title={mark.label} aria-hidden="true" />)}
        <span className="absolute -top-1.5 h-5 w-[3px] -translate-x-1/2 rounded-full ring-2 ring-surface" style={{ left: `${pct(scale, value)}%`, background: flag ? levelVar(flag) : "var(--foreground)" }} aria-hidden="true" />
        {beyond ? <span className={`readout absolute -top-4 text-[11px] text-fg ${beyond === "›" ? "right-0" : "left-0"}`} aria-hidden="true">{beyond}</span> : null}
      </div>
      <div className="readout relative h-3 text-[9.5px] text-fg-3" aria-hidden="true">
        {edges.map((v) => <span key={v} className="absolute -translate-x-1/2" style={{ left: `${pct(scale, v)}%` }}>{fmt(v)}</span>)}
      </div>
      <p className="mt-0.5 flex flex-wrap items-center gap-x-1.5 text-[11px] text-fg-3">
        {[...scale.marks.map((m) => m.label), ...(current ? [`Sim comfort band: ${current.label.toLowerCase()}`] : [])].join(" · ")}
        <Sources>
          {scale.marks.length ? <span className="block font-medium">Reference</span> : null}
          {scale.marks.map((m) => <span key={m.label} className="block"><b>{m.to === undefined ? `┊ ${fmt(m.at)}` : `${fmt(m.at)}–${fmt(m.to)}`} {unit}</b> {m.label}</span>)}
          {bands.length ? <span className="mt-1 block font-medium">Sim comfort band, context only</span> : null}
          {bands.map((b) => <span key={b.to} className="block"><b>{b.to === Infinity ? `> ${fmt(b.from)}` : `≤ ${fmt(b.to)}`} {unit}</b> {b.label}</span>)}
          <span className="mt-1 block font-medium">Sources</span>
          {[...new Set([...scale.marks.map((m) => m.source), scale.comfort?.source])].filter(Boolean).map((source) => <span key={source} className="block opacity-80">{source}</span>)}
        </Sources>
      </p>
    </div>
  );
}
