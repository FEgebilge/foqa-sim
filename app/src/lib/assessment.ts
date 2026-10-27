// The landing-2 assessment (docs/scoring.md) for display: the domain bars' geometry, the zones of a criterion's band strip
// and the words for values, levels and deductions. Pure functions, so the panel, the landing card's gauges and the EFB
// mirror read one set of bands and cannot disagree with the score.
import type { Anchors, CriterionAssessment, Debrief, DomainAssessment, LandingAssessment } from "../types";
import { number } from "./format";

/** The six zones of the scale: before the ideal anchor, then past each anchor in turn. */
export const ZONES = ["Ideal", "Within tolerance", "Outside tolerance", "Level 1", "Level 2", "Level 3"] as const;
/** Each zone's colour (CSS), ideal to level 3. */
export const ZONE_COLOR = [
  "color-mix(in oklab, var(--success) 55%, transparent)",
  "color-mix(in oklab, var(--success) 24%, transparent)",
  "color-mix(in oklab, var(--foreground) 13%, transparent)",
  "color-mix(in oklab, var(--level-low) 50%, transparent)",
  "color-mix(in oklab, var(--level-medium) 52%, transparent)",
  "color-mix(in oklab, var(--level-high) 55%, transparent)",
] as const;

/** A whole level's colour: 0 within tolerance, 1 low, 2 medium, 3 high. */
export const levelColor = (level: number) => (level <= 0 ? "var(--success)" : level === 1 ? "var(--level-low)" : level === 2 ? "var(--level-medium)" : "var(--level-high)");
export const LEVEL_WORD = ["Level 0", "Level 1", "Level 2", "Level 3"] as const;

/** ℓ in words: ideal, within or outside the checkride tolerance, or the FDM level. */
export function ellWords(ell: number) {
  if (ell <= 0) return "ideal";
  if (ell <= 0.5) return "within tolerance";
  if (ell < 1) return "outside checkride tolerance";
  return `level ${Math.min(3, Math.floor(ell))}`;
}

/** The colour of a continuous level: green within tolerance, grey outside it, then the FDM levels' colours. */
export const ellColor = (ell: number) => (ell <= 0.5 ? "var(--success)" : ell < 1 ? "color-mix(in oklab, var(--foreground) 50%, transparent)" : levelColor(Math.floor(ell)));

/** A domain bar: its track as a share of the heaviest domain, the part lost, and the colour of the item that set it. */
export function domainBar(domain: DomainAssessment, maxWeight = 25) {
  const ell = Math.max(0, ...domain.criteria.filter((c) => c.sets).map((c) => c.ell), ...domain.findings.filter((f) => f.sets).map((f) => f.ell));
  return {
    track: (domain.weight / maxWeight) * 100,
    lost: domain.loss === null ? 0 : Math.min(1, domain.loss) * 100,
    color: ellColor(ell),
    na: domain.loss === null,
  };
}

/** One stretch of a band strip, in the criterion's unit; `from`/`to` may be ±Infinity. */
export interface Zone { from: number; to: number; zone: number }

/**
 * The zones along the value axis, ascending: the below side's anchors far to near, the ideal stretch, the above side's.
 * A missing anchor merges its stretch into the zone nearer the ideal (ℓ runs straight to the next anchor); beyond the last
 * anchor the zone holds. A side with no anchor at all grades nothing on that side: ideal to ±Infinity.
 */
export function zones({ above, below }: { above: Anchors; below: Anchors }): Zone[] {
  const side = (anchors: Anchors) => anchors.flatMap((at, k) => (at === null ? [] : [{ at, zone: k + 1 }]));
  const up = side(above), down = side(below);
  const out: Zone[] = [];
  let from = -Infinity;
  for (let i = down.length - 1; i >= 0; i--) { out.push({ from, to: down[i]!.at, zone: down[i]!.zone }); from = down[i]!.at; }
  // From the first below anchor to the first above anchor: before the first anchor ℓ is 0.
  for (let i = 0; i <= up.length; i++) {
    const to = up[i]?.at ?? Infinity;
    out.push({ from, to, zone: i ? up[i - 1]!.zone : 0 });
    from = to;
  }
  return out.filter((z) => z.to > z.from);
}

/** The zone holding `x`. */
export const zoneOf = (list: Zone[], x: number) => list.find((z) => x <= z.to)?.zone ?? list.at(-1)?.zone ?? 0;

/** The strip's ends: every finite anchor and the value, padded; a side without anchors gets a stretch as wide as the other. */
export function stripRange(c: CriterionAssessment): [number, number] {
  const finite = [...c.bands.above, ...c.bands.below].filter((a): a is number => a !== null);
  const all = [...finite, c.value];
  let lo = Math.min(...all), hi = Math.max(...all);
  const span = hi - lo || Math.abs(hi) || 1;
  if (c.bands.below.every((a) => a === null)) lo = Math.min(lo, (c.bands.above.find((a) => a !== null) ?? hi) - span * 0.25);
  if (c.bands.above.every((a) => a === null)) hi = Math.max(hi, (c.bands.below.find((a) => a !== null) ?? lo) + span * 0.25);
  const pad = (hi - lo) * 0.06;
  return [lo - pad, hi + pad];
}

/** The finite anchors of a criterion with their texts from the debrief (`bands_text`), ascending and each value once. */
export function anchorTicks(c: CriterionAssessment) {
  const pairs = (["below", "above"] as const).flatMap((side) => c.bands[side].map((at, k) => ({ at, text: c.bands_text[side][k] })));
  const byValue = new Map(pairs.filter((p): p is { at: number; text: string } => p.at !== null && !!p.text).map((p) => [p.at, p.text]));
  return [...byValue].map(([at, text]) => ({ at, text })).sort((a, b) => a.at - b.at);
}

/** What the value of a criterion is measured against, where its title does not say. */
export function againstText(c: CriterionAssessment, landing: LandingAssessment) {
  const s = landing.speeds;
  const speed = s ? (landing.class === "light" ? `approach speed ${number(s.vapp_kt)}` : `reference speed ${number(s.vref_kt)}`) : "";
  if (c.id === "A1" || c.id === "B2") return speed ? `vs ${speed} kt` : "";
  if (c.id === "B1") return "vs 50 ft over the threshold";
  if (c.id === "C1") return "vs the aiming point";
  return "";
}

/** The landing assessment of the touchdown at `t`. */
export const landingAt = (debrief: Debrief, t: number) => debrief.flight.assessment_detail.landings?.find((l) => Math.abs(l.t_s - t) < 1e-3);

/** The criterion of a landing by id, the worse half when graded twice. */
export function criterion(landing: LandingAssessment | undefined, id: string) {
  const all = landing?.domains.flatMap((d) => d.criteria).filter((c) => c.id === id) ?? [];
  return all.sort((a, b) => b.ell - a.ell)[0];
}

/** The domain that cost the most, for the opening view and the EFB's one line. */
export const topDomain = (landing: LandingAssessment) => [...landing.domains].sort((a, b) => b.points - a.points)[0];

