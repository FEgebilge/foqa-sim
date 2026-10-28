import type { RideBand } from "../types";

/** ISO 2631-1 Annex C comfort bands (docs/skills.md), lowest first; `upTo` is the RMS (m/s²) the band ends below, as the backend draws it. */
export const RIDE_BANDS: { band: RideBand; short: string; upTo: number; range: string; color: string }[] = [
  { band: "not uncomfortable", short: "Smooth", upTo: 0.315, range: "< 0.315", color: "var(--success)" },
  { band: "a little uncomfortable", short: "A little", upTo: 0.63, range: "0.315–0.63", color: "var(--level-advisory)" },
  { band: "fairly uncomfortable", short: "Fairly", upTo: 1, range: "0.5–1", color: "var(--level-low)" },
  { band: "uncomfortable", short: "Uncomf.", upTo: 1.6, range: "0.8–1.6", color: "var(--level-medium)" },
  { band: "very uncomfortable", short: "Very", upTo: 2.5, range: "1.25–2.5", color: "var(--level-high)" },
  { band: "extremely uncomfortable", short: "Extreme", upTo: Infinity, range: "> 2", color: "var(--level-high)" },
];

/** The band an RMS falls in: the lowest that contains it. */
export const rideStep = (rms: number) => RIDE_BANDS.findIndex((b) => rms < b.upTo);
export const rideStepOf = (band: RideBand | null | undefined) => (band ? RIDE_BANDS.findIndex((b) => b.band === band) : -1);

export const BANK_LIMIT_DEG = 30;
export const ROTATION_RATE_DPS = 3;

/** Share (0-1) as a whole percent. */
export const percent = (share: number) => `${Math.round(share * 100)}%`;

/** Share of `total` that `part` is, or null when either is missing. */
export const shareOf = (part: number | null | undefined, total: number | null | undefined) => (part == null || !total ? null : part / total);
