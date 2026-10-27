import { useMemo } from "react";
import { ArrowRight, Trash2 } from "lucide-react";
import { Button } from "@ui/components/button/button";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { SortableDataTable, type DataColumn, type SortState } from "@ui/components/sortable-data-table/sortable-data-table";
import { duration, flightDate, flightTimeUtc } from "../../lib/format";
import { flightHref, openFlight } from "../../lib/route";
import type { FlightRow } from "./flight-row";
import { EventCounts, GradeBadge } from "../level";

const Airport = ({ icao }: { icao?: string }) => <span className={icao ? "readout" : "text-fg-3"}>{icao ?? "----"}</span>;

const columnsFor = (onDelete: (row: FlightRow) => void, recordingId?: string): DataColumn<FlightRow>[] => [
  {
    key: "date", label: "Date", width: 150, numeric: false,
    render: (_, row) => (
      <a href={flightHref(row.id)} className="flex flex-col leading-tight text-fg no-underline hover:text-accent-strong" aria-label={`Open debrief of ${flightDate(row.flight.started_utc)} ${row.aircraft}`}>
        <span className="font-medium">{flightDate(row.flight.started_utc)}</span>
        <span className="readout text-xs text-fg-3">{flightTimeUtc(row.flight.started_utc)}</span>
      </a>
    ),
  },
  {
    key: "aircraft", label: "Aircraft", width: 220,
    render: (_, row) => (
      <span className="flex min-w-0 flex-col leading-tight">
        <span className={row.flight.aircraft ? "readout font-medium" : "text-fg-3"}>{row.aircraft}</span>
        {row.flight.aircraft?.icao ? <span className="truncate text-xs text-fg-3" title={row.flight.aircraft.title}>{row.flight.aircraft.title}</span> : null}
      </span>
    ),
  },
  {
    key: "route", label: "Route", width: 150,
    render: (_, row) => (
      <span className="inline-flex items-center gap-1.5">
        <Airport icao={row.flight.departure} />
        <ArrowRight size={14} className="text-fg-3" aria-label="to" />
        <Airport icao={row.flight.arrival} />
      </span>
    ),
  },
  { key: "assessment", label: "Assessment", width: 150, render: (_, row) => <GradeBadge assessment={row.flight.assessment} /> },
  { key: "duration", label: "Duration", width: 110, numeric: true, render: (_, row) => <span className="readout">{duration(row.duration)}</span> },
  { key: "landings", label: "Landings", width: 115, numeric: true, render: (_, row) => <span className="readout">{row.landings}</span> },
  { key: "events", label: "Events", width: 160, render: (_, row) => <EventCounts counts={row.flight.event_counts} /> },
  {
    key: "actions", label: "Actions", width: 64, sortable: false, compact: "actions",
    render: (_, row) => {
      const recording = row.id === recordingId;
      const label = recording ? "This flight is being recorded" : `Delete flight ${flightDate(row.flight.started_utc)} ${row.aircraft}`;
      return (
        <Tooltip content={recording ? label : "Delete flight"}>
          <Button variant="ghost" size="sm" iconOnly label={label} disabled={recording} onClick={() => onDelete(row)}><Trash2 size={15} aria-hidden="true" /></Button>
        </Tooltip>
      );
    },
  },
];

interface Props { rows: FlightRow[]; sort: SortState; onSortChange: (sort: SortState) => void; onDelete: (row: FlightRow) => void; recordingId?: string }

export function FlightsTable({ rows, sort, onSortChange, onDelete, recordingId }: Props) {
  const columns = useMemo(() => columnsFor(onDelete, recordingId), [onDelete, recordingId]);
  return (
    <div className="flights-table">
      <SortableDataTable<FlightRow>
        rows={rows}
        columns={columns}
        rowKey="id"
        caption="Recorded flights"
        sort={sort}
        onSortChange={onSortChange}
        rowClick="open"
        onRowOpen={(row) => openFlight(row.id)}
        itemName={{ one: "flight", other: "flights" }}
        stickyHeader
        stickyColumn
      />
    </div>
  );
}
