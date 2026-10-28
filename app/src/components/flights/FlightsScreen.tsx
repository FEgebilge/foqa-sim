import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { BookOpen, FolderOpen, Plane, MapPin, RefreshCw, SearchX, ServerCrash, Tag, Trash2 } from "lucide-react";
import { api } from "../../api";
import { BulkActionBar, type BulkAction } from "@ui/components/bulk-action-bar/bulk-action-bar";
import { Button } from "@ui/components/button/button";
import { DateRangePicker, type DateRange } from "@ui/components/date-range-picker/date-range-picker";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import { FilterToolbar, type FilterChip, type FilterField } from "@ui/components/filter-toolbar/filter-toolbar";
import { SearchField } from "@ui/components/search-field/search-field";
import SegmentedControl from "@ui/components/segmented-control/segmented-control";
import { sortRowsBy, type SortState } from "@ui/components/sortable-data-table/sortable-data-table";
import { useNotify } from "../../lib/context";
import { duration, flightDate, GRADE_LABEL, GRADES, number } from "../../lib/format";
import type { RecorderState, Setup } from "../../lib/use-backend";
import { useFlights, useLoad } from "../../lib/use-backend";
import { ConfirmDialog } from "../ConfirmDialog";
import { ExportMenu } from "../ExportMenu";
import { FirstRun } from "../home/FirstRun";
import { LatestFlight } from "../home/LatestFlight";
import { useOpenFolder } from "../use-open-folder";
import { dayKey, toRow, type FlightRow } from "./flight-row";
import { FlightsTable } from "./FlightsTable";
import { bytes, cleared, filtered, isFiltered, options, PAGE, saveState, savedState, visibleRows, type GradeFilter, type ListState } from "./list-state";

const plural = (n: number, one: string, other = `${one}s`) => `${number(n)} ${n === 1 ? one : other}`;
const day = (key: string) => { const [y, m, d] = key.split("-").map(Number); return new Date(y!, m! - 1, d!); };
const localDay = (date: Date) => dayKey(date.toISOString())!;
/** The days a trashed flight waits before it is purged (`TRASH_KEEP` in app/src-tauri/src/store.rs). */
const TRASH_DAYS = 30;
const loadUsage = () => api.storageUsage();

export function FlightsScreen({ recorder, setup }: { recorder: RecorderState | null; setup?: Setup }) {
  const { state, refresh, refreshing, patch } = useFlights();
  const { state: usage, reload: reloadUsage } = useLoad(loadUsage);
  const openFolder = useOpenFolder();
  const notify = useNotify();
  const [view, setView] = useState<ListState>(savedState);
  const update = useCallback((change: Partial<ListState>) => setView((old) => ({ ...old, ...change })), []);
  const [selected, setSelected] = useState<string[]>([]);
  const [purging, setPurging] = useState<"ask" | "busy" | null>(null);

  // The list's place survives a trip into a debrief: the state on every change, the scroll when the screen goes.
  const latest = useRef(view);
  useEffect(() => { latest.current = view; saveState(view); }, [view]);
  useEffect(() => () => saveState({ ...latest.current, scrollY: window.scrollY }), []);

  const flights = useMemo(() => (state.status === "ready" ? state.data : []), [state]);
  const rows = useMemo(() => flights.map(toRow), [flights]);
  const base = useMemo(() => filtered(rows, view), [rows, view]);
  const visible = useMemo(() => visibleRows(base, view), [base, view]);
  // Sorted here, not in the table, so only the page shown is rendered.
  const sorted = useMemo(() => sortRowsBy(visible, view.sort, (row, key) => row[key]), [visible, view.sort]);
  const page = useMemo(() => sorted.slice(0, view.limit), [sorted, view.limit]);
  const counted = useMemo(() => rows.filter((row) => row.counted), [rows]);
  const choices = useMemo(() => options(view.showAll ? rows : counted), [rows, counted, view.showAll]);
  const others = useMemo(() => base.filter((row) => !row.counted), [base]);
  const counts = useMemo(() => {
    const byGrade: Record<GradeFilter, number> = { all: 0, good: 0, fair: 0, poor: 0 };
    for (const row of base) if (view.showAll || row.counted) { byGrade.all++; byGrade[row.flight.assessment.grade]++; }
    return byGrade;
  }, [base, view.showAll]);

  const ready = state.status === "ready";
  const restored = useRef(false);
  useLayoutEffect(() => {
    if (!ready || restored.current) return;
    restored.current = true;
    window.scrollTo(0, savedState().scrollY);
  }, [ready]);

  const shownSelection = useMemo(() => {
    const ids = new Set(visible.map((row) => row.id));
    return selected.filter((id) => ids.has(id));
  }, [selected, visible]);
  const recordingId = recorder?.value.state === "recording" ? recorder.value.flight_id : undefined;

  const restore = useCallback((ids: string[]) => api.restoreFlights(ids).then(
    (back) => { refresh(); reloadUsage(); notify({ title: `${plural(back.length, "flight")} restored` }); },
    (error: unknown) => notify({ title: "Could not restore", body: String(error), tone: "error" })), [refresh, reloadUsage, notify]);

  const remove = useCallback((ids: string[]) => api.deleteFlights(ids).then(({ trashed, failed }) => {
    const gone = new Set(trashed);
    patch((list) => list.filter((f) => !gone.has(f.id)));
    setSelected((old) => old.filter((id) => !gone.has(id)));
    reloadUsage();
    if (trashed.length) {
      notify({
        title: `${plural(trashed.length, "flight")} moved to the trash`,
        body: `Kept for ${TRASH_DAYS} days, then deleted for good.${failed.length ? ` Not moved: ${failed.join("; ")}.` : ""}`,
        action: { label: "Undo", run: () => void restore(trashed) },
      });
    } else notify({ title: "Nothing was moved to the trash", body: failed.join("; "), tone: "error" });
  }, (error: unknown) => notify({ title: "Could not delete", body: String(error), tone: "error" })), [patch, reloadUsage, notify, restore]);

  // Stable, so the table keeps its columns while the search text changes.
  const askDelete = useCallback((row: FlightRow) => void remove([row.id]), [remove]);
  const onSort = useCallback((sort: SortState) => update({ sort: sort as ListState["sort"] }), [update]);

  const purge = () => {
    setPurging("busy");
    api.purgeTrash().then(
      (n) => notify({ title: `${plural(n, "flight")} deleted for good` }),
      (error: unknown) => notify({ title: "Could not empty the trash", body: String(error), tone: "error" }),
    ).finally(() => { setPurging(null); reloadUsage(); });
  };

  const logbook = (identity: boolean, ids?: string[]) => api.exportLogbook(identity, ids).then(
    (file) => { if (file) notify({ title: "Logbook exported", body: <>{file}. In Little Navmap: Logbook → Import from CSV.</> }); },
    (error: unknown) => notify({ title: "Could not export the logbook", body: String(error), tone: "error" }));

  const fields: FilterField[] = [
    { id: "aircraft", label: "Aircraft", icon: <Plane size={14} />, options: choices.aircraft },
    { id: "airport", label: "Airport", icon: <MapPin size={14} />, options: choices.airports },
    ...(choices.tags.length ? [{ id: "tag", label: "Tag", icon: <Tag size={14} />, options: choices.tags }] : []),
  ];
  const range: DateRange | null = view.from && view.to ? { start: day(view.from), end: day(view.to) } : null;
  const chips: FilterChip[] = [
    ...(view.aircraft ? [{ id: "aircraft", label: "Aircraft", value: view.aircraft }] : []),
    ...(view.airport ? [{ id: "airport", label: "Airport", value: view.airport }] : []),
    ...(view.tag ? [{ id: "tag", label: "Tag", value: view.tag }] : []),
    ...(range ? [{ id: "dates", label: "Dates", value: `${flightDate(range.start.toISOString())} – ${flightDate(range.end.toISOString())}` }] : []),
  ];
  const removeChip = (id: string) => update(id === "dates" ? { from: undefined, to: undefined } : { [id]: undefined } as Partial<ListState>);

  const bulk: BulkAction[] = [
    { id: "delete", label: "Move to trash", icon: <Trash2 size={16} />, tone: "danger", primary: true,
      disabled: shownSelection.length === 1 && shownSelection[0] === recordingId ? "This flight is being recorded" : false,
      confirm: { prompt: `Move ${plural(shownSelection.length, "flight")} to the trash?`, confirmLabel: "Move to trash" },
      onSelect: () => remove(shownSelection.filter((id) => id !== recordingId)) },
    { id: "export", label: "Export logbook", icon: <BookOpen size={16} />, onSelect: () => logbook(false, shownSelection) },
  ];

  const filterOptions = [
    { value: "all", label: "All" },
    ...GRADES.map((value) => ({ value, label: GRADE_LABEL[value] })),
  ].map((option) => ({ ...option, accessory: <span className="readout text-xs text-fg-3">{counts[option.value as GradeFilter]}</span> }));

  const totals = useMemo(() => ({
    time: counted.reduce((sum, row) => sum + row.duration, 0),
    landings: counted.reduce((sum, row) => sum + row.landings, 0),
    needAttention: counted.filter((row) => row.flight.assessment.grade !== "good").length,
  }), [counted]);
  const storage = usage.status === "ready" ? usage.data : null;

  let body;
  if (state.status === "loading") body = <TableSkeleton />;
  else if (state.status === "error") {
    body = <div className="panel"><EmptyState icon={<ServerCrash size={20} />} title="Could not read the flights" description={state.message} action={<Button variant="secondary" size="sm" onClick={refresh}>Try again</Button>} /></div>;
  } else if (!flights.length) body = <FirstRun recorder={recorder} setup={setup} />;
  else {
    const hidden = view.showAll ? 0 : others.length;
    const [shownTotal, noun] = view.showAll ? [rows.length, "recording"] : [counted.length, "flight"];
    body = (
      <>
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center gap-3">
            <div className="w-full max-w-sm"><SearchField label="Search flights" hideLabel size="sm" placeholder="Aircraft, airport, date, title, notes or tag" value={view.query} onValueChange={(query) => update({ query })} /></div>
            <SegmentedControl label="Assessment" options={filterOptions} value={view.grade} onValueChange={(grade) => update({ grade: grade as GradeFilter })} />
            <span className="readout ml-auto text-xs text-fg-3" aria-live="polite">{visible.length === shownTotal ? plural(shownTotal, noun) : `${number(visible.length)} of ${plural(shownTotal, noun)}`}</span>
          </div>
          <FilterToolbar filters={chips} onRemove={removeChip} onClearAll={() => setView(cleared)} label="Flight filters"
            addFilter={{ fields, align: "start", onAdd: (chip) => update({ [chip.id]: chip.value } as Partial<ListState>) }}>
            <DateRangePicker label="Dates" size="sm" presets="filter" clearable value={range}
              onChange={(next) => update(next ? { from: localDay(next.start), to: localDay(next.end) } : { from: undefined, to: undefined })} />
          </FilterToolbar>
          {others.length ? (
            <p className="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-fg-2" aria-live="polite">
              <span>{hidden
                ? `${plural(hidden, "recording")} that ${hidden === 1 ? "isn't a flight is" : "aren't flights are"} hidden: never airborne, or under a minute and a mile in the air.`
                : `Showing ${plural(others.length, "recording")} that ${others.length === 1 ? "isn't a flight" : "aren't flights"}, marked Ground only or Too short.`}</span>
              <Button variant="ghost" size="sm" onClick={() => update({ showAll: !view.showAll })}>{hidden ? "Show" : "Hide"}</Button>
              <Button variant="ghost" size="sm" onClick={() => { update({ showAll: true }); setSelected(others.map((row) => row.id).filter((id) => id !== recordingId)); }}>Select them to clean up</Button>
            </p>
          ) : null}
        </div>
        {visible.length ? (
          <FlightsTable rows={page} sort={view.sort} onSortChange={onSort} selected={shownSelection} onSelectionChange={setSelected} onDelete={askDelete} recordingId={recordingId} />
        ) : (
          <div className="panel">
            <EmptyState size="sm" icon={<SearchX size={18} />} title="No flights match" description={hidden && !isFiltered(view) ? "Every recording here is one that isn't a flight." : "Change the search or the filters."}
              action={<Button variant="secondary" size="sm" onClick={() => setView(cleared)}>Clear filters</Button>} />
          </div>
        )}
        {sorted.length > page.length ? (
          <div className="flex items-center justify-center gap-2">
            <span className="text-sm text-fg-3">Showing {number(page.length)} of {number(sorted.length)}</span>
            <Button variant="secondary" size="sm" onClick={() => update({ limit: view.limit + PAGE })}>Show {number(Math.min(PAGE, sorted.length - page.length))} more</Button>
            <Button variant="ghost" size="sm" onClick={() => update({ limit: sorted.length })}>Show all</Button>
          </div>
        ) : null}
        <BulkActionBar count={shownSelection.length} total={visible.length} itemName={{ one: "flight", other: "flights" }} actions={bulk}
          onClear={() => setSelected([])} onSelectAll={() => setSelected(visible.map((row) => row.id))} allSelected={shownSelection.length === visible.length} label="Selected flights" />
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
        <div className="flex items-center gap-2">
          {flights.length ? <ExportMenu label="Export" items={[{ label: "Little Navmap logbook", hint: "Every flight with an airport, as a CSV its logbook imports", icon: <BookOpen size={16} />, run: (identity) => logbook(identity) }]} /> : null}
          <Button variant="secondary" size="sm" onClick={refresh} loading={refreshing}><RefreshCw size={15} aria-hidden="true" />Refresh</Button>
        </div>
      </header>
      {flights.length ? <LatestFlight flights={flights} /> : null}
      {flights.length ? (
        <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-[var(--radius-panel)] border border-line bg-line md:grid-cols-5">
          <Stat label="Flights" value={number(counted.length)} />
          <Stat label="Flight time" value={duration(totals.time)} />
          <Stat label="Landings" value={number(totals.landings)} />
          <Stat label="Fair or poor" value={number(totals.needAttention)} />
          <div className="col-span-2 flex flex-col bg-surface px-4 py-3 md:col-span-1">
            <dt className="eyebrow">On disk</dt>
            <dd className="mt-1 flex flex-wrap items-baseline gap-x-2">
              <span className="readout text-xl">{storage ? bytes(storage.bytes) : "…"}</span>
              {storage?.trash.flights ? <span className="text-xs text-fg-3">+ {bytes(storage.trash.bytes)} in the trash</span> : null}
            </dd>
            <dd className="mt-1 flex flex-wrap gap-1">
              <Button variant="ghost" size="sm" onClick={openFolder}><FolderOpen size={14} aria-hidden="true" />Open folder</Button>
              {storage?.trash.flights ? <Button variant="ghost" size="sm" onClick={() => setPurging("ask")}><Trash2 size={14} aria-hidden="true" />Empty trash ({storage.trash.flights})</Button> : null}
            </dd>
          </div>
        </dl>
      ) : null}
      {body}
      <ConfirmDialog open={purging !== null} title="Empty the trash?" confirmLabel="Delete for good" busy={purging === "busy"} onConfirm={purge} onCancel={() => setPurging(null)}>
        {storage ? `${plural(storage.trash.flights, "deleted flight")}, ${bytes(storage.trash.bytes)}. ` : ""}Their recording files are deleted from disk for good; this cannot be undone. Flights in the trash are deleted by themselves {TRASH_DAYS} days after they were moved there.
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
