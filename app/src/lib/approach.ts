// The reference approach (docs/reference-approach.md) as numbers: the profile's lines read from the debrief series, the score's
// bands per stretch of the approach, and the first point each criterion left its tolerance. Pure, so the Node tests can run it.
import type { ApproachProfile, CriterionAssessment, Debrief, LandingAssessment, SeriesValues } from "../types";
import { zones, type Zone } from "./assessment.ts";
import { valueAt } from "./series.ts";

export const FT_PER_NM = 6076;
const TAN3 = (deg: number) => Math.tan((deg * Math.PI) / 180);
/** The reference path's height at a distance: `tch + dist · tan(path)`. */
export const pathHeight = (p: ApproachProfile, distNm: number) => p.tch_ft + distNm * FT_PER_NM * TAN3(p.path_deg);
/** The descent rate the reference path needs at a ground speed, fpm. */
export const pathSink = (p: ApproachProfile, gsKt: number) => ((gsKt * FT_PER_NM) / 60) * TAN3(p.path_deg);

/** What the profile's points show beyond the profile itself: each line at the point's time. */
export interface Lines {
  /** IAS − Vapp, kt. */
  dev: (number | null)[];
  ias: (number | null)[];
  /** Descent rate, fpm, positive down. */
  sink: (number | null)[];
  need: (number | null)[];
  flap: (number | null)[];
  gear: (number | null)[];
}

export function linesOf(series: Debrief["series"], p: ApproachProfile, vapp: number | null): Lines {
  const at = (v: SeriesValues, t: number, how?: "hold") => valueAt(series.t_s, v, t, how);
  const gs = p.t_s.map((t) => at(series.gs_kt, t));
  const ias = p.t_s.map((t) => at(series.ias_kt, t));
  return {
    ias,
    dev: ias.map((v) => (v === null || vapp === null ? null : v - vapp)),
    sink: p.t_s.map((t) => { const vs = at(series.vs_fpm, t); return vs === null ? null : -vs; }),
    need: gs.map((v) => (v === null ? null : pathSink(p, v))),
    flap: p.t_s.map((t) => at(series.flap_deg, t, "hold")),
    gear: p.t_s.map((t) => at(series.gear_ext, t, "hold")),
  };
}

/** A criterion's id in the landing, all its halves (graded above and below the split height, or once). */
export const halves = (landing: LandingAssessment, id: string) => landing.domains.flatMap((d) => d.criteria).filter((c) => c.id === id);

/** The half that grades a point at this height, within the window; none outside it. */
export function halfAt(list: CriterionAssessment[], height: number, landing: LandingAssessment, p: ApproachProfile) {
  if (height > p.top_ft || height < p.bottom_ft) return undefined;
  const split = landing.split_ft ?? 0;
  return list.find((c) => c.height === "all" || (c.height === "above" ? height >= split : height < split));
}

/** One stretch of the x axis graded by one set of bands: the points `from` to `to` (inclusive). */
export interface Segment { from: number; to: number; zones: Zone[] }

/** The bands along the profile: consecutive points graded by the same half share a segment. */
export function segments(landing: LandingAssessment, p: ApproachProfile, id: string): Segment[] {
  const list = halves(landing, id);
  const out: Segment[] = [];
  let open: { half: CriterionAssessment; seg: Segment } | null = null;
  p.height_ft.forEach((h, i) => {
    const half = h === null ? undefined : halfAt(list, h, landing, p);
    if (!half) { open = null; return; }
    if (open?.half === half) open.seg.to = i;
    else { open = { half, seg: { from: i, to: i, zones: zones(half.bands) } }; out.push(open.seg); }
  });
  return out;
}

/** The index where the height first falls to `ft` (the gate), with the distance interpolated; null if it never does. */
export function crossing(p: ApproachProfile, ft: number): number | null {
  for (let i = 1; i < p.height_ft.length; i++) {
    const a = p.height_ft[i - 1], b = p.height_ft[i], da = p.dist_nm[i - 1], db = p.dist_nm[i];
    if (a == null || b == null || da == null || db == null || a < ft || b > ft || a === b) continue;
    return da + ((db - da) * (a - ft)) / (a - b);
  }
  return null;
}

export interface Departure { id: string; label: string; index: number; text: string }


const fixed = (v: number, digits = 0) => v.toLocaleString("en-US", { minimumFractionDigits: digits, maximumFractionDigits: digits });

/** "2.4 nm, 760 ft: 12 kt fast", for each criterion at level 1 or more: the first point it left its tolerance band, as the
 * score found it (`first_out_s`, persistence included), so the list never disagrees with the grade. */
export function departures(landing: LandingAssessment, lines: Lines): Departure[] {
  const p = landing.profile;
  if (!p) return [];
  const out: Departure[] = [];
  const add = (id: string, label: string, index: number, text: string) => {
    const d = p.dist_nm[index], h = p.height_ft[index];
    if (index >= 0 && d != null && h != null) out.push({ id, label, index, text: `${fixed(Math.max(0, d), 1)} nm, ${fixed(h)} ft: ${text}` });
  };
  const outsideBy = (id: string) => {
    const at = halves(landing, id).filter((c) => c.level >= 1 && c.first_out_s != null).map((c) => c.first_out_s!);
    if (!at.length) return -1;
    const t = Math.min(...at);
    return p.t_s.reduce((best, s, k) => (Math.abs(s - t) < Math.abs(p.t_s[best]! - t) ? k : best), 0);
  };

  let i = outsideBy("A1");
  if (i >= 0) { const d = lines.dev[i]!; add("A1", "Speed", i, `${fixed(Math.abs(d))} kt ${d > 0 ? "fast" : "slow"}`); }
  i = outsideBy("A2");
  if (i >= 0 && lines.sink[i] != null) add("A2", "Sink", i, `${fixed(lines.sink[i]!)} fpm, the path needs ${fixed(lines.need[i] ?? 0)}`);
  i = outsideBy("A5G");
  if (i >= 0) { const d = p.gs_dot[i]!; add("A5G", "Glideslope", i, `${fixed(Math.abs(d), 1)} dot ${d > 0 ? "below" : "above"} the glideslope`); }
  i = outsideBy("A5L");
  if (i >= 0) { const d = p.loc_dot[i]!; add("A5L", "Localizer", i, `${fixed(Math.abs(d), 1)} dot ${d > 0 ? "right" : "left"} of course`); }
  if (halves(landing, "A6").some((c) => c.level >= 1)) {
    const inWindow = (k: number) => p.height_ft[k] != null;
    const gearDown = (k: number) => (lines.gear[k] ?? 100) >= 99;
    let k = p.t_s.findIndex((_, n) => n > 0 && inWindow(n) && (Math.abs((lines.flap[n] ?? 0) - (lines.flap[n - 1] ?? 0)) > 2 || gearDown(n) !== gearDown(n - 1)));
    if (k >= 0) add("A6", "Configuration", k, gearDown(k) === gearDown(k - 1) ? `flaps moved to ${fixed(lines.flap[k] ?? 0)}°` : "gear moved");
    else { k = p.t_s.length - 1; if (!gearDown(k)) add("A6", "Configuration", k, "gear not down"); }
  }
  return out.sort((a, b) => a.index - b.index);
}
