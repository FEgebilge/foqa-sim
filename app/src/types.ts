// The data contract of docs/debrief-api.md. Times are seconds of sim time since the start of the recording.

export type Level = "advisory" | "low" | "medium" | "high";

export type RecorderStatus =
  | { state: "waiting_for_sim" }
  | { state: "connected" }
  | { state: "recording"; flight_id: string; elapsed_s: number }
  | { state: "error"; message: string };

export interface Aircraft {
  icao?: string;
  title: string;
}

export interface FlightSummary {
  id: string;
  started_utc?: string;
  duration_s: number;
  aircraft?: Aircraft;
  departure?: string;
  arrival?: string;
  landings: number;
  worst_level?: Level;
  event_counts: Record<Level, number>;
}

export interface PhaseSpan {
  phase: string;
  start_s: number;
  end_s: number;
}

export interface RunwayFix {
  icao: string;
  runway: string;
  along_m: number;
  cross_m: number;
  lda_m: number;
  tdz_m: number;
  in_tdz: boolean;
  long_landing: boolean;
  short_landing: boolean;
}

export interface Touchdown {
  t_s: number;
  bounce: boolean;
  sink_fpm: number;
  sim_sink_fpm?: number;
  peak_g: number;
  pitch_deg?: number;
  roll_deg?: number;
  ias_kt?: number;
  runway?: RunwayFix;
}

export interface FlightEvent {
  rule: string;
  version: number;
  title: string;
  description: string;
  level: Level;
  start_s: number;
  end_s: number;
  /** Where `value` was measured; absent from older debriefs. */
  peak_s?: number;
  measure: string;
  value?: number;
  threshold?: number;
  unit?: string;
  /** Plain-language consequence and technique, from the rule. */
  why_it_matters?: string;
  advice?: string;
  /** Context at `peak_s`: IAS, flap angle and the profile's VFE there. */
  ias_kt?: number;
  flap_deg?: number;
  vfe_kt?: number;
}

export type SeriesValues = (number | null)[];

export interface Series {
  t_s: number[];
  alt_ft: SeriesValues;
  aal_ft: SeriesValues;
  ias_kt: SeriesValues;
  gs_kt: SeriesValues;
  vs_fpm: SeriesValues;
  nz_g: SeriesValues;
  pitch_deg: SeriesValues;
  roll_deg: SeriesValues;
  flap_deg: SeriesValues;
  /** The profile's flap-speed limit at `flap_deg`. */
  vfe_kt: SeriesValues;
  lat: SeriesValues;
  lon: SeriesValues;
}

export interface RunwayEnd {
  runway: string;
  lat: number;
  lon: number;
  heading_deg: number;
  lda_m: number;
  tdz_m: number;
}

export interface Airport {
  icao: string;
  runways: { width_m: number; ends: RunwayEnd[] }[];
}

export interface Debrief {
  flight: FlightSummary & { lfl_version: number; profile: string; class?: "light" | "transport"; damaged_at?: number };
  phases: PhaseSpan[];
  touchdowns: Touchdown[];
  events: FlightEvent[];
  series: Series;
  airports: Airport[];
}

export interface LandingNotice {
  flight_id: string;
  touchdown: Touchdown;
  events: FlightEvent[];
}

/** Payload of the `open-flight` event (a notification click asks the app to show a flight). */
export interface OpenFlight {
  flight_id: string;
}

/** Disk use of the flights folder, raw captures included. */
export interface StorageUsage {
  flights: number;
  bytes: number;
  folder: string;
}
