/**
 * The status tones shared by badges, lanes, ratings and chips; the same union as `BadgeTone`. Meaning never rests on
 * the tone alone: pair it with an icon or text.
 *
 * CSS mapping (from `badge.module.css`): a `[data-tone]` element sets a private `--_tone`, then tints from it.
 *   neutral → var(--text-secondary), info → var(--accent), success → var(--success), warning → var(--warning),
 *   danger → var(--danger).
 *   Tint:   color-mix(in oklab, var(--_tone) 10%, var(--surface))
 *   Border: color-mix(in oklab, var(--_tone) 25%, var(--border))
 * Neutral uses var(--surface-muted) and var(--border) as its tint and border, and info text uses var(--accent-strong).
 */
export type Tone = "neutral" | "info" | "success" | "warning" | "danger";
