import { memo, useMemo } from "react";
import { MotionConfig } from "motion/react";
import { ArrowRight, Trash2 } from "lucide-react";
import { Badge } from "@ui/components/badge/badge";
import { Button } from "@ui/components/button/button";
import { SortableDataTable, type DataColumn, type SortState } from "@ui/components/sortable-data-table/sortable-data-table";
import { duration, flightDate, flightTimeUtc } from "../../lib/format";
import { flightHref, openFlight } from "../../lib/route";
import type { RecordingKind } from "../../types";
import type { FlightRow } from "./flight-row";
import { EventCounts, GradeBadge } from "../level";

/** Above this many rows the rows stop gliding when sorted: so many layout animations at once stutter. */
const ANIMATED_ROWS = 60;
const NOT_A_FLIGHT: Record<Exclude<RecordingKind, "flight">, string> = { ground_only: "Ground only", too_short: "Too short" };

const Airport = ({ icao }: { icao?: string }) => <span className={icao ? "readout" : "text-fg-3"}>{icao ?? "----"}</span>;

const columnsFor = (onDelete: (row: FlightRow) => void, recordingId?: string): DataColumn<FlightRow>[] => [
  {
    key: "date", label: "Flight", width: 210, numeric: false,
    render: (_, { flight, aircraft }) => {
      const title = flight.meta?.title;
      const kind = flight.kind && flight.kind !== "flight" ? flight.kind : undefined;
      return (
        <a href={flightHref(flight.id)} className="flex min-w-0 flex-col leading-tight text-fg no-underline hover:text-accent-strong" aria-label={`Open debrief of ${title ? `${title}, ` : ""}${flightDate(flight.started_utc)} ${aircraft}`}>
          <span className="truncate font-medium" title={title}>{title ?? flightDate(flight.started_utc)}</span>
          <span className="flex items-center gap-1.5 text-xs text-fg-3">
            <span className="readout truncate">{title ? `${flightDate(flight.started_utc)} ` : ""}{flightTimeUtc(flight.started_utc)}</span>
            {kind ? <Badge size="sm" tone="neutral" animated={false} title="Not counted as a flight in Trends">{NOT_A_FLIGHT[kind]}</Badge> : null}
          </span>
        </a>
      );
    },
  },
  {
    key: "aircraft", label: "Aircraft", width: 200,
    render: (_, row) => (
      <span className="flex min-w-0 flex-col leading-tight">
        <span className={row.flight.aircraft ? "readout font-medium" : "text-fg-3"}>{row.aircraft}</span>
        {row.flight.aircraft?.icao ? <span className="truncate text-xs text-fg-3" title={row.flight.aircraft.title}>{row.flight.aircraft.title}</span> : null}
      </span>
    ),
  },
  {
    key: "route", label: "Route", width: 140,
    render: (_, row) => (
      <span className="inline-flex items-center gap-1.5">
        <Airport icao={row.flight.departure} />
        <ArrowRight size={14} className="text-fg-3" aria-label="to" />
        <Airport icao={row.flight.arrival} />
      </span>
    ),
  },
  {
    key: "tags", label: "Tags", width: 170, sortable: false,
    render: (_, row) => {
      const tags = row.flight.meta?.tags ?? [];
      return tags.length ? (
        <span className="flex flex-wrap gap-1" title={tags.join(", ")}>
          {tags.slice(0, 3).map((tag) => <Badge key={tag} size="sm" tone="neutral" animated={false}>{tag}</Badge>)}
          {tags.length > 3 ? <span className="readout text-xs text-fg-3">+{tags.length - 3}</span> : null}
        </span>
      ) : <span className="text-xs text-fg-3">–</span>;
    },
  },
  { key: "assessment", label: "Assessment", width: 150, render: (_, row) => <GradeBadge assessment={row.flight.assessment} /> },
  { key: "duration", label: "Duration", width: 105, numeric: true, render: (_, row) => <span className="readout">{duration(row.duration)}</span> },
  { key: "landings", label: "Landings", width: 105, numeric: true, render: (_, row) => <span className="readout">{row.landings}</span> },
  { key: "events", label: "Events", width: 150, render: (_, row) => <EventCounts counts={row.flight.event_counts} /> },
  {
    key: "actions", label: "Actions", width: 64, sortable: false, compact: "actions",
    render: (_, row) => {
      const recording = row.id === recordingId;
      const label = recording ? "This flight is being recorded" : `Move flight ${flightDate(row.flight.started_utc)} ${row.aircraft} to the trash`;
      // A native title, not a Tooltip: one per row is a lot of portals at 500 rows.
      return <Button variant="ghost" size="sm" iconOnly label={label} title={recording ? label : "Move to the trash"} disabled={recording} onClick={() => onDelete(row)}><Trash2 size={15} aria-hidden="true" /></Button>;
    },
  },
];

interface Props {
  /** Sorted already, one page of them. */
  rows: FlightRow[];
  sort: SortState;
  onSortChange: (sort: SortState) => void;
  selected: string[];
  onSelectionChange: (ids: string[]) => void;
  onDelete: (row: FlightRow) => void;
  recordingId?: string;
}

export const FlightsTable = memo(function FlightsTable({ rows, sort, onSortChange, selected, onSelectionChange, onDelete, recordingId }: Props) {
  const columns = useMemo(() => columnsFor(onDelete, recordingId), [onDelete, recordingId]);
  return (
    <div className="flights-table">
      <MotionConfig reducedMotion={rows.length > ANIMATED_ROWS ? "always" : "user"}>
        <SortableDataTable<FlightRow>
          rows={rows}
          columns={columns}
          rowKey="id"
          caption="Recorded flights"
          sort={sort}
          onSortChange={onSortChange}
          manualSort
          selectable
          selectedKeys={selected}
          onSelectionChange={onSelectionChange}
          rowClick="open"
          onRowOpen={openRow}
          itemName={{ one: "flight", other: "flights" }}
          labels={LABELS}
          stickyHeader
          stickyColumn
        />
      </MotionConfig>
    </div>
  );
});

const openRow = (row: FlightRow) => openFlight(row.id);
/** The checkbox is named after the first column's value: the start time, or the id without one. */
const LABELS = { selectRow: (name: string) => `Select ${/^\d{4}-/.test(name) ? `flight of ${flightDate(name)} ${flightTimeUtc(name)}` : name}` };
