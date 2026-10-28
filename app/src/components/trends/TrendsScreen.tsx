import { useState, type ReactNode } from "react";
import { FolderOpen, RefreshCw, SearchX, ServerCrash, TrendingUp } from "lucide-react";
import { Button } from "@ui/components/button/button";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import SegmentedControl from "@ui/components/segmented-control/segmented-control";
import { useTheme } from "../../lib/context";
import { number } from "../../lib/format";
import type { Theme } from "../../lib/palette";
import { delta } from "../../lib/trends";
import { useTrends } from "../../lib/use-backend";
import type { TrendFilter, Trends } from "../../types";
import { useOpenFolder } from "../use-open-folder";
import { DomainBars } from "./DomainBars";
import { Footprint } from "./Footprint";
import { Panel } from "./Panel";
import { ScoreChart } from "./ScoreChart";
import { SkillsRow } from "./SkillsRow";
import { ByAircraftTable, ByAirportTable, Change, EventsTable } from "./TrendTables";

const PERIODS = [{ value: "30", label: "30 days" }, { value: "90", label: "90 days" }, { value: "365", label: "1 year" }, { value: "all", label: "All" }];
const field = "h-8 min-w-32 rounded-md border border-line bg-surface px-2 text-sm text-fg outline-none focus-visible:border-accent";

/** One pilot's flights over time, the way an airline's FOQA programme reads its fleet: rates, dispersion and the landing area that keeps costing points. */
export function TrendsScreen() {
  const [filter, setFilter] = useState<TrendFilter>({});
  const { state, refresh, refreshing } = useTrends(filter);
  const { theme } = useTheme();
  const filtered = filter.aircraft !== undefined || filter.airport !== undefined || filter.days !== undefined;
  // The filter options come from all flights, so they stay once loaded even when the filter empties the rest.
  const [options, setOptions] = useState<Trends["options"] | null>(null);
  if (state.status === "ready" && state.data.options !== options) setOptions(state.data.options);

  let body: ReactNode;
  if (state.status === "loading") body = <div className="grid gap-5 xl:grid-cols-3" aria-label="Loading trends">{Array.from({ length: 3 }, (_, i) => <div key={i} className="panel h-64 animate-pulse" />)}</div>;
  else if (state.status === "error") body = <div className="panel"><EmptyState icon={<ServerCrash size={20} />} title="Could not read the trends" description={state.message} action={<Button variant="secondary" size="sm" onClick={refresh}>Try again</Button>} /></div>;
  else if (!state.data.flights) body = <NoFlights filtered={filtered} onReset={() => setFilter({})} recordings={state.data.recordings} />;
  else body = <Content data={state.data} theme={theme} stale={refreshing} />;

  return (
    <main className="flex w-full flex-col gap-5 px-6 py-6 2xl:px-8">
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <p className="eyebrow">Flight data monitoring</p>
          <h1 className="mt-1 font-display text-2xl font-semibold tracking-tight">Trends</h1>
          <p className="mt-1 text-sm text-fg-2">An airline reads its fleet in rates and trends, not single flights. This does the same for yours: where landings end up, what keeps firing, whether it is getting better.</p>
        </div>
        <Button variant="secondary" size="sm" onClick={refresh} loading={refreshing}><RefreshCw size={15} aria-hidden="true" />Refresh</Button>
      </header>
      {options ? <Filters options={options} filter={filter} onChange={setFilter} filtered={filtered} /> : null}
      {body}
    </main>
  );
}

function Filters({ options, filter, onChange, filtered }: { options: Trends["options"]; filter: TrendFilter; onChange: (filter: TrendFilter) => void; filtered: boolean }) {
  const set = (patch: Partial<TrendFilter>) => onChange({ ...filter, ...patch });
  return (
    <div className="flex flex-wrap items-end gap-x-5 gap-y-3" role="group" aria-label="Filters">
      <label className="flex flex-col gap-1"><span className="eyebrow">Aircraft</span>
        <select className={field} value={filter.aircraft ?? ""} onChange={(e) => set({ aircraft: e.target.value || undefined })}>
          <option value="">All types</option>
          {options.aircraft.map((a) => <option key={a.icao} value={a.icao}>{a.icao} ({a.flights})</option>)}
        </select>
      </label>
      <label className="flex flex-col gap-1"><span className="eyebrow">Airport</span>
        <select className={field} value={filter.airport ?? ""} onChange={(e) => set({ airport: e.target.value || undefined })}>
          <option value="">All airports</option>
          {options.airports.map((a) => <option key={a.icao} value={a.icao}>{a.icao} ({a.landings})</option>)}
        </select>
      </label>
      <div className="flex flex-col gap-1"><span className="eyebrow">Period</span>
        <SegmentedControl label="Period" options={PERIODS} value={filter.days === undefined ? "all" : String(filter.days)} onValueChange={(v) => set({ days: v === "all" ? undefined : Number(v) })} />
      </div>
      {filtered ? <Button variant="ghost" size="sm" onClick={() => onChange({})}>Clear filters</Button> : null}
    </div>
  );
}

function NoFlights({ filtered, onReset, recordings }: { filtered: boolean; onReset: () => void; recordings: Trends["recordings"] }) {
  const openFolder = useOpenFolder();
  return (
    <div className="panel">
      {/* Recordings that exist but did not count say why, so an empty screen is never a mystery. */}
      <LeftOut recordings={recordings} className="px-5 pt-4" />
      {filtered ? (
        <EmptyState icon={<SearchX size={20} />} title="No flights match these filters" description="Widen the period or pick another aircraft or airport." action={<Button variant="secondary" size="sm" onClick={onReset}>Clear filters</Button>} />
      ) : (
        <EmptyState icon={<TrendingUp size={20} />} title="No flights to compare yet"
          description="Once you have flown a few times, this screen shows your landing score over time, where your touchdowns land on the runway, which landing area costs the most points and which events keep firing. A trend needs at least 5 flights in each half of the period."
          action={<Button variant="secondary" size="sm" onClick={openFolder}><FolderOpen size={15} aria-hidden="true" />Open flights folder</Button>} />
      )}
    </div>
  );
}

const Kpi = ({ label, children, note }: { label: string; children: ReactNode; note?: ReactNode }) => (
  <div className="bg-surface px-4 py-3">
    <dt className="eyebrow">{label}</dt>
    <dd className="mt-1 flex flex-wrap items-baseline gap-x-3 gap-y-0.5"><span className="readout text-xl">{children}</span>{note}</dd>
  </div>
);

/** Plain words for the recordings Trends did not count (docs/trends.md). */
function LeftOut({ recordings: r, className = "" }: { recordings: Trends["recordings"]; className?: string }) {
  const out = r.total - r.counted;
  if (!out && !r.partial) return null;
  const parts = [
    out ? `${number(out)} ${out === 1 ? "recording was" : "recordings were"} left out: ${[r.ground_only ? `${number(r.ground_only)} never left the ground` : "", r.too_short ? `${number(r.too_short)} ${r.too_short === 1 ? "was" : "were"} too short to be a flight (under a minute or a mile in the air)` : ""].filter(Boolean).join(", ")}.` : "",
    r.partial ? `${number(r.partial)} counted ${r.partial === 1 ? "flight" : "flights"} started or ended in the air.` : "",
  ];
  return <p className={`text-xs text-fg-3 ${className}`}>{parts.filter(Boolean).join(" ")}</p>;
}

const Nothing = ({ text }: { text: string }) => <EmptyState size="sm" icon={<TrendingUp size={18} />} title="Nothing to chart yet" description={text} />;

function Content({ data, theme, stale }: { data: Trends; theme: Theme; stale: boolean }) {
  const { score } = data;
  const noScores = data.scored === 0;
  return (
    <div className={`flex flex-col gap-5 transition-opacity ${stale ? "opacity-60" : ""}`}>
      <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-[var(--radius-panel)] border border-line bg-line sm:grid-cols-4">
        <Kpi label="Flights" note={<span className="text-xs text-fg-3">of {number(data.recordings.total)} recordings</span>}>{number(data.flights)}</Kpi>
        <Kpi label="Hours flown">{number(data.hours, 1)} h</Kpi>
        <Kpi label="Landings" note={data.scored < data.landing_count ? <span className="text-xs text-fg-3">{data.scored} scored</span> : undefined}>{number(data.landing_count)}</Kpi>
        <Kpi label="Mean landing score" note={score.mean === null ? undefined : <Change change={delta(score.recent, score.earlier)} higherIsBetter />}>
          {score.mean === null ? "–" : number(score.mean, 1)}
        </Kpi>
      </dl>
      <LeftOut recordings={data.recordings} className="-mt-2" />
      <div className="grid items-start gap-5 xl:grid-cols-3">
        <Panel className="xl:col-span-2" title="Landing score over time" why="Each dot is one landing, coloured by grade. The line smooths the last five, so you see the direction rather than the luck of one touchdown.">
          {noScores ? <Nothing text="No scored landings yet. A landing is scored when the recording has enough data around touchdown." /> : <ScoreChart landings={data.landings} theme={theme} />}
        </Panel>
        <Panel title="Landing areas" why="Which part of the landing costs points. Longer bars are worse; the share is how often it reached level 2 or more.">
          {noScores ? <Nothing text="Appears once a landing has been scored." /> : <DomainBars domains={data.domains} />}
        </Panel>
      </div>
      <SkillsRow skills={data.skills} />
      <Panel title="Touchdown footprint" why="Where the wheels touched, past the threshold and off the centreline. A tight cluster near the aiming point is a stable technique; a long smear is a habit to fix.">
        <Footprint landings={data.landings} />
      </Panel>
      <Panel title="Events" why="How many flights in every 100 had each event: a count means little without its denominator, and repeats within one flight count once. The arrow compares your newer half of flights with the older half.">
        <EventsTable events={data.events} />
      </Panel>
      <div className="grid items-start gap-5 lg:grid-cols-2">
        <Panel title="By aircraft" why="Does one type carry the problems? Mean score and events per flight side by side."><ByAircraftTable rows={data.by_aircraft} /></Panel>
        <Panel title="By airport" why="Some runways and approaches are harder. Zone share and offset show where the dispersion comes from."><ByAirportTable rows={data.by_airport} /></Panel>
      </div>
    </div>
  );
}
