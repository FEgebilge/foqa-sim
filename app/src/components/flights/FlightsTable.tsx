import { ArrowRight } from "lucide-react";
import { SortableDataTable, type DataColumn, type SortState } from "@ui/components/sortable-data-table/sortable-data-table";
import { duration, flightDate, flightTimeUtc } from "../../lib/format";
import { flightHref, openFlight } from "../../lib/route";
import type { FlightRow } from "./flight-row";
import { EventCounts, LevelBadge } from "../level";

const Airport = ({ icao }: { icao?: string }) => <span className={icao ? "readout" : "text-fg-3"}>{icao ?? "----"}</span>;

const columns: DataColumn<FlightRow>[] = [
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
  { key: "worst", label: "Worst level", width: 130, render: (_, row) => <LevelBadge level={row.flight.worst_level} /> },
  { key: "duration", label: "Duration", width: 110, numeric: true, render: (_, row) => <span className="readout">{duration(row.duration)}</span> },
  { key: "landings", label: "Landings", width: 115, numeric: true, render: (_, row) => <span className="readout">{row.landings}</span> },
  { key: "events", label: "Events", width: 160, render: (_, row) => <EventCounts counts={row.flight.event_counts} /> },
];

export function FlightsTable({ rows, sort, onSortChange }: { rows: FlightRow[]; sort: SortState; onSortChange: (sort: SortState) => void }) {
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
