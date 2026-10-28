// The landing card a pilot shares (docs/exports.md): what it says, worked out from the debrief alone. Pure, so the Node
// tests check it; components/debrief/ShareCard.tsx draws it. The aircraft is named by its ICAO type only: never the sim
// title or tail number, never a file path.
import type { Debrief, Grade } from "../types";
import { landingAt } from "./assessment.ts";
import { flightDate, GRADE_LABEL, number } from "./format.ts";
import { groupLandings } from "./landings.ts";

/** The card in CSS pixels, the common link-preview size; drawn at twice that for sharp text. */
export const CARD = { width: 1200, height: 630, scale: 2 } as const;

export interface CardBar {
  label: string;
  /** Share of the domain kept, 0–1; null when the domain did not apply. */
  kept: number | null;
  /** Its worst item's level, 0 within tolerance to 3. */
  level: number;
  /** "−2.5" points, "0", or "n/a". */
  text: string;
}

export interface CardStat { label: string; value: string; sub?: string }

export interface CardData {
  /** "LTBL → LTBL". */
  route: string;
  /** "RWY 17", when the runway was matched. */
  runway: string | null;
  /** ICAO type, or "Unknown aircraft". */
  aircraft: string;
  date: string;
  /** "Landing 1 of 2", "Touch-and-go 1 of 2", "Landing". */
  landing: string;
  /** "77.4", null when the landing was not scored. */
  score: string | null;
  grade: Grade | null;
  gradeLabel: string | null;
  /** Why there is no score, when there is none. */
  note: string | null;
  /** The six domains in the method's order; empty when not scored. */
  bars: CardBar[];
  stats: CardStat[];
}

const side = (value: number, left: string, right: string) => (value < 0 ? left : value > 0 ? right : "");

/** The card of landing `index` (bounces belong to their landing); null for a flight without that landing. */
export function cardData(debrief: Debrief, index: number): CardData | null {
  const landings = groupLandings(debrief.touchdowns);
  const landing = landings[index];
  if (!landing) return null;
  const { flight } = debrief, td = landing.touchdown, fix = td.runway;
  const scored = landingAt(debrief, td.t_s);
  const ok = scored?.status === "scored" && scored.score !== null;
  const kind = scored && !scored.full_stop ? "Touch-and-go" : "Landing";
  const stats: CardStat[] = [
    { label: "Sink rate", value: `${number(td.sink_fpm)} fpm` },
    { label: "Peak load", value: `${number(td.peak_g, 2)} g` },
  ];
  if (fix) {
    stats.push({ label: "Touchdown", value: `${number(fix.along_m)} m`, sub: `past the threshold · ${fix.zone}` });
    const off = Math.abs(fix.cross_m);
    stats.push({ label: "Centreline", value: `${number(off, 1)} m ${side(fix.cross_m, "L", "R")}`.trim(), sub: off < 0.05 ? "on the line" : `${side(fix.cross_m, "left", "right")} of the line` });
  }
  if (td.wind) {
    const w = td.wind;
    const parts = [
      Math.round(Math.abs(w.headwind_kt)) ? `${number(Math.abs(w.headwind_kt))} kt ${w.headwind_kt < 0 ? "tail" : "head"}` : "",
      Math.round(Math.abs(w.crosswind_kt)) ? `${number(Math.abs(w.crosswind_kt))} kt from the ${side(w.crosswind_kt, "left", "right")}` : "",
    ].filter(Boolean);
    stats.push({ label: "Wind", value: `${String(Math.round(w.from_deg) % 360 || 360).padStart(3, "0")}° ${number(w.speed_kt)} kt`, sub: parts.join(" · ") || "calm on the runway" });
  }
  return {
    route: `${flight.departure ?? "----"} → ${fix?.icao ?? flight.arrival ?? "----"}`,
    runway: fix ? `RWY ${fix.runway}` : null,
    aircraft: flight.aircraft?.icao ?? "Unknown aircraft",
    date: flightDate(flight.started_utc),
    landing: landings.length > 1 ? `${kind} ${index + 1} of ${landings.length}` : kind,
    score: ok ? number(scored.score!, 1) : null,
    grade: ok ? scored.grade : null,
    gradeLabel: ok && scored.grade ? GRADE_LABEL[scored.grade] : null,
    note: ok ? null : scored?.note ?? "This landing was not scored.",
    bars: ok ? scored.domains.map((d) => ({
      label: d.short,
      kept: d.loss === null ? null : 1 - Math.min(1, Math.max(0, d.loss)),
      level: Math.min(3, Math.max(0, d.level)),
      text: d.loss === null ? "n/a" : d.points > 0 ? `−${number(d.points, 1)}` : "0",
    })) : [],
    stats,
  };
}

/** `foqa-sim-ltbl-17-2026-10-26-landing-1.png`: route, runway, date and landing, safe as a file name. */
export function cardFileName(debrief: Debrief, index: number) {
  const fix = groupLandings(debrief.touchdowns)[index]?.touchdown.runway;
  const parts = ["foqa-sim", fix?.icao ?? debrief.flight.arrival, fix?.runway, debrief.flight.started_utc?.slice(0, 10), `landing-${index + 1}`];
  return `${parts.filter(Boolean).join("-").toLowerCase().replace(/[^a-z0-9-]+/g, "")}.png`;
}
