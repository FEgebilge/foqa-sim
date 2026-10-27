// A landing is the first touchdown plus the bounces that follow it (`bounce: true`), in time order.
import type { Touchdown } from "../types";

export interface Landing {
  /** First contact: the one the landing is graded on. */
  touchdown: Touchdown;
  /** Further contacts of the same landing. */
  bounces: Touchdown[];
}

export function groupLandings(touchdowns: Touchdown[]): Landing[] {
  const landings: Landing[] = [];
  for (const td of touchdowns) {
    const last = landings.at(-1);
    if (td.bounce && last) last.bounces.push(td);
    else landings.push({ touchdown: td, bounces: [] });
  }
  return landings;
}

/** "TD1" for a landing's first contact, "TD1b", "TD1c"… for its bounces. */
export const contactLabel = (landing: number, bounce = 0) => `TD${landing + 1}${bounce ? String.fromCharCode(97 + bounce) : ""}`;

/** Every contact of every landing with its label and time, for the timeline and the map. */
export const contacts = (landings: Landing[]) =>
  landings.flatMap(({ touchdown, bounces }, i) => [touchdown, ...bounces].map((td, k) => ({ td, label: contactLabel(i, k) })));
