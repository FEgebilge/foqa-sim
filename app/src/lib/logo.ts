// The attitude-indicator mark (horizon line and aircraft symbol in a ring) on a 26-unit grid: the header's logo and the
// landing card's mark draw the same paths.
export const LOGO = {
  size: 26,
  ring: { cx: 13, cy: 13, r: 11.5 },
  horizon: "M1.8 14.5 Q13 11 24.2 14.5",
  symbol: "M6.5 13 H10.5 L13 15.5 L15.5 13 H19.5",
} as const;
