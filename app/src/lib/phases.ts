// The phase boundaries in words, from the values in effect for one flight (docs/phases.md). Display only.
import type { Debrief, PhaseSpan } from "../types";

type Values = Debrief["flight"]["phase_values"];

interface Row { phase: string; starts: string; ends: string; keys: string[] }

/** `{key}` in a template becomes the flight's value with its unit; a value the flight does not carry reads "the set height". */
const fill = (template: string, values: Values) =>
  template.replace(/\{(\w+)\}/g, (_, key: string) => {
    const v = values[key]?.value;
    return v === undefined ? "the set height" : `${v} ${key.endsWith("_kt") ? "kt" : "ft"}`;
  });

const ROWS: Row[] = [
  { phase: "Standing", starts: "on the ground, under 2 kt", ends: "the aircraft moves", keys: [] },
  { phase: "Taxi", starts: "moving on the ground, under {roll_kt}", ends: "a takeoff roll starts, or it stops", keys: ["roll_kt"] },
  { phase: "Takeoff", starts: "on the ground, above {roll_kt}", ends: "35 ft above the runway", keys: ["roll_kt"] },
  { phase: "InitialClimb", starts: "35 ft above the runway", ends: "{climb_end_ft} above the aerodrome", keys: ["climb_end_ft"] },
  { phase: "EnRoute", starts: "the initial climb ends", ends: "descending through {approach_ft} above the aerodrome", keys: ["approach_ft"] },
  { phase: "Approach", starts: "descending through {approach_ft} above the aerodrome", ends: "below {flare_ft} above the runway", keys: ["approach_ft", "flare_ft"] },
  { phase: "Landing", starts: "below {flare_ft} above the runway", ends: "under {roll_kt} after touchdown", keys: ["flare_ft", "roll_kt"] },
  { phase: "GoAround", starts: "lowest point of an approach, then a {go_around_rise_ft} climb without touching down", ends: "the climb ends, {go_around_drop_ft} below its peak", keys: ["go_around_rise_ft", "go_around_drop_ft"] },
];

export interface PhaseRule { phase: string; starts: string; ends: string; sources: string[]; seen: PhaseSpan[] }

/** The rows for the phases this flight went through, in flight order, each with where its values came from. */
export function phaseRules(phases: PhaseSpan[], values: Values = {}): PhaseRule[] {
  const order = [...new Set(phases.map((p) => p.phase))];
  return order.flatMap((phase) => {
    const row = ROWS.find((r) => r.phase === phase);
    if (!row) return [];
    const sources = [...new Set(row.keys.flatMap((k) => (values[k] ? [values[k].source] : [])))];
    return [{ phase, starts: fill(row.starts, values), ends: fill(row.ends, values), sources, seen: phases.filter((p) => p.phase === phase) }];
  });
}
