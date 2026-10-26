// One source for the colours that canvas (uPlot) and WebGL (MapLibre) draw, which cannot read CSS variables.
// applyPalette() mirrors them onto :root as --phase-* and --level-* for the CSS side.
import type { Level } from "../types";
import type { Grade } from "./limits";

export type Theme = "light" | "dark";

const PHASES: Record<string, [light: string, dark: string]> = {
  Standing: ["#7a8396", "#5d6679"],
  Taxi: ["#8c7b4a", "#9a8b5e"],
  Takeoff: ["#1d8f5c", "#34b87e"],
  InitialClimb: ["#13827d", "#3cbab2"],
  EnRoute: ["#2f6fc0", "#5294e4"],
  Approach: ["#7650c8", "#9c79e8"],
  Landing: ["#b8408a", "#d963ab"],
  GoAround: ["#c06a1c", "#e58f42"],
};
const UNKNOWN_PHASE: [string, string] = ["#8a93a6", "#7d8799"];

const LEVELS: Record<Level, [light: string, dark: string]> = {
  high: ["#c9362c", "#f0605a"],
  medium: ["#b86a00", "#f2a33a"],
  low: ["#8a7a00", "#e2cf5c"],
  advisory: ["#2f6fb3", "#73aee6"],
};

const pick = ([light, dark]: [string, string], theme: Theme) => (theme === "dark" ? dark : light);
export const phaseColor = (phase: string, theme: Theme) => pick(PHASES[phase] ?? UNKNOWN_PHASE, theme);
export const levelColor = (level: Level, theme: Theme) => pick(LEVELS[level], theme);

/** CSS colour of a landing grade (lib/limits.ts); "ok" is a muted green so only "good" reads as a clear pass. */
export const GRADE_COLOR: Record<Grade, string> = {
  good: "var(--success)",
  ok: "color-mix(in oklab, var(--success) 45%, var(--text-muted))",
  firm: "var(--level-medium)",
  review: "var(--level-high)",
};

/** Hex colour with alpha, for fills. */
export const alpha = (hex: string, a: number) => `${hex}${Math.round(a * 255).toString(16).padStart(2, "0")}`;

export function applyPalette(theme: Theme) {
  const root = document.documentElement.style;
  for (const phase of Object.keys(PHASES)) root.setProperty(`--phase-${phase}`, phaseColor(phase, theme));
  root.setProperty("--phase-unknown", pick(UNKNOWN_PHASE, theme));
  for (const level of Object.keys(LEVELS) as Level[]) root.setProperty(`--level-${level}`, levelColor(level, theme));
}

/** Resolved value of a CSS custom property, for canvas text and grid colours. */
export const cssVar = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
