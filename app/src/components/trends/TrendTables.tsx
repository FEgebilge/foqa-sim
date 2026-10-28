import { useMemo } from "react";
import { ArrowDownRight, ArrowUpRight, Minus } from "lucide-react";
import { SortableDataTable, type DataColumn } from "@ui/components/sortable-data-table/sortable-data-table";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { levelRank, number } from "../../lib/format";
import { delta, direction } from "../../lib/trends";
import type { Trends } from "../../types";
import { LevelBadge } from "../level";

type Event = Trends["events"][number];
type EventRow = { rule: string; title: string; level: string; rank: number; flights: number; per_100: number; change: number | null; event: Event };

const NOT_ENOUGH = "Not enough flights yet: a trend needs at least 5 flights in each half.";

/** Recent against earlier as an arrow; for event rates, down is good. */
export function Change({ change, higherIsBetter, digits = 1, unit = "" }: { change: number | null; higherIsBetter: boolean; digits?: number; unit?: string }) {
  if (change === null) return <Tooltip content={NOT_ENOUGH}><span className="cursor-help text-xs text-fg-3">not enough data</span></Tooltip>;
  const way = direction(change, higherIsBetter, 0.5 * 10 ** -digits);
  const Icon = way === "same" ? Minus : change > 0 ? ArrowUpRight : ArrowDownRight;
  const tone = way === "better" ? "text-success" : way === "worse" ? "text-danger" : "text-fg-3";
  return (
    <span className={`readout inline-flex items-center gap-1 text-xs ${tone}`} aria-label={`${way === "same" ? "No change" : `${change > 0 ? "Up" : "Down"} ${number(Math.abs(change), digits)}${unit}, ${way}`}`}>
      <Icon size={14} aria-hidden="true" />{way === "same" ? "flat" : `${change > 0 ? "+" : "−"}${number(Math.abs(change), digits)}${unit}`}
    </span>
  );
}

const eventColumns: DataColumn<EventRow>[] = [
  {
    key: "title", label: "Rule", compact: "title",
    render: (_, row) => <span className="flex min-w-0 flex-col leading-tight"><span className="truncate font-medium">{row.title}</span><span className="readout truncate text-[11px] text-fg-3">{row.rule}</span></span>,
  },
  { key: "rank", label: "Level", width: 110, numeric: false, render: (_, row) => <LevelBadge level={row.event.level} /> },
  { key: "flights", label: "Flights", width: 104, numeric: true, render: (_, row) => <span className="readout">{row.flights}</span> },
  { key: "per_100", label: "Per 100 flights", width: 170, numeric: true, render: (_, row) => <span className="readout">{number(row.per_100, 1)}</span> },
  {
    key: "change", label: "Recent vs earlier", width: 160, numeric: false,
    render: (_, row) => (
      <span className="inline-flex items-center gap-2">
        <Change change={row.change} higherIsBetter={false} />
        {row.change !== null ? <span className="readout text-[11px] text-fg-3">{number(row.event.earlier_per_100 ?? 0, 1)} → {number(row.event.recent_per_100 ?? 0, 1)}</span> : null}
      </span>
    ),
  },
];

/** Each rule that fired, how many flights it touched and its rate per 100 flights, sortable. */
export function EventsTable({ events }: { events: Event[] }) {
  const rows = useMemo<EventRow[]>(() => events.map((e) => ({ rule: e.rule, title: e.title, level: e.level, rank: levelRank(e.level), flights: e.flights, per_100: e.per_100, change: delta(e.recent_per_100, e.earlier_per_100), event: e })), [events]);
  return (
    <SortableDataTable<EventRow> rows={rows} columns={eventColumns} rowKey="rule" caption="Events by rule" itemName={{ one: "rule", other: "rules" }} emptyMessage="No events in these flights."
      defaultSort={{ key: "per_100", direction: "desc" }} />
  );
}

type AircraftRow = Trends["by_aircraft"][number];
const aircraftColumns: DataColumn<AircraftRow>[] = [
  { key: "icao", label: "Aircraft", compact: "title", render: (_, r) => <span className="readout font-medium">{r.icao}</span> },
  { key: "flights", label: "Flights", numeric: true, render: (_, r) => <span className="readout">{r.flights}</span> },
  { key: "landings", label: "Landings", numeric: true, render: (_, r) => <span className="readout">{r.landings}</span> },
  { key: "mean_score", label: "Mean score", numeric: true, render: (_, r) => <span className="readout">{r.mean_score === null ? "–" : number(r.mean_score, 1)}</span> },
  { key: "events_per_flight", label: "Events / flight", numeric: true, render: (_, r) => <span className="readout">{number(r.events_per_flight, 1)}</span> },
];

export const ByAircraftTable = ({ rows }: { rows: AircraftRow[] }) => (
  <SortableDataTable<AircraftRow> rows={rows} columns={aircraftColumns} rowKey="icao" caption="By aircraft" itemName={{ one: "type", other: "types" }} defaultSort={{ key: "flights", direction: "desc" }} />
);

type AirportRow = Trends["by_airport"][number];
const airportColumns: DataColumn<AirportRow>[] = [
  { key: "icao", label: "Airport", compact: "title", render: (_, r) => <span className="readout font-medium">{r.icao}</span> },
  { key: "landings", label: "Landings", numeric: true, render: (_, r) => <span className="readout">{r.landings}</span> },
  { key: "mean_score", label: "Mean score", numeric: true, render: (_, r) => <span className="readout">{r.mean_score === null ? "–" : number(r.mean_score, 1)}</span> },
  { key: "in_tdz", label: "In zone", numeric: true, render: (_, r) => <span className="readout">{r.in_tdz === null ? "–" : `${Math.round(r.in_tdz * 100)}%`}</span> },
  { key: "mean_cross_m", label: "Mean offset", numeric: true, render: (_, r) => <span className="readout">{r.mean_cross_m === null ? "–" : `${r.mean_cross_m >= 0 ? "R" : "L"} ${number(Math.abs(r.mean_cross_m), 1)} m`}</span> },
];

export const ByAirportTable = ({ rows }: { rows: AirportRow[] }) => (
  <SortableDataTable<AirportRow> rows={rows} columns={airportColumns} rowKey="icao" caption="By airport" itemName={{ one: "airport", other: "airports" }} defaultSort={{ key: "landings", direction: "desc" }} />
);
