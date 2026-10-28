import { lazy, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ArrowLeft, Crosshair, Database, FileWarning, Globe, Image, Keyboard, MapPinOff, Pause, Play, PlaneLanding, Route, ScanSearch, ServerCrash, Sheet, UserRoundX, ZoomOut } from "lucide-react";
import { Button } from "@ui/components/button/button";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { api, type ExportFormat } from "../../api";
import { bySeverity, verdict } from "../../lib/explain";
import { clock, number, phaseName } from "../../lib/format";
import { identityNotices } from "../../lib/identity";
import { levelVar } from "../../lib/palette";
import { shortcutAllowed } from "../../lib/keys";
import { openFlight, openFlights } from "../../lib/route";
import type { TermId } from "../../lib/glossary";
import { useNotify, useTheme } from "../../lib/context";
import { landingAt } from "../../lib/assessment";
import { groupLandings } from "../../lib/landings";
import { adjacentEvent, hasPosition, valueAt } from "../../lib/series";
import { TimeBus, useHover, usePlaced, type TimeRange } from "../../lib/time";
import { useDebrief } from "../../lib/use-backend";
import type { Debrief, Series } from "../../types";
import { ErrorBoundary } from "../ErrorBoundary";
import { Term } from "../Term";
import { useAdjacentFlights, useFlightMeta, useTypeMedian } from "./use-history";
import { ExportMenu, type ExportItem } from "../ExportMenu";
import { useOpenFolder } from "../use-open-folder";
import { AssessmentPanel } from "./AssessmentPanel";
import { SkillsPanel } from "./SkillsPanel";
import { DebriefHeader } from "./DebriefHeader";
import { EventList, LevelLegend } from "./EventList";
import { ApproachProfile } from "./ApproachProfile";
import { LandingCard } from "./LandingCard";
import { PhaseTimeline } from "./PhaseTimeline";
import { ReplayPfd } from "./ReplayPfd";
import { ShareCard } from "./ShareCard";

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
  // Scored landings that carry the reference approach, with the index of their landing card.
  const profiled = useMemo(() => landings.flatMap((l, index) => { const a = landingAt(debrief, l.touchdown.t_s); return a?.status === "scored" && a.profile?.dist_nm.some((d) => d !== null) ? [{ landing: a, index }] : []; }), [landings, debrief]);
  const [selected, setSelected] = useState<number | null>(null);
  const [view, setView] = useState<TimeRange | null>(flown);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(4);
  const selectedEvent = selected === null ? null : events[selected] ?? null;
  const clamp = useCallback((t: number) => Math.min(range[1], Math.max(range[0], t)), [range]);

  const viewRef = useRef(view);
  useLayoutEffect(() => { viewRef.current = view; });

  /** Moves the cursor through the bus, so a playback tick re-renders only what shows it; a zoomed view follows it when it leaves, keeping its width. */
  const place = useCallback((t: number) => {
    bus.place(t);
    const v = viewRef.current;
    if (v && (t < v[0] || t > v[1])) setView([t - (v[1] - v[0]) * 0.1, t + (v[1] - v[0]) * 0.9]);
  }, [bus]);
  const from = useCallback(() => bus.placed ?? view?.[0] ?? range[0], [bus, view, range]);

  /** Selects an event: the view zooms around it and the cursor goes to its peak. */
  const openEvent = useCallback((index: number) => {
    const event = events[index];
    if (!event) return;
    setSelected(index);
    setView(around(event.start_s, event.end_s));
    place(event.peak_s ?? event.start_s);
  }, [events, place]);
  const selectEvent = useCallback((index: number) => { if (index === selected) setSelected(null); else openEvent(index); }, [selected, openEvent]);
  const showTime = useCallback((t: number) => { setView([t - 20, t + 12]); place(t); }, [place]);
  const resetZoom = useCallback(() => { setView(flown); setSelected(null); }, [flown]);
  const step = useCallback((seconds: number) => place(clamp(from() + seconds)), [place, clamp, from]);
  const { older, newer } = useAdjacentFlights(flight.id);
  const { meta, save: saveMeta } = useFlightMeta(flight.id);
  const typical = useTypeMedian(flight.aircraft?.icao, flight.id);

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
      if (event.ctrlKey || event.metaKey || event.altKey || !shortcutAllowed(event.target, event.key)) return;
      const key = event.key.toLowerCase(), to = key === "j" ? older : key === "k" ? newer : undefined;
      if (to) openFlight(to.id);
      else if (event.key === "Escape") resetZoom();
      else if (event.key === "ArrowLeft" || event.key === "ArrowRight") { event.preventDefault(); step((event.key === "ArrowLeft" ? -1 : 1) * (event.shiftKey ? 10 : 1)); }
      else if (event.key === " ") { event.preventDefault(); setPlaying((p) => !p); }
      else if (event.key === "[" || event.key === "]") { const index = adjacentEvent(events, from(), event.key === "]" ? 1 : -1); if (index !== null) openEvent(index); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [resetZoom, step, events, from, openEvent, older, newer]);

  const runwayWidth = (icao: string, runway: string) => debrief.airports.find((a) => a.icao === icao)?.runways.find((r) => r.ends.some((e) => e.runway === runway))?.width_m;
  const eventsAt = (t: number) => events.filter((e) => Math.abs(e.start_s - t) < 1);
  const isFlown = !!view && !!flown && view[0] === flown[0] && view[1] === flown[1];
  const [share, setShare] = useState<number | null>(null);
  const exportMenu = <ExportMenu items={useExports(debrief, landings.length, setShare)} />;

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
      <Tooltip content={<dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-xs">{SHORTCUTS.map(([keys, what]) => <div key={keys} className="contents"><dt className="readout font-semibold">{keys}</dt><dd>{what}</dd></div>)}</dl>}>
        <Button size="xs" variant="ghost" aria-label="Keyboard shortcuts"><Keyboard size={14} aria-hidden="true" /><span className="2xl:hidden" aria-hidden="true">Keys</span></Button>
      </Tooltip>
      <span className="hidden text-xs text-fg-3 2xl:inline">← → step · Shift ×10 · [ ] event · Space play · Esc reset · J K flight</span>
    </div>
  );
  const notices = (
    <>
      {flight.damaged_at !== undefined ? (
        <Banner tone="danger" icon={<FileWarning size={16} />} title="This recording was cut off">
          The journal ends abruptly at byte {number(flight.damaged_at)} (the sim or the app probably closed mid-flight). The debrief covers everything recorded before that point; later phases, landings and events are missing.
        </Banner>
      ) : null}
      {identityNotices(flight).map((n) => (
        <Banner key={n.kind} tone="info" icon={NOTICE_ICON[n.kind]} title={n.title}>{n.body}</Banner>
      ))}
    </>
  );

  return (
    <>
      <DebriefHeader flight={flight} refreshing={refreshing} onRefresh={onRefresh} onOpenFolder={onOpenFolder} exportMenu={exportMenu} notices={notices}
        older={older} newer={newer} meta={meta} onMeta={saveMeta} />
      <ShareCard debrief={debrief} landing={share} onLanding={setShare} onClose={() => setShare(null)} />
      <Verdict debrief={debrief} onSelectEvent={(index) => { openEvent(index); document.getElementById("replay")?.scrollIntoView({ block: "start", behavior: "smooth" }); }} />
      <AssessmentPanel debrief={debrief} onSelectEvent={selectEvent} />
      {debrief.skills ? <SkillsPanel skills={debrief.skills} /> : null}

      {/* Replay workspace: on wide screens it fills the window. The timeline is the one slider; under it the events list
          beside the map and the rebuilt flight display (side by side from xl, neither a tall column), the charts below. */}
      <section id="replay" aria-label="Replay" className="flex scroll-mt-4 flex-col gap-3 xl:h-[calc(100dvh-4.5rem)] xl:min-h-[680px]">
        <PhaseTimeline phases={phases} phaseValues={flight.phase_values ?? {}} events={events} touchdowns={touchdowns} range={range} view={view} selected={selected} bus={bus} onFocus={place} onZoom={setView} onSelectEvent={selectEvent} controls={controls} />

        <div className="grid min-h-0 flex-1 gap-4 min-[960px]:grid-cols-[minmax(260px,300px)_minmax(0,1fr)] xl:grid-cols-[minmax(290px,340px)_minmax(0,1fr)] 2xl:grid-cols-[minmax(320px,400px)_minmax(0,1fr)]">
          {/* Beside the replay from the 960 px minimum: it scrolls on its own, so a long list never pushes the map away. */}
          <aside aria-labelledby="events-title" className="flex min-h-0 flex-col gap-2 min-[960px]:sticky min-[960px]:top-4 min-[960px]:max-h-[calc(100dvh-2rem)] min-[960px]:self-start min-[960px]:overflow-y-auto min-[960px]:pr-1 xl:static xl:max-h-none xl:self-stretch">
            <h2 id="events-title" className="eyebrow">Events <span className="readout">{events.length}</span></h2>
            <LevelLegend />
            <EventList events={events} phases={phases} selected={selected} onSelect={selectEvent} />
          </aside>

          <div className="flex min-h-0 min-w-0 flex-col gap-3">
            {/* From xl one row: the display's width follows the row height at its own aspect, the map takes the rest; the row
                height also follows the window width, so the map stays landscape. Narrower, they stack, the map landscape. */}
            <div className="grid gap-4 [--deck:clamp(300px,min(40dvh,28vw),480px)] xl:h-[var(--deck)] xl:grid-cols-[minmax(0,1fr)_calc(var(--deck)*1.12)]">
              <section aria-labelledby="map-title" className="panel flex min-h-0 flex-col overflow-hidden">
                <h2 id="map-title" className="eyebrow border-b border-line-subtle px-3 py-2">Track and runways</h2>
                {/* Landscape when stacked; beside the display it takes the row's height and the width that is left. */}
                <div className="aspect-[16/10] max-h-[55dvh] w-full xl:aspect-auto xl:max-h-none xl:min-h-0 xl:flex-1">
                  {hasPosition(series) ? (
                    <ErrorBoundary what="The map" size="sm">
                      <Suspense fallback={<div className="h-full animate-pulse bg-muted" />}>
                        <FlightMap debrief={debrief} theme={theme} selected={selectedEvent} bus={bus} onFocus={place} />
                      </Suspense>
                    </ErrorBoundary>
                  ) : <EmptyState size="sm" icon={<MapPinOff size={18} />} title="No position recorded" description="This flight has no latitude/longitude samples to draw." />}
                </div>
              </section>
              <div className="mx-auto aspect-[1.12] w-full max-w-[480px] xl:aspect-auto xl:h-full xl:max-w-none">
                <ReplayPfd debrief={debrief} theme={theme} bus={bus} />
              </div>
            </div>
            <CursorReadout series={series} phases={phases} bus={bus} />

            <div className="flex min-h-0 flex-col gap-4 xl:flex-1 xl:overflow-y-auto xl:pr-1">
              <section className="panel px-3 pt-2 pb-1" aria-labelledby="charts-title">
                <div className="flex flex-wrap items-center justify-between gap-2 border-b border-line-subtle pb-2">
                  <h2 id="charts-title" className="eyebrow">Parameters</h2>
                  <span className="text-xs text-fg-3">Drag across a chart to zoom · click to place the cursor</span>
                </div>
                <ErrorBoundary what="The charts" size="sm">
                  <Suspense fallback={<ChartSkeleton />}>
                    <Charts debrief={debrief} theme={theme} view={view} selected={selectedEvent} bus={bus} onView={setView} onFocus={place} />
                  </Suspense>
                </ErrorBoundary>
              </section>
              <section aria-labelledby="landings-title">
                <h2 id="landings-title" className="eyebrow mb-2">Landings <span className="readout">{landings.length}</span></h2>
                {landings.length ? (
                  <div className="grid gap-4 [grid-template-columns:repeat(auto-fill,minmax(min(100%,400px),1fr))]">
                    {landings.map((landing, index) => (
                      <LandingCard key={landing.touchdown.t_s} index={index} landing={landing} typical={typical} events={[landing.touchdown, ...landing.bounces].flatMap((td) => eventsAt(td.t_s))} debrief={debrief} runwayWidth={landing.touchdown.runway ? runwayWidth(landing.touchdown.runway.icao, landing.touchdown.runway.runway) : undefined} onShow={showTime} onShare={setShare} />
                    ))}
                  </div>
                ) : (
                  <div className="panel"><EmptyState size="sm" icon={<PlaneLanding size={18} />} title="No touchdown recorded" description={flight.damaged_at !== undefined ? "The recording ends before the aircraft landed." : "The aircraft did not land during this recording."} /></div>
                )}
              </section>
              {profiled.length ? (
                <section aria-labelledby="profile-title" className="flex flex-col gap-4">
                  <h2 id="profile-title" className="eyebrow">Reference approach</h2>
                  {profiled.map(({ landing, index }) => <ApproachProfile key={landing.t_s} debrief={debrief} landing={landing} index={index} bus={bus} onFocus={place} />)}
                </section>
              ) : null}
            </div>
          </div>
        </div>
      </section>
    </>
  );
}

/** The Export/Share menu's items: the flight in each format, and the landing card of the graded landing. */
function useExports(debrief: Debrief, landings: number, share: (landing: number) => void): ExportItem[] {
  const notify = useNotify();
  const id = debrief.flight.id;
  const save = (format: ExportFormat) => (identity: boolean) => api.exportFlight(id, format, identity).then(
    (file) => { if (file) notify({ title: "Flight exported", body: file }); },
    (error: unknown) => notify({ title: "Could not export the flight", body: String(error), tone: "error" }));
  const items: ExportItem[] = [
    { label: "CSV", hint: "Every recorded parameter at 8 Hz, for spreadsheets", icon: <Sheet size={16} />, run: save("csv") },
    { label: "KML", hint: "3D path by phase with touchdowns and events, for Google Earth", icon: <Globe size={16} />, run: save("kml") },
    { label: "GPX", hint: "Track with time and altitude, for Little Navmap and map viewers", icon: <Route size={16} />, run: save("gpx") },
    { label: "Parquet", hint: "Every sample at its own rate, for pandas or DuckDB", icon: <Database size={16} />, run: save("parquet") },
  ];
  if (landings) {
    const graded = debrief.flight.assessment_detail.graded_by ?? landings - 1;
    items.push({ label: "Landing card", hint: "An image of a landing's score to copy or save", icon: <Image size={16} />, anonymous: true, run: () => share(Math.min(graded, landings - 1)) });
  }
  return items;
}

/** Events the verdict points at: the most severe ones, as many as fit one line. */
const LOOK_AT = 3;

/** One plain-language sentence on top: the worst level, the key issue, where the aircraft landed; then the events to look
 * at first, each opening in the replay. */
function Verdict({ debrief, onSelectEvent }: { debrief: Debrief; onSelectEvent: (index: number) => void }) {
  const level = debrief.flight.worst_level;
  const color = level ? levelVar(level) : "var(--success)";
  const worst = bySeverity(debrief.events.filter((e) => e.level !== "advisory")).slice(0, LOOK_AT);
  return (
    <div className="flex flex-wrap items-start gap-x-3 gap-y-2 rounded-[var(--radius-panel)] border px-4 py-2.5 text-sm" style={{ borderColor: `color-mix(in oklab, ${color} 40%, var(--border))`, background: `color-mix(in oklab, ${color} 8%, var(--surface))` }}>
      <span className="mt-1.5 size-2.5 shrink-0 rounded-[2px]" style={{ background: color }} aria-hidden="true" />
      <p className="min-w-0 flex-1 basis-[24rem]">{verdict(debrief)}</p>
      {worst.length ? (
        <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Events to look at first">
          <span className="eyebrow">Look at</span>
          {worst.map((e) => {
            const index = debrief.events.indexOf(e);
            return (
              <Button key={index} size="xs" variant="secondary" onClick={() => onSelectEvent(index)}>
                <span className="size-2 shrink-0 rounded-[2px]" style={{ background: levelVar(e.level) }} aria-hidden="true" />{e.title}
                <span className="readout text-fg-3">T+{clock(e.start_s)}</span><Crosshair size={12} aria-hidden="true" />
              </Button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

const SHORTCUTS: [string, string][] = [
  ["← →", "Step the cursor 1 s (Shift: 10 s)"],
  ["Space", "Play or pause"],
  ["[ ]", "Previous or next event"],
  ["Esc", "Back to the airborne view"],
  ["J K", "Older or newer flight"],
];

/** Values at the hovered time, else at the focus: what the aircraft was doing where the map dot is. */
function CursorReadout({ series, phases, bus }: { series: Series; phases: Debrief["phases"]; bus: TimeBus }) {
  const [hover, setHover] = useState<number | null>(null);
  useHover(bus, setHover);
  const focus = usePlaced(bus);
  const t = hover ?? focus;
  const value = (values: Series["aal_ft"], unit: string, digits = 0) => { const v = t === null ? null : valueAt(series.t_s, values, t); return v === null ? "–" : `${number(v, digits)} ${unit}`; };
  const phase = t === null ? undefined : (phases.find((p) => t >= p.start_s && t < p.end_s) ?? phases.at(-1))?.phase;
  const cells: [string, string, TermId?][] = [
    ["Time", t === null ? "–" : `T+${clock(t)}`],
    ["Phase", phase ? phaseName(phase) : "–"],
    ["Alt AAL", value(series.aal_ft, "ft"), "aal"],
    ["IAS", value(series.ias_kt, "kt"), "ias"],
    ["VS", value(series.vs_fpm, "fpm"), "vs"],
    ["Load", value(series.nz_g, "g", 2), "load"],
  ];
  return (
    <dl className="panel grid grid-cols-3 gap-x-3 gap-y-1.5 px-3 py-2 sm:grid-cols-6" aria-label={hover !== null ? "Values at the hovered time" : "Values at the cursor"}>
      {cells.map(([label, text, term]) => (
        <div key={label} className="min-w-0">
          <dt className="eyebrow">{term ? <Term id={term}>{label}</Term> : label}</dt>
          <dd className="readout truncate text-sm">{text}</dd>
        </div>
      ))}
      {/* Always one line, so the rows below never move as the pointer comes and goes. */}
      <p className="col-span-full truncate text-xs text-fg-3">{hover !== null ? "Values at the pointer · click to place the cursor here." : t !== null ? "Values at the cursor · click the timeline, a chart or the track to move it." : "Click the timeline, a chart or the track to place the cursor."}</p>
    </dl>
  );
}

const NOTICE_ICON = {
  unidentified: <UserRoundX size={16} />,
  "class-default": <Database size={16} />,
  "not-assessed": <PlaneLanding size={16} />,
};

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
