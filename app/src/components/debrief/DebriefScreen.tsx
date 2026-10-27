import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ArrowLeft, FileWarning, Pause, Play, PlaneLanding, ScanSearch, ServerCrash, UserRoundX, ZoomOut } from "lucide-react";
import { Button } from "@ui/components/button/button";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import { verdict } from "../../lib/explain";
import { clock, number, phaseName } from "../../lib/format";
import { levelVar } from "../../lib/palette";
import { openFlights } from "../../lib/route";
import { useTheme } from "../../lib/context";
import { groupLandings } from "../../lib/landings";
import { indexAt, TimeBus, useHover, type TimeRange } from "../../lib/time";
import { useDebrief } from "../../lib/use-backend";
import type { Debrief, Series } from "../../types";
import { useOpenFolder } from "../use-open-folder";
import { AssessmentPanel } from "./AssessmentPanel";
import { DebriefHeader } from "./DebriefHeader";
import { EventList, LevelLegend } from "./EventList";
import { LandingCard } from "./LandingCard";
import { PhaseTimeline } from "./PhaseTimeline";

// uPlot and MapLibre are the heavy parts; they load after the debrief itself.
const Charts = lazy(() => import("./Charts"));
const FlightMap = lazy(() => import("./FlightMap"));

export function DebriefScreen({ id }: { id: string }) {
  const { state, refresh, refreshing } = useDebrief(id);
  const openFolder = useOpenFolder();

  if (state.status === "loading") return <Shell><Loading /></Shell>;
  if (state.status === "error") {
    return (
      <Shell>
        <div className="panel">
          <EmptyState icon={<ServerCrash size={20} />} title="Could not open this flight" description={state.message}
            action={<><Button variant="secondary" size="sm" onClick={openFlights}><ArrowLeft size={15} aria-hidden="true" />All flights</Button><Button size="sm" onClick={refresh}>Try again</Button></>} />
        </div>
      </Shell>
    );
  }
  return <Shell><DebriefView debrief={state.data} refreshing={refreshing} onRefresh={refresh} onOpenFolder={openFolder} /></Shell>;
}

const Shell = ({ children }: { children: ReactNode }) => <main className="flex w-full flex-col gap-5 px-6 py-5 2xl:px-8">{children}</main>;

/** Padding around a span when zooming to it, so the lead-in and recovery stay visible. */
const around = (start: number, end: number): TimeRange => { const pad = Math.max(15, (end - start) * 0.6); return [start - pad, end + pad]; };

/** Ground phases; the default view leaves them out (most of a recording can be taxi). */
const GROUND = new Set(["Standing", "Taxi"]);
const SPEEDS = [1, 2, 4, 8, 16];
const TICK_MS = 100;

/** From the takeoff roll to the end of the last landing roll, padded; `null` for a flight that never flew. */
function airborne(phases: Debrief["phases"]): TimeRange | null {
  const flying = phases.filter((p) => !GROUND.has(p.phase));
  if (!flying.length) return null;
  return [flying[0]!.start_s - 10, flying.at(-1)!.end_s + 10];
}

function DebriefView({ debrief, refreshing, onRefresh, onOpenFolder }: { debrief: Debrief; refreshing: boolean; onRefresh: () => void; onOpenFolder: () => void }) {
  const { theme } = useTheme();
  const bus = useMemo(() => new TimeBus(), []);
  const { flight, phases, touchdowns, events, series } = debrief;
  const range = useMemo<TimeRange>(() => [Math.min(series.t_s[0] ?? 0, phases[0]?.start_s ?? Infinity), Math.max(series.t_s.at(-1) ?? 0, phases.at(-1)?.end_s ?? 0)], [series, phases]);
  const flown = useMemo(() => airborne(phases), [phases]);
  const landings = useMemo(() => groupLandings(touchdowns), [touchdowns]);
  const [selected, setSelected] = useState<number | null>(null);
  const [focus, setFocus] = useState<number | null>(null);
  const [view, setView] = useState<TimeRange | null>(flown);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(4);
  const selectedEvent = selected === null ? null : events[selected] ?? null;
  const clamp = useCallback((t: number) => Math.min(range[1], Math.max(range[0], t)), [range]);
  const focusRef = useRef(focus);

  /** Moves the cursor; a zoomed view follows it when it leaves, keeping its width. */
  const place = useCallback((t: number) => {
    focusRef.current = t;
    setFocus(t);
    setView((v) => (!v || (t >= v[0] && t <= v[1]) ? v : [t - (v[1] - v[0]) * 0.1, t + (v[1] - v[0]) * 0.9]));
  }, []);
  const from = useCallback(() => focusRef.current ?? view?.[0] ?? range[0], [view, range]);

  const selectEvent = useCallback((index: number) => {
    if (index === selected) { setSelected(null); return; }
    const event = events[index];
    if (!event) return;
    setSelected(index);
    setView(around(event.start_s, event.end_s));
    place(event.peak_s ?? event.start_s);
  }, [events, selected, place]);
  const showTime = useCallback((t: number) => { setView([t - 20, t + 12]); place(t); }, [place]);
  const resetZoom = useCallback(() => { setView(flown); setSelected(null); }, [flown]);
  const step = useCallback((seconds: number) => place(clamp(from() + seconds)), [place, clamp, from]);

  // Playback advances the cursor; it stops at the end of the recording.
  useEffect(() => {
    if (!playing) return;
    const timer = window.setInterval(() => {
      const next = clamp(from() + (speed * TICK_MS) / 1000);
      place(next);
      if (next >= range[1]) setPlaying(false);
    }, TICK_MS);
    return () => window.clearInterval(timer);
  }, [playing, speed, range, clamp, from, place]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.target instanceof Element && event.target.closest("input, textarea, select, [contenteditable='true']")) return;
      if (event.key === "Escape") resetZoom();
      else if (event.key === "ArrowLeft" || event.key === "ArrowRight") { event.preventDefault(); step((event.key === "ArrowLeft" ? -1 : 1) * (event.shiftKey ? 10 : 1)); }
      else if (event.key === " ") { event.preventDefault(); setPlaying((p) => !p); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [resetZoom, step]);

  const runwayWidth = (icao: string, runway: string) => debrief.airports.find((a) => a.icao === icao)?.runways.find((r) => r.ends.some((e) => e.runway === runway))?.width_m;
  const eventsAt = (t: number) => events.filter((e) => Math.abs(e.start_s - t) < 1);
  const isFlown = !!view && !!flown && view[0] === flown[0] && view[1] === flown[1];

  const controls = (
    <div className="flex flex-wrap items-center gap-2">
      <div className="inline-flex gap-0.5 rounded-[var(--radius-control)] border border-line p-0.5" role="group" aria-label="Time span">
        {flown ? <Button size="xs" variant={isFlown ? "secondary" : "ghost"} aria-pressed={isFlown} onClick={resetZoom}>Airborne</Button> : null}
        <Button size="xs" variant={view === null ? "secondary" : "ghost"} aria-pressed={view === null} onClick={() => setView(null)}><ZoomOut size={14} aria-hidden="true" />Whole flight</Button>
      </div>
      <Button size="xs" variant="secondary" onClick={() => setPlaying((p) => !p)}>
        {playing ? <Pause size={14} aria-hidden="true" /> : <Play size={14} aria-hidden="true" />}{playing ? "Pause" : "Play"}
      </Button>
      <label className="flex items-center text-xs">
        <span className="sr-only">Playback speed</span>
        <select value={speed} onChange={(e) => setSpeed(Number(e.target.value))} className="readout rounded-[var(--radius-control)] border border-line bg-surface px-1 py-0.5 text-xs text-fg">
          {SPEEDS.map((s) => <option key={s} value={s}>{s}×</option>)}
        </select>
      </label>
      <span className="hidden text-xs text-fg-3 2xl:inline">← → step · Shift ×10 · Space play · Esc reset</span>
    </div>
  );

  return (
    <>
      <DebriefHeader flight={flight} refreshing={refreshing} onRefresh={onRefresh} onOpenFolder={onOpenFolder} />
      <Verdict debrief={debrief} />
      <AssessmentPanel debrief={debrief} onSelectEvent={selectEvent} />


      {flight.damaged_at !== undefined ? (
        <Banner tone="danger" icon={<FileWarning size={16} />} title="This recording was cut off">
          The journal ends abruptly at byte {number(flight.damaged_at)} (the sim or the app probably closed mid-flight). The debrief covers everything recorded before that point; later phases, landings and events are missing.
        </Banner>
      ) : null}
      {!flight.aircraft ? (
        <Banner tone="info" icon={<UserRoundX size={16} />} title="Aircraft not identified">
          The sim did not report which aircraft was flown, so type limits (flap speeds, tail clearance, hard-landing inspection) could not be checked. Rules ran with: {flight.profile}.
        </Banner>
      ) : null}

      {/* Replay workspace: on wide screens it fills the window, the timeline on top, findings | charts | map side by side. */}
      <section aria-label="Replay" className="flex flex-col gap-3 xl:h-[calc(100dvh-4.5rem)] xl:min-h-[620px]">
        <PhaseTimeline phases={phases} phaseValues={flight.phase_values ?? {}} events={events} touchdowns={touchdowns} range={range} focus={focus} view={view} selected={selected} bus={bus} onFocus={place} onZoom={setView} onSelectEvent={selectEvent} controls={controls} />

        <div className="grid min-h-0 flex-1 gap-4 lg:grid-cols-[minmax(290px,330px)_minmax(0,1fr)] lg:[grid-template-areas:'rail_map'_'rail_charts'] xl:grid-cols-[minmax(290px,340px)_minmax(0,1fr)_clamp(340px,30vw,800px)] xl:[grid-template-areas:'rail_charts_map']">
          <aside aria-labelledby="events-title" className="flex min-h-0 flex-col gap-2 lg:[grid-area:rail] xl:overflow-y-auto xl:pr-1">
            <h2 id="events-title" className="eyebrow">Findings <span className="readout">{events.length}</span></h2>
            <LevelLegend />
            <EventList events={events} phases={phases} selected={selected} onSelect={selectEvent} />
          </aside>

          <div className="flex min-h-0 min-w-0 flex-col gap-4 max-lg:order-2 lg:[grid-area:charts] xl:overflow-y-auto xl:pr-1">
            <section className="panel px-3 pt-2 pb-1" aria-labelledby="charts-title">
              <div className="flex flex-wrap items-center justify-between gap-2 border-b border-line-subtle pb-2">
                <h2 id="charts-title" className="eyebrow">Parameters</h2>
                <span className="text-xs text-fg-3">Drag across a chart to zoom · click to place the cursor</span>
              </div>
              <Suspense fallback={<ChartSkeleton />}>
                <Charts debrief={debrief} theme={theme} view={view} focus={focus} selected={selectedEvent} bus={bus} onView={setView} onFocus={place} />
              </Suspense>
            </section>
            <section aria-labelledby="landings-title">
              <h2 id="landings-title" className="eyebrow mb-2">Landings <span className="readout">{landings.length}</span></h2>
              {landings.length ? (
                <div className="grid gap-4 [grid-template-columns:repeat(auto-fill,minmax(min(100%,400px),1fr))]">
                  {landings.map((landing, index) => (
                    <LandingCard key={landing.touchdown.t_s} index={index} landing={landing} events={[landing.touchdown, ...landing.bounces].flatMap((td) => eventsAt(td.t_s))} debrief={debrief} runwayWidth={landing.touchdown.runway ? runwayWidth(landing.touchdown.runway.icao, landing.touchdown.runway.runway) : undefined} onShow={showTime} />
                  ))}
                </div>
              ) : (
                <div className="panel"><EmptyState size="sm" icon={<PlaneLanding size={18} />} title="No touchdown recorded" description={flight.damaged_at !== undefined ? "The recording ends before the aircraft landed." : "The aircraft did not land during this recording."} /></div>
              )}
            </section>

          </div>

          <section aria-labelledby="map-title" className="flex min-h-0 flex-col gap-2 max-lg:order-1 lg:[grid-area:map]">
            <div className="panel flex flex-col overflow-hidden">
              <h2 id="map-title" className="eyebrow border-b border-line-subtle px-3 py-2">Track and runways</h2>
              {/* Never taller than wide: 4:3, and on a wide single column capped so it stays a landscape strip. */}
              <div className="aspect-[4/3] max-h-[60dvh] w-full">
                <Suspense fallback={<div className="h-full animate-pulse bg-muted" />}>
                  <FlightMap debrief={debrief} theme={theme} focus={focus} selected={selectedEvent} bus={bus} onFocus={place} />
                </Suspense>
              </div>
            </div>
            <CursorReadout series={series} phases={phases} focus={focus} bus={bus} />
          </section>
        </div>
      </section>
    </>
  );
}

/** One plain-language sentence on top: the worst level, the key issue, where the aircraft landed. */
function Verdict({ debrief }: { debrief: Debrief }) {
  const level = debrief.flight.worst_level;
  const color = level ? levelVar(level) : "var(--success)";
  return (
    <p className="flex items-start gap-3 rounded-[var(--radius-panel)] border px-4 py-2.5 text-sm" style={{ borderColor: `color-mix(in oklab, ${color} 40%, var(--border))`, background: `color-mix(in oklab, ${color} 8%, var(--surface))` }}>
      <span className="mt-1.5 size-2.5 shrink-0 rounded-[2px]" style={{ background: color }} aria-hidden="true" />
      <span>{verdict(debrief)}</span>
    </p>
  );
}

/** Values at the hovered time, else at the focus: what the aircraft was doing where the map dot is. */
function CursorReadout({ series, phases, focus, bus }: { series: Series; phases: Debrief["phases"]; focus: number | null; bus: TimeBus }) {
  const [hover, setHover] = useState<number | null>(null);
  useHover(bus, setHover);
  const t = hover ?? focus;
  const i = t === null ? -1 : indexAt(series.t_s, t);
  const value = (values: Series["aal_ft"], unit: string, digits = 0) => { const v = i < 0 ? null : values[i]; return v == null ? "–" : `${number(v, digits)} ${unit}`; };
  const phase = t === null ? undefined : (phases.find((p) => t >= p.start_s && t < p.end_s) ?? phases.at(-1))?.phase;
  const cells: [string, string][] = [
    ["Time", t === null ? "–" : `T+${clock(t)}`],
    ["Phase", phase ? phaseName(phase) : "–"],
    ["Alt AAL", value(series.aal_ft, "ft")],
    ["IAS", value(series.ias_kt, "kt")],
    ["VS", value(series.vs_fpm, "fpm")],
    ["Load", value(series.nz_g, "g", 2)],
  ];
  return (
    <dl className="panel grid grid-cols-3 gap-x-3 gap-y-1.5 px-3 py-2" aria-label={hover !== null ? "Values at the hovered time" : "Values at the cursor"}>
      {cells.map(([label, text]) => (
        <div key={label} className="min-w-0">
          <dt className="eyebrow">{label}</dt>
          <dd className="readout truncate text-sm">{text}</dd>
        </div>
      ))}
      {/* Kept in the layout while hidden so hovering never changes the panel height. */}
      <p className={`col-span-3 text-xs text-fg-3 ${t === null ? "" : "invisible"}`}>Click the timeline, a chart or the track to place the cursor.</p>
    </dl>
  );
}

function Banner({ tone, icon, title, children }: { tone: "danger" | "info"; icon: ReactNode; title: string; children: ReactNode }) {
  const color = tone === "danger" ? "var(--danger)" : "var(--accent)";
  return (
    <div role={tone === "danger" ? "alert" : "note"} className="flex gap-3 rounded-[var(--radius-panel)] border px-4 py-3" style={{ borderColor: `color-mix(in oklab, ${color} 40%, var(--border))`, background: `color-mix(in oklab, ${color} 8%, var(--surface))` }}>
      <span className="mt-0.5 shrink-0" style={{ color }}>{icon}</span>
      <div>
        <p className="text-sm font-medium">{title}</p>
        <p className="mt-0.5 text-sm text-fg-2">{children}</p>
      </div>
    </div>
  );
}

const Loading = () => (
  <div className="flex flex-col gap-4" aria-label="Loading the debrief">
    <div className="h-24 animate-pulse rounded-[var(--radius-panel)] bg-muted" />
    <div className="h-32 animate-pulse rounded-[var(--radius-panel)] bg-muted" />
    <div className="flex items-center gap-2 text-sm text-fg-3"><ScanSearch size={16} aria-hidden="true" />Analysing the flight…</div>
  </div>
);

const ChartSkeleton = () => <div className="flex flex-col gap-2 py-2">{Array.from({ length: 4 }, (_, i) => <div key={i} className="h-28 animate-pulse rounded-md bg-muted" />)}</div>;
