// Events and flights in plain language: the one-line verdict and the events of a landing. The words of each event
// (`detail`) come from its rule.
import type { Debrief, FlightEvent, PhaseSpan } from "../types";
import { LEVEL_LABEL, LEVELS, number } from "./format";
import { groupLandings } from "./landings";

/** The events, the most severe first, then the earliest. */
export const bySeverity = (events: FlightEvent[]) => LEVELS.flatMap((level) => events.filter((e) => e.level === level));

/** From the start of the approach that ended in this touchdown to 10 s into the rollout. */
export function landingWindow(t: number, phases: PhaseSpan[]): [number, number] {
  const before = phases.filter((p) => p.start_s <= t);
  const approach = before.findLast((p) => p.phase === "Approach") ?? before.at(-1);
  return [Math.min(approach?.start_s ?? t, t), t + 10];
}
export const eventsIn = (events: FlightEvent[], [start, end]: [number, number]) => events.filter((e) => e.end_s >= start && e.start_s <= end);

/** One sentence: the worst level, the key issue and where the aircraft landed. */
export function verdict({ flight, events, touchdowns }: Debrief) {
  const key = bySeverity(events)[0];
  const landingList = groupLandings(touchdowns);
  const runway = landingList.at(-1)?.touchdown.runway;
  const landed = !landingList.length ? "no landing recorded"
    : runway ? `landed on runway ${runway.runway} at ${runway.icao}`
      : `landed${flight.arrival ? ` at ${flight.arrival}` : ""}, runway not identified`;
  const landings = landingList.length > 1 ? ` (${landingList.length} landings)` : "";
  if (!key) return `Clean flight: no rule exceeded; ${landed}${landings}.`;
  const same = events.filter((e) => e.level === key.level).length - 1;
  const more = same ? `, plus ${number(same)} more ${LEVEL_LABEL[key.level].toLowerCase()} event${same > 1 ? "s" : ""}` : "";
  return `${LEVEL_LABEL[key.level]}: ${key.title.toLowerCase()}${key.detail ? ` (${key.detail})` : ""}${more}; ${landed}${landings}.`;
}
