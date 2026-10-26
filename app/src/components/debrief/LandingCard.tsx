import type { ReactNode } from "react";
import { Crosshair, Info, MapPinOff } from "lucide-react";
import { Badge } from "@ui/components/badge/badge";
import { Button } from "@ui/components/button/button";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { clock, M_TO_FT, number } from "../../lib/format";
import { aircraftClass, bandOf, distanceScale, eventsIn, landingVerdict, landingWindow, PEAK_G, PITCH, PROVENANCE_LABEL, SINK, type Grade, type Scale } from "../../lib/limits";
import { GRADE_COLOR } from "../../lib/palette";
import type { Debrief, FlightEvent, Level, Touchdown } from "../../types";
import { RunwayDiagram } from "./RunwayDiagram";

/** Which touchdown measure each touchdown rule looked at, so the card can flag the value that raised an event. */
const MEASURE_OF: Record<string, string> = { "td.sink": "sink", "td.peak_g": "g", "td.bounce": "bounce", "abs(td.cross)": "cross", "td.long_landing": "along", "td.short_landing": "along", "td.pitch": "pitch" };
const rank: Record<Level, number> = { advisory: 1, low: 2, medium: 3, high: 4 };
const TONE: Record<Grade, "success" | "neutral" | "warning" | "danger"> = { good: "success", ok: "neutral", firm: "warning", review: "danger" };

interface Props {
  index: number;
  touchdown: Touchdown;
  /** Events at the touchdown itself; they flag the measure they judged. */
  events: FlightEvent[];
  runwayWidth?: number;
  /** The whole debrief: picks the aircraft class bands and lets the verdict judge the approach too. */
  debrief?: Debrief;
  onShow: (t: number) => void;
}

export function LandingCard({ index, touchdown: td, events, runwayWidth, debrief, onShow }: Props) {
  // A measure is flagged with the most severe event that judged it.
  const flags = new Map<string, Level>();
  for (const event of events) {
    const key = Object.entries(MEASURE_OF).find(([measure]) => event.measure.includes(measure))?.[1];
    if (key && (!flags.has(key) || rank[event.level] > rank[flags.get(key)!])) flags.set(key, event.level);
  }
  const cls = debrief ? aircraftClass(debrief.flight) : null;
  const judged = debrief ? eventsIn(debrief.events, landingWindow(td.t_s, debrief.phases)) : events;
  const verdict = landingVerdict(td, cls, judged);
  const fix = td.runway;
  const width = runwayWidth ?? (fix && debrief?.airports.find((a) => a.icao === fix.icao)?.runways.find((r) => r.ends.some((e) => e.runway === fix.runway))?.width_m);

  return (
    <article className="panel flex flex-col gap-3 p-4" aria-labelledby={`landing-${index}`}>
      <header className="flex flex-wrap items-center gap-2">
        <h3 id={`landing-${index}`} className="font-display text-base font-semibold">Landing {index + 1}</h3>
        <span className="readout text-xs text-fg-3">T+{clock(td.t_s)}</span>
        {fix ? <span className="readout rounded-sm bg-muted px-1.5 text-xs">{fix.icao} {fix.runway}</span> : null}
        <span className="ml-auto"><Badge size="sm" tone={TONE[verdict.grade]} animated={false}>{verdict.title}</Badge></span>
      </header>

      <div className="rounded-[var(--radius-control)] border px-3 py-2 text-xs" style={{ borderColor: `color-mix(in oklab, ${GRADE_COLOR[verdict.grade]} 45%, var(--border))` }}>
        {verdict.reasons.length ? (
          <ul className="flex flex-col gap-1">
            {verdict.reasons.map((reason) => (
              <li key={reason.text} className="flex items-start gap-2">
                <Swatch grade={reason.grade} className="mt-1" />
                <span className="text-fg-2">{reason.text}</span>
              </li>
            ))}
          </ul>
        ) : <p className="text-fg-2">Nothing flagged from the start of the approach to 10 s into the rollout.</p>}
        <p className="mt-1.5 text-[11px] text-fg-3">
          {cls ? `Bands for ${cls} aircraft (${debrief!.flight.profile}). The bright segment is where the value sits; ⓘ lists each band's source.`
            : "Aircraft class unknown: values are shown without bands; only events and the runway position are judged."}
        </p>
      </div>

      <dl className="flex flex-col gap-3">
        <Metric label="Sink rate" value={`${number(td.sink_fpm)} fpm`} sub={td.sim_sink_fpm !== undefined ? `sim reports ${number(td.sim_sink_fpm)}` : undefined} flag={flags.get("sink")}>
          {cls ? <Gauge scale={SINK[cls]} value={td.sink_fpm} unit="fpm" /> : null}
        </Metric>
        <Metric label="Peak load" value={`${number(td.peak_g, 2)} g`} sub="1 g = level flight" flag={flags.get("g")}>
          {cls ? <Gauge scale={PEAK_G[cls]} value={td.peak_g} unit="g" digits={2} /> : null}
        </Metric>
        <Metric label="Pitch" value={td.pitch_deg !== undefined ? `${number(td.pitch_deg, 1)}°` : "–"} sub="nose up" flag={flags.get("pitch")}>
          {cls === "transport" && td.pitch_deg !== undefined ? <Gauge scale={PITCH.transport} value={td.pitch_deg} unit="°" />
            : cls === "light" ? <Note text={PITCH.light.text} source={PITCH.light.source} /> : null}
        </Metric>
        <Metric label="Roll" value={td.roll_deg !== undefined ? `${number(Math.abs(td.roll_deg), 1)}° ${td.roll_deg > 0 ? "R" : td.roll_deg < 0 ? "L" : ""}` : "–"} sub="bank">
          <Note text="No numeric reference: some bank into the wind is normal in a crosswind." />
        </Metric>
        <Metric label="Speed" value={td.ias_kt !== undefined ? `${number(td.ias_kt)} kt` : "–"} sub="indicated">
          <Note text="Compare with your reference speed (Vref); it is not recorded." />
        </Metric>
        <Metric label="Bounce" value={td.bounce ? "Yes" : "No"} flag={flags.get("bounce")}>
          <p className="flex items-center gap-1.5 text-[11px] text-fg-2"><Swatch grade={td.bounce ? "firm" : "good"} />{td.bounce ? "Left the ground again after the first contact" : "Stayed on the ground after the first contact"}</p>
        </Metric>
      </dl>

      {fix ? (
        <div className="flex flex-col gap-3 rounded-[var(--radius-control)] border border-line-subtle bg-bg px-3 py-2.5">
          <Metric label="Touchdown" value={`${number(fix.along_m)} m`} sub={`${number(fix.along_m * M_TO_FT)} ft past the threshold`} flag={flags.get("along")}>
            <Gauge scale={distanceScale(fix, cls)} value={fix.along_m} unit="m" />
          </Metric>
          <Metric label="Centreline" value={fix.cross_m === 0 ? "0 m" : `${number(Math.abs(fix.cross_m), 1)} m ${fix.cross_m > 0 ? "R" : "L"}`} sub={`${number(Math.abs(fix.cross_m) * M_TO_FT)} ft off`} flag={flags.get("cross")}>
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

function Metric({ label, value, sub, flag, children }: { label: string; value: string; sub?: string; flag?: Level; children?: ReactNode }) {
  return (
    <div className="grid grid-cols-[6.25rem_minmax(0,1fr)] items-start gap-x-3">
      <div className="min-w-0">
        <dt className="eyebrow flex items-center gap-1">
          {label}
          {flag ? <span className="size-1.5 rounded-full" style={{ background: `var(--level-${flag})` }} aria-label={`${flag} event`} /> : null}
        </dt>
        <dd className="readout text-base leading-tight" style={flag ? { color: `var(--level-${flag})` } : undefined}>{value}</dd>
        {sub ? <dd className="text-[11px] leading-tight text-fg-3">{sub}</dd> : null}
      </div>
      <dd className="min-w-0 pt-0.5">{children}</dd>
    </div>
  );
}

const Swatch = ({ grade, className = "" }: { grade: Grade; className?: string }) => <span className={`size-2 shrink-0 rounded-full ${className}`} style={{ background: GRADE_COLOR[grade] }} aria-hidden="true" />;

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

/** Bullet chart: the class bands as coloured segments, official marks as dashed ticks, the value as a needle. */
function Gauge({ scale, value, unit, digits = 0 }: { scale: Scale; value: number; unit: string; digits?: number }) {
  const current = bandOf(scale, value);
  const fmt = (v: number) => number(v, digits);
  const segments = scale.bands.map((band, i) => ({ band, from: Math.max(scale.min, scale.bands[i - 1]?.to ?? scale.min), to: Math.min(band.to, scale.max) })).filter((s) => s.to > s.from);
  const kinds = [...new Set(scale.bands.map((b) => b.kind))];
  const beyond = value > scale.max ? "›" : value < scale.min ? "‹" : null;

  return (
    <div className="flex flex-col gap-0.5">
      <div className="relative mt-2 h-2">
        {segments.map(({ band, from, to }) => (
          <div key={from} className="absolute inset-y-0 first:rounded-l-full last:rounded-r-full" title={`${band.label} (${PROVENANCE_LABEL[band.kind]})`}
            style={{ left: `${pct(scale, from)}%`, width: `${pct(scale, to) - pct(scale, from)}%`, background: GRADE_COLOR[band.grade], opacity: band === current ? 0.95 : 0.28 }} />
        ))}
        {scale.marks?.map((mark) => (
          <span key={mark.label} className="absolute -top-1.5 h-5 w-0 border-l border-dashed border-fg-2" style={{ left: `${pct(scale, mark.at)}%` }} title={mark.label} aria-hidden="true" />
        ))}
        <span className="absolute -top-1.5 h-5 w-[3px] -translate-x-1/2 rounded-full bg-fg ring-2 ring-surface" style={{ left: `${pct(scale, value)}%` }} aria-hidden="true" />
        {beyond ? <span className={`readout absolute -top-4 text-[11px] text-fg ${beyond === "›" ? "right-0" : "left-0"}`} aria-hidden="true">{beyond}</span> : null}
      </div>
      <div className="readout relative h-3 text-[9.5px] text-fg-3" aria-hidden="true">
        {/* Band edges inside the scale; an edge too close to the previous label is left to the ⓘ list */}
        {scale.bands.filter((b, i, all) => b.to > scale.min && b.to < scale.max && !(i && pct(scale, b.to) - pct(scale, all[i - 1]!.to) < 9)).map((b) => (
          <span key={b.to} className="absolute -translate-x-1/2" style={{ left: `${pct(scale, b.to)}%` }}>{fmt(b.to)}</span>
        ))}
      </div>
      <p className="mt-0.5 flex flex-wrap items-center gap-x-1.5 text-[11px]">
        <Swatch grade={current.grade} />
        <span className="text-fg">{current.label}</span>
        <span className="text-fg-3">· {kinds.map((k) => PROVENANCE_LABEL[k]).join(" + ")}</span>
        <Sources>
          {scale.bands.map((b, i) => (
            <span key={b.to} className="block"><b>{b.to === Infinity ? `> ${fmt(scale.bands[i - 1]?.to ?? scale.min)}` : `≤ ${fmt(b.to)}`} {unit}</b> {b.label} · {PROVENANCE_LABEL[b.kind]}</span>
          ))}
          {scale.marks?.map((m) => <span key={m.label} className="block"><b>┊ {m.label}</b> · reference</span>)}
          <span className="mt-1 block font-medium">Sources</span>
          {[...new Set([...scale.bands, ...(scale.marks ?? [])].map((x) => x.source))].map((source) => <span key={source} className="block opacity-80">{source}</span>)}
        </Sources>
      </p>
    </div>
  );
}
