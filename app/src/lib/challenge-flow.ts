// Pure helpers of the Challenges screen: where the attempt is in the flow, and how a challenge's scoring method is worded.
import type { AttemptView, PreflightLine } from "../types.ts";
import { OPEN_STATES } from "./attempts.ts";

/** Library to result, the way a challenge is flown; the stepper above the briefing draws it. */
export const FLOW = [
  { id: "choose", label: "Choose", description: "Pick a challenge" },
  { id: "brief", label: "Briefing", description: "Read the conditions" },
  { id: "preflight", label: "Pre-flight", description: "Fix what is red" },
  { id: "fly", label: "Fly", description: "Land past the gate" },
  { id: "result", label: "Result", description: "Graded here" },
];

/** The step in progress (`FLOW.length`: done). An open attempt decides; otherwise a failing check keeps it at Pre-flight, also
 * after a graded attempt, since the next one (Fly again) cannot start until it is fixed. */
export function flowStep(lines: PreflightLine[] | null, attempts: AttemptView[]): number {
  const last = attempts[0];
  if (last && OPEN_STATES.includes(last.state)) return last.state === "rollout" || last.state === "finished" ? 4 : 3;
  const blocked = !lines || lines.some((l) => l.state === "fail");
  if (last && last.result) return blocked ? 2 : FLOW.length;
  return blocked ? 2 : 3;
}

export interface ScorePart { name: string; label: string; weight: number }
const part = (name: string, label: string, weight: number): ScorePart => ({ name, label, weight });

const METHODS: Record<string, ScorePart[]> = {
  "landing-1": [part("touchdown_point", "touchdown point", 25), part("sink_rate", "sink rate", 25), part("centreline", "centreline", 15), part("stability", "stability from the gate", 10), part("peak_g", "peak g", 10), part("bounce", "bounce", 10), part("rollout", "rollout", 5)],
  // The six domains of the debrief's assessment (docs/scoring.md §2.3).
  "landing-3": [part("stability", "approach stability", 25), part("energy", "threshold and flare energy", 10), part("point", "touchdown point", 20), part("lateral", "lateral control", 15), part("contact", "touchdown contact", 20), part("rollout", "rollout", 10)],
};

/** The method a challenge is graded with: landing-2 is retired and graded as landing-3 (docs/challenges.md); null for a version this app does not know. */
export const methodOf = (version: string) => (version === "landing-2" ? "landing-3" : version in METHODS ? version : null);

/** A part's name in words; the same name means different things in landing-1 and landing-3 ("stability"). */
export const partLabel = (version: string, name: string) => METHODS[methodOf(version) ?? ""]?.find((p) => p.name === name)?.label ?? name.replace(/_/g, " ");

const points = (list: { label: string; weight: number }[]) => list.map((p) => `${p.label} ${Math.round(p.weight * 10) / 10}`).join(", ");

/**
 * The Scoring fact of a briefing: the method the challenge names and its parts with their weights. `scored` is the newest
 * graded attempt's score: when landing-3 shared a domain's weight out (no data for it), the note says what that attempt used.
 */
export function scoringFact(version: string, scored?: NonNullable<AttemptView["result"]>["score"]): { value: string; note: string } {
  const method = methodOf(version);
  if (!method) return { value: version, note: "A method this version of the app does not know: its attempts cannot be graded here." };
  const parts = METHODS[method]!;
  const used = scored?.version === method ? scored.parts : undefined;
  const shared = used && used.some((p) => Math.abs(p.weight - (parts.find((n) => n.name === p.name)?.weight ?? 0)) > 0.05);
  return {
    value: method,
    note: [
      `0 to 100: ${points(parts)}.`,
      method === "landing-3" ? "The debrief's landing assessment run from the gate; a safety cap can hold the total below the parts' sum, and a domain without data shares its weight out." : "",
      version !== method ? `${version} is retired: it is graded as ${method}.` : "",
      shared ? `Latest attempt: ${points(used.map((p) => ({ label: partLabel(method, p.name), weight: p.weight })))}.` : "",
    ].filter(Boolean).join(" "),
  };
}
