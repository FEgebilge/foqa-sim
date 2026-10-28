import type { Debrief, FlightEvent, LandingAssessment, SeriesValues } from "../types";

// Reading the debrief series at any time: what the replay's instruments, the map dot and the cursor readout show.
// Pure, so the Node tests can run it. A value is only ever interpolated between two recorded neighbours; a gap stays a gap.

type Series = Debrief["series"];
export type Position = [lon: number, lat: number];

/** Index of the last sample at or before t (binary search over a sorted time base). */
export function indexAt(times: number[], t: number) {
  let lo = 0, hi = times.length - 1;
  if (hi < 0 || t <= times[0]!) return 0;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (times[mid]! <= t) lo = mid; else hi = mid - 1;
  }
  return lo;
}

/**
 * How a parameter reads between samples: `linear`; `angle`, linear the short way round 360°;
 * `hold`, the last sample's value (discretes and settings, which step).
 */
export type Interp = "linear" | "angle" | "hold";

/** The value at t: interpolated between the samples around it, `null` outside the recording or next to a missing sample. */
export function valueAt(times: number[], values: SeriesValues, t: number, how: Interp = "linear"): number | null {
  const last = times.length - 1;
  if (last < 0 || t < times[0]! || t > times[last]!) return null;
  const i = indexAt(times, t);
  const a = values[i] ?? null;
  if (how === "hold" || t === times[i]) return a;
  const b = values[i + 1] ?? null;
  if (a === null || b === null) return null;
  const f = (t - times[i]!) / (times[i + 1]! - times[i]!);
  if (how === "linear") return a + (b - a) * f;
  const turn = ((((b - a) % 360) + 540) % 360) - 180;
  return (((a + turn * f) % 360) + 360) % 360;
}

/** Whether any sample has a position: a flight without one has no map to draw. */
export const hasPosition = (series: Series) => series.lat.some((v) => v !== null);

/** Position at t, interpolated like any other parameter. */
export function positionAt(series: Series, t: number): Position | null {
  const lat = valueAt(series.t_s, series.lat, t), lon = valueAt(series.t_s, series.lon, t);
  return lat === null || lon === null ? null : [lon, lat];
}

/** Initial bearing from a to b, degrees true, -180 to 180. */
export function bearing([lo1, la1]: Position, [lo2, la2]: Position) {
  const r = Math.PI / 180, dl = (lo2 - lo1) * r;
  return Math.atan2(Math.sin(dl) * Math.cos(la2 * r), Math.cos(la1 * r) * Math.sin(la2 * r) - Math.sin(la1 * r) * Math.cos(la2 * r) * Math.cos(dl)) / r;
}

/** Track over the ground at t, degrees true, from the positions 2 s either side; `null` under 20 m apart (stopped or slow). */
export function trackAt(series: Series, t: number): number | null {
  const a = positionAt(series, t - 2), b = positionAt(series, t + 2);
  if (!a || !b) return null;
  const metres = Math.hypot((b[0] - a[0]) * 111_320 * Math.cos((a[1] * Math.PI) / 180), (b[1] - a[1]) * 111_320);
  return metres < 20 ? null : (bearing(a, b) + 360) % 360;
}

/** Up, down, or moving between them with its share extended (0-1). */
export type Gear = { state: "up" | "down" } | { state: "transit"; extended: number };

/** The gear at t from `gear_ext`, 0-100 %. */
export function gearAt(series: Series, t: number): Gear | null {
  const v = valueAt(series.t_s, series.gear_ext, t, "hold");
  if (v === null) return null;
  const extended = Math.min(1, Math.max(0, v / 100));
  return extended >= 0.99 ? { state: "down" } : extended <= 0.01 ? { state: "up" } : { state: "transit", extended };
}

/** The approach speeds landing-2 graded the landing at or after t with (30 s past its touchdown still counts as it). */
export function approachSpeedsAt(landings: LandingAssessment[], t: number) {
  const index = landings.findIndex((l) => l.t_s + 30 >= t);
  const speeds = index < 0 ? null : landings[index]!.speeds;
  return speeds ? { ...speeds, landing: index } : null;
}

/** What each instrument value on the replay PFD is built from, when it is not a recorded parameter read as is. */
export const DERIVED = {
  vs: "Derived: inertial vertical speed, from pressure altitude blended with normal load (not recorded).",
  trk: "Derived: track over the ground from the GPS positions 2 s either side, degrees true (not recorded).",
  ra: "Radio height as recorded, with its on-ground bias removed; shown below 2500 ft only.",
} as const;

/** Everything the replay PFD draws at time t; `null` where the parameter has no data there. */
export interface Instruments {
  t: number;
  pitch: number | null;
  roll: number | null;
  ias: number | null;
  /** Pressure altitude. */
  alt: number | null;
  /** Derived (`DERIVED.vs`). */
  vs: number | null;
  ra: number | null;
  /** Magnetic. */
  hdg: number | null;
  /** True, derived (`DERIVED.trk`). */
  trk: number | null;
  flap: number | null;
  gear: Gear | null;
  ap: boolean | null;
  at: boolean | null;
  /** The type profile's flap limit at the current flaps; `null` with the flaps up or no profile band. */
  vfe: number | null;
  /** The type profile's maximum (VMO, or VNE for a light type). */
  vmo: number | null;
  /** Reference speeds of the landing ahead, as landing-2 graded it. */
  speeds: ReturnType<typeof approachSpeedsAt>;
}

const flag = (v: number | null) => (v === null ? null : v >= 0.5);

/** The PFD at t. */
export function instrumentsAt(debrief: Debrief, t: number): Instruments {
  const s = debrief.series;
  const at = (values: SeriesValues, how?: Interp) => valueAt(s.t_s, values, t, how);
  const ra = at(s.ra_ft);
  return {
    t,
    pitch: at(s.pitch_deg),
    roll: at(s.roll_deg),
    ias: at(s.ias_kt),
    alt: at(s.alt_ft),
    vs: at(s.vs_fpm),
    ra: ra !== null && ra <= 2500 ? ra : null,
    hdg: at(s.hdg_deg, "angle"),
    trk: trackAt(s, t),
    flap: at(s.flap_deg),
    gear: gearAt(s, t),
    ap: flag(at(s.ap_engaged, "hold")),
    at: flag(at(s.at_engaged, "hold")),
    vfe: at(s.vfe_kt, "hold"),
    vmo: debrief.flight.profile_limits?.vmo?.value ?? null,
    speeds: approachSpeedsAt(debrief.flight.assessment_detail.landings ?? [], t),
  };
}

/** The event after (`1`) or before (`-1`) time t by its peak, where selecting it puts the cursor; `null` at the end. */
export function adjacentEvent(events: FlightEvent[], t: number, direction: 1 | -1): number | null {
  const order = events.map((e, i) => ({ i, at: e.peak_s ?? e.start_s })).sort((a, b) => a.at - b.at);
  const hit = direction > 0 ? order.find((e) => e.at > t + 0.05) : order.findLast((e) => e.at < t - 0.05);
  return hit?.i ?? null;
}
