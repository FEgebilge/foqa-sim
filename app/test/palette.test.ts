import assert from "node:assert/strict";
import test from "node:test";
import { levelColor, type Theme } from "../src/lib/palette.ts";
import type { Level } from "../src/types.ts";

/** oklch(L% C H) to sRGB 0..1 (Ottosson's matrices), for the theme backgrounds in styles/night-flight.css. */
function oklch(l: number, c: number, h: number): [number, number, number] {
  const a = c * Math.cos((h * Math.PI) / 180), b = c * Math.sin((h * Math.PI) / 180);
  const [L, M, S] = [l + 0.3963377774 * a + 0.2158037573 * b, l - 0.1055613458 * a - 0.0638541728 * b, l - 0.0894841775 * a - 1.291485548 * b].map((v) => v ** 3) as [number, number, number];
  const lin = [4.0767416621 * L - 3.3077115913 * M + 0.2309699292 * S, -1.2684380046 * L + 2.6097574011 * M - 0.3413193965 * S, -0.0041960863 * L - 0.7034186147 * M + 1.707614701 * S];
  return lin.map((v) => Math.min(1, Math.max(0, v))) as [number, number, number];
}
const luminance = ([r, g, b]: number[]) => 0.2126 * r + 0.7152 * g + 0.0722 * b;
const srgb = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
const fromHex = (value: string) => [1, 3, 5].map((i) => srgb(parseInt(value.slice(i, i + 2), 16) / 255));
const contrast = (a: number[], b: number[]) => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi! + 0.05) / (lo! + 0.05);
};

// Backgrounds text can sit on: page, panel and muted fills (--background, --surface, --surface-muted, --surface-raised).
const BACKGROUNDS: Record<Theme, Record<string, [number, number, number]>> = {
  light: { background: oklch(0.986, 0.005, 255), surface: oklch(0.995, 0.003, 255), muted: oklch(0.948, 0.009, 255) },
  dark: { background: oklch(0.165, 0.035, 258), surface: oklch(0.195, 0.038, 258), raised: oklch(0.225, 0.04, 258), muted: oklch(0.255, 0.042, 258) },
};

test("every level colour reads as text (AA, 4.5:1) on every background of both themes", () => {
  for (const theme of ["light", "dark"] as Theme[])
    for (const level of ["high", "medium", "low", "advisory"] as Level[])
      for (const [name, bg] of Object.entries(BACKGROUNDS[theme])) {
        const ratio = contrast(fromHex(levelColor(level, theme)), bg);
        assert.ok(ratio >= 4.5, `${level} on ${theme} ${name}: ${ratio.toFixed(2)}:1`);
      }
});

test("the contrast helper matches known values", () => {
  assert.ok(Math.abs(contrast(fromHex("#000000"), fromHex("#ffffff")) - 21) < 0.01);
});
