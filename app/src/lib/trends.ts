// Small pure helpers of the Trends screen.
import type { Grade, TrendLanding } from "../types";

/** Mean of the last `window` values at each point (fewer at the start). */
export function rollingMean(values: number[], window: number): number[] {
  return values.map((_, i) => {
    const part = values.slice(Math.max(0, i - window + 1), i + 1);
    return part.reduce((a, b) => a + b, 0) / part.length;
  });
}

/** Recent minus earlier, null when either half is missing (fewer than 5 in it). */
export const delta = (recent: number | null, earlier: number | null) => (recent === null || earlier === null ? null : recent - earlier);

export type Direction = "better" | "worse" | "same";
/** Whether a change is an improvement; `higherIsBetter` is false for event rates. Changes under `epsilon` read as "same". */
export const direction = (change: number, higherIsBetter: boolean, epsilon = 0.05): Direction =>
  Math.abs(change) < epsilon ? "same" : (change > 0) === higherIsBetter ? "better" : "worse";

/** Landings that were scored full stops, in the order given. */
export const scoredLandings = (landings: TrendLanding[]) => landings.filter((l) => l.score !== undefined && l.full_stop);

/** The domain's mean level 0-3 as a tone: on target, drifting, costing points. */
export const levelTone = (mean: number): Grade => (mean < 0.75 ? "good" : mean < 1.5 ? "fair" : "poor");

export const GRADE_VAR: Record<Grade, string> = { good: "--success", fair: "--warning", poor: "--danger" };
