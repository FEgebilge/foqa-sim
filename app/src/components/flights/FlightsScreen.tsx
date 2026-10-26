import { useMemo, useState } from "react";
import { FolderOpen, PlaneLanding, RefreshCw, SearchX, ServerCrash } from "lucide-react";
import { Button } from "@ui/components/button/button";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import { SearchField } from "@ui/components/search-field/search-field";
import SegmentedControl from "@ui/components/segmented-control/segmented-control";
import type { SortState } from "@ui/components/sortable-data-table/sortable-data-table";
import { duration, LEVEL_LABEL, LEVELS, number } from "../../lib/format";
import type { RecorderState } from "../../lib/use-backend";
import { useFlights } from "../../lib/use-backend";
import type { Level } from "../../types";
import { useOpenFolder } from "../use-open-folder";
import { toRow } from "./flight-row";
import { FlightsTable } from "./FlightsTable";

type LevelFilter = "all" | Level | "clean";

export function FlightsScreen({ recorder }: { recorder: RecorderState | null }) {
  const { state, refresh, refreshing } = useFlights();
  const openFolder = useOpenFolder();
  const [query, setQuery] = useState("");
  const [level, setLevel] = useState<LevelFilter>("all");
  const [sort, setSort] = useState<SortState>({ key: "date", direction: "desc" });

  const flights = useMemo(() => (state.status === "ready" ? state.data : []), [state]);
  const rows = useMemo(() => flights.map(toRow), [flights]);
  const counts = useMemo(() => {
    const byLevel: Record<LevelFilter, number> = { all: rows.length, high: 0, medium: 0, low: 0, advisory: 0, clean: 0 };
    for (const row of rows) byLevel[row.flight.worst_level ?? "clean"]++;
    return byLevel;
  }, [rows]);
  const visible = useMemo(() => {
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    return rows.filter((row) => {
      if (level !== "all" && (row.flight.worst_level ?? "clean") !== level) return false;
      const { aircraft, departure, arrival, id } = row.flight;
      const text = [aircraft?.icao, aircraft?.title, departure, arrival, id].filter(Boolean).join(" ").toLowerCase();
      return words.every((word) => text.includes(word));
    });
  }, [rows, query, level]);

  const totals = useMemo(() => ({
    time: flights.reduce((sum, f) => sum + f.duration_s, 0),
    landings: flights.reduce((sum, f) => sum + f.landings, 0),
    withEvents: flights.filter((f) => f.worst_level && f.worst_level !== "advisory").length,
  }), [flights]);

  const filterOptions = [
    { value: "all", label: "All" },
    ...LEVELS.map((value) => ({ value, label: LEVEL_LABEL[value] })),
    { value: "clean", label: "Clean" },
  ].map((option) => ({ ...option, accessory: <span className="readout text-xs text-fg-3">{counts[option.value as LevelFilter]}</span> }));

  const actions = (
    <div className="flex items-center gap-2">
      <Button variant="secondary" size="sm" onClick={refresh} loading={refreshing}><RefreshCw size={15} aria-hidden="true" />Refresh</Button>
      <Button variant="secondary" size="sm" onClick={openFolder}><FolderOpen size={15} aria-hidden="true" />Open flights folder</Button>
    </div>
  );

  let body;
  if (state.status === "loading") body = <TableSkeleton />;
  else if (state.status === "error") {
    body = <div className="panel"><EmptyState icon={<ServerCrash size={20} />} title="Could not read the flights" description={state.message} action={<Button variant="secondary" size="sm" onClick={refresh}>Try again</Button>} /></div>;
  } else if (!flights.length) {
    const waiting = recorder?.value.state === "waiting_for_sim";
    body = (
      <div className="panel">
        <EmptyState
          icon={<PlaneLanding size={20} />}
          title="No flights yet"
          description={waiting
            ? "Start MSFS 2024 and fly. FOQA-Sim records in the background and lists the flight here, analysed, when it closes."
            : "Load a flight in MSFS 2024. It is recorded automatically and appears here, analysed, when it closes."}
          action={<Button variant="secondary" size="sm" onClick={openFolder}><FolderOpen size={15} aria-hidden="true" />Open flights folder</Button>}
        />
      </div>
    );
  } else {
    body = (
      <>
        <div className="flex flex-wrap items-center gap-3">
          <div className="w-full max-w-sm"><SearchField label="Search flights" hideLabel size="sm" placeholder="Aircraft, airport or flight ID" value={query} onValueChange={setQuery} /></div>
          <SegmentedControl label="Worst event level" options={filterOptions} value={level} onValueChange={(value) => setLevel(value as LevelFilter)} />
          <span className="readout ml-auto text-xs text-fg-3" aria-live="polite">{visible.length === rows.length ? `${rows.length} flights` : `${visible.length} of ${rows.length} flights`}</span>
        </div>
        {visible.length ? <FlightsTable rows={visible} sort={sort} onSortChange={setSort} /> : (
          <div className="panel">
            <EmptyState size="sm" icon={<SearchX size={18} />} title="No flights match" description="Change the search or the level filter." action={<Button variant="secondary" size="sm" onClick={() => { setQuery(""); setLevel("all"); }}>Clear filters</Button>} />
          </div>
        )}
      </>
    );
  }

  return (
    <main className="flex w-full flex-col gap-5 px-6 py-6 2xl:px-8">
      <header className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <p className="eyebrow">Flight data monitoring</p>
          <h1 className="mt-1 font-display text-2xl font-semibold tracking-tight">Flights</h1>
          <p className="mt-1 text-sm text-fg-2">Every recorded flight, analysed against the FOQA rule set. Open one for its debrief.</p>
        </div>
        {actions}
      </header>
      {flights.length ? (
        <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-[var(--radius-panel)] border border-line bg-line sm:grid-cols-4">
          <Stat label="Flights" value={number(flights.length)} />
          <Stat label="Flight time" value={duration(totals.time)} />
          <Stat label="Landings" value={number(totals.landings)} />
          <Stat label="With low or higher events" value={number(totals.withEvents)} />
        </dl>
      ) : null}
      {body}
    </main>
  );
}

const Stat = ({ label, value }: { label: string; value: string }) => (
  <div className="bg-surface px-4 py-3">
    <dt className="eyebrow">{label}</dt>
    <dd className="readout mt-1 text-xl">{value}</dd>
  </div>
);

const TableSkeleton = () => (
  <div className="panel flex flex-col gap-3 p-4" aria-label="Loading flights">
    {Array.from({ length: 6 }, (_, i) => <div key={i} className="h-9 animate-pulse rounded-md bg-muted" />)}
  </div>
);
