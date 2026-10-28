import type { Debrief } from "../types";
import { aircraftName } from "./format.ts";

export interface IdentityNotice {
  kind: "unidentified" | "class-default" | "not-assessed";
  title: string;
  body: string;
}

/** "a", "a and b", "a, b and c". */
const list = (items: string[]) => (items.length < 2 ? items.join("") : `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`);

/** Where the class of an aircraft without a profile came from, by identification step. */
const BASIS: Record<string, string> = {
  sim: "its class comes from what the sim reports about it (maximum weight, engines, gear)",
  weight: "its class comes from the recorded gross weight",
  unknown: "nothing gave its class, so the transport defaults apply",
};

/**
 * What the debrief must say about how the aircraft was graded: not identified at all, graded on a class default
 * (no type profile, and so which type-limit checks were off), or a kind the landing score is not for.
 */
export function identityNotices(flight: Debrief["flight"]): IdentityNotice[] {
  const id = flight.identification;
  const off = id?.unchecked.length ? list(id.unchecked) : "type limits (flap speeds, tail clearance, hard-landing inspection)";
  const notices: IdentityNotice[] = [];
  if (!flight.aircraft) {
    notices.push({ kind: "unidentified", title: "Aircraft not identified", body: `The sim did not report which aircraft was flown, so ${off} could not be checked. Rules ran with: ${flight.profile}.` });
  } else if (id && BASIS[id.matched]) {
    notices.push({
      kind: "class-default",
      title: `Graded on the ${flight.class ?? "transport"} class default`,
      body: `No type profile matches ${aircraftName(flight.aircraft)}: ${BASIS[id.matched]}. These checks were off: ${off}. Rules ran with: ${flight.profile}.`,
    });
  }
  if (id && !id.runway_landing) {
    notices.push({
      kind: "not-assessed",
      title: "Landing not assessed for this aircraft kind",
      body: `A ${id.airframe.kind ?? "non-airplane"} gets no airplane landing score and none of the runway landing rules (touchdown point, centreline, crosswind and tailwind limits); its landings count through their other findings only.`,
    });
  }
  return notices;
}
