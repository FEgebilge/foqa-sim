import type { ReactNode } from "react";
import { ArrowLeft, ArrowRight, FolderOpen, RefreshCw } from "lucide-react";
import { Button } from "@ui/components/button/button";
import { aircraftName, duration, flightDate, flightTimeUtc } from "../../lib/format";
import { openFlights } from "../../lib/route";
import type { Debrief } from "../../types";
import { EventCounts, LevelBadge } from "../level";

interface Props { flight: Debrief["flight"]; refreshing: boolean; onRefresh: () => void; onOpenFolder: () => void }

export function DebriefHeader({ flight, refreshing, onRefresh, onOpenFolder }: Props) {
  const facts: [string, ReactNode][] = [
    ["Date", <>{flightDate(flight.started_utc)} <span className="text-fg-3">{flightTimeUtc(flight.started_utc)}</span></>],
    ["Duration", duration(flight.duration_s)],
    ["Landings", String(flight.landings)],
    ["Profile", flight.profile],
  ];
  return (
    <header className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <Button variant="ghost" size="sm" onClick={openFlights}><ArrowLeft size={15} aria-hidden="true" />All flights</Button>
        <div className="flex items-center gap-2">
          <Button variant="secondary" size="sm" onClick={onRefresh} loading={refreshing}><RefreshCw size={15} aria-hidden="true" />Refresh</Button>
          <Button variant="secondary" size="sm" onClick={onOpenFolder}><FolderOpen size={15} aria-hidden="true" />Open flights folder</Button>
        </div>
      </div>
      <div className="flex flex-wrap items-end justify-between gap-x-8 gap-y-3">
        <div className="min-w-0">
          <p className="eyebrow">Debrief · <span className="normal-case tracking-normal">{flight.id}</span></p>
          <h1 className="mt-1 flex flex-wrap items-baseline gap-x-3 font-display text-2xl font-semibold tracking-tight">
            <span className={flight.aircraft ? "" : "text-fg-3"}>{aircraftName(flight.aircraft)}</span>
            <span className="inline-flex items-center gap-2 text-xl text-fg-2">
              <span className={flight.departure ? "" : "text-fg-3"}>{flight.departure ?? "----"}</span>
              <ArrowRight size={18} className="text-fg-3" aria-label="to" />
              <span className={flight.arrival ? "" : "text-fg-3"}>{flight.arrival ?? "----"}</span>
            </span>
          </h1>
          {flight.aircraft?.icao ? <p className="mt-0.5 text-sm text-fg-2">{flight.aircraft.title}</p> : null}
        </div>
        <div className="flex items-center gap-3">
          <LevelBadge level={flight.worst_level} size="md" />
          <EventCounts counts={flight.event_counts} />
        </div>
      </div>
      <dl className="flex flex-wrap gap-x-8 gap-y-2 border-y border-line-subtle py-2.5">
        {facts.map(([label, value]) => (
          <div key={label} className="flex items-baseline gap-2">
            <dt className="eyebrow">{label}</dt>
            <dd className="readout text-sm">{value}</dd>
          </div>
        ))}
      </dl>
    </header>
  );
}
