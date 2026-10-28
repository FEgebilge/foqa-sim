import { useMemo, useState } from "react";
import { BookOpen, FolderOpen, PlaneLanding, RefreshCw, SearchX, ServerCrash } from "lucide-react";
import { api } from "../../api";
import { Button } from "@ui/components/button/button";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import { SearchField } from "@ui/components/search-field/search-field";
import SegmentedControl from "@ui/components/segmented-control/segmented-control";
import type { SortState } from "@ui/components/sortable-data-table/sortable-data-table";
import { useNotify } from "../../lib/context";
import { aircraftName, duration, flightDate, GRADE_LABEL, GRADES, number } from "../../lib/format";
import type { RecorderState } from "../../lib/use-backend";
import { useFlights } from "../../lib/use-backend";
import type { FlightSummary, Grade } from "../../types";
import { ConfirmDialog } from "../ConfirmDialog";
import { ExportMenu } from "../ExportMenu";
import { useOpenFolder } from "../use-open-folder";
import { toRow } from "./flight-row";
import { FlightsTable } from "./FlightsTable";

type GradeFilter = "all" | Grade;

export function FlightsScreen({ recorder }: { recorder: RecorderState | null }) {
  const { state, refresh, refreshing } = useFlights();
  const openFolder = useOpenFolder();
  const [query, setQuery] = useState("");
  const [grade, setGrade] = useState<GradeFilter>("all");
  const [doomed, setDoomed] = useState<FlightSummary | null>(null);
  const [deleting, setDeleting] = useState(false);
  const notify = useNotify();
  const [sort, setSort] = useState<SortState>({ key: "date", direction: "desc" });

  const flights = useMemo(() => (state.status === "ready" ? state.data : []), [state]);
  const rows = useMemo(() => flights.map(toRow), [flights]);
  const counts = useMemo(() => {
    const byGrade: Record<GradeFilter, number> = { all: rows.length, good: 0, fair: 0, poor: 0 };
    for (const row of rows) byGrade[row.flight.assessment.grade]++;
    return byGrade;
  }, [rows]);
  const visible = useMemo(() => {
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    return rows.filter((row) => {
      if (grade !== "all" && row.flight.assessment.grade !== grade) return false;
      const { aircraft, departure, arrival, id } = row.flight;
      const text = [aircraft?.icao, aircraft?.title, departure, arrival, id].filter(Boolean).join(" ").toLowerCase();
      return words.every((word) => text.includes(word));
    });
  }, [rows, query, grade]);

  const totals = useMemo(() => ({
    time: flights.reduce((sum, f) => sum + f.duration_s, 0),
    landings: flights.reduce((sum, f) => sum + f.landings, 0),
    needAttention: flights.filter((f) => f.assessment.grade !== "good").length,
  }), [flights]);

  const filterOptions = [
    { value: "all", label: "All" },
    ...GRADES.map((value) => ({ value, label: GRADE_LABEL[value] })),
  ].map((option) => ({ ...option, accessory: <span className="readout text-xs text-fg-3">{counts[option.value as GradeFilter]}</span> }));
  const recordingId = recorder?.value.state === "recording" ? recorder.value.flight_id : undefined;

  const confirmDelete = () => {
    if (!doomed) return;
    setDeleting(true);
    api.deleteFlight(doomed.id)
      .then(refresh, (error: unknown) => notify({ title: "Could not delete the flight", body: String(error), tone: "error" }))
      .finally(() => { setDeleting(false); setDoomed(null); });
  };

  const logbook = (identity: boolean) => api.exportLogbook(identity).then(
    (file) => { if (file) notify({ title: "Logbook exported", body: <>{file}. In Little Navmap: Logbook → Import from CSV.</> }); },
    (error: unknown) => notify({ title: "Could not export the logbook", body: String(error), tone: "error" }));
  const actions = (
    <div className="flex items-center gap-2">
      {flights.length ? <ExportMenu label="Export" items={[{ label: "Little Navmap logbook", hint: "Every flight with an airport, as a CSV its logbook imports", icon: <BookOpen size={16} />, run: logbook }]} /> : null}
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
          <SegmentedControl label="Assessment" options={filterOptions} value={grade} onValueChange={(value) => setGrade(value as GradeFilter)} />
          <span className="readout ml-auto text-xs text-fg-3" aria-live="polite">{visible.length === rows.length ? `${rows.length} flights` : `${visible.length} of ${rows.length} flights`}</span>
        </div>
        {visible.length ? <FlightsTable rows={visible} sort={sort} onSortChange={setSort} onDelete={(row) => setDoomed(row.flight)} recordingId={recordingId} /> : (
          <div className="panel">
            <EmptyState size="sm" icon={<SearchX size={18} />} title="No flights match" description="Change the search or the assessment filter." action={<Button variant="secondary" size="sm" onClick={() => { setQuery(""); setGrade("all"); }}>Clear filters</Button>} />
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
          <Stat label="Fair or poor" value={number(totals.needAttention)} />
        </dl>
      ) : null}
      {body}
      <ConfirmDialog open={doomed !== null} title="Delete this flight?" confirmLabel="Delete flight" busy={deleting} onConfirm={confirmDelete} onCancel={() => setDoomed(null)}>
        {doomed ? `${aircraftName(doomed.aircraft)}, ${flightDate(doomed.started_utc)} (${doomed.id}). ` : ""}Its recording files are deleted from disk for good. This cannot be undone.
      </ConfirmDialog>
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
