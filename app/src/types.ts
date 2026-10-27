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

export type Grade = "good" | "fair" | "poor";

/**
 * The flight graded as a whole: by landing-2 (docs/scoring.md) when a landing was scored (`score` 0-100), else by the
 * points method. `points` and `findings` are the points method's (each rule broken counts once at its worst level: high 4,
 * medium 2, low 1, advisory 0), kept for one release; older summaries have no `method`.
 */
export interface Assessment {
  grade: Grade;
  points: number;
  /** Rules broken at low or above. */
  findings: number;
  method?: "landing-2" | "points";
  score?: number;
  confidence?: Confidence;
}

export type Confidence = "full" | "partial";


/** One rule's share of the assessment: its worst level in the flight alone counts. */
export interface AssessmentPart {
  rule: string;
  title: string;
  level: Level;
  points: number;
  /** How many times it fired. */
  count: number;
}

/** A grade's reach on the points scale: up to `up_to` inclusive, the last (no `up_to`) any more. */
export interface GradeBand {
  grade: Grade;
  up_to?: number;
}

export type Domain = "stability" | "energy" | "point" | "lateral" | "contact" | "rollout";
export type ScoringClass = "light" | "turboprop" | "jet";

/** A safety cap on a landing, or (`kind: "flight"`) why the flight's score is below its grading landing's. */
export interface CapNote {
  kind: "material" | "unstable" | "inspection" | "undershoot" | "excursion" | "flight";
  score: number;
  reason: string;
  source: string;
  /** Index in `AssessmentDetail.landings`. */
  landing?: number;
}

/** [ideal, tolerance, L1, L2, L3] at levels 0, 0.5, 1, 2, 3; `above` grades larger values, `below` smaller; null: absent. */
export type Anchors = [number | null, number | null, number | null, number | null, number | null];

export interface CriterionAssessment {
  id: string;
  title: string;
  /** The half of the approach window it was graded in. */
  height: "all" | "above" | "below";
  /** The worst value in `unit` (none: a level). */
  value: number;
  unit: string | null;
  /** Continuous level 0-3. */
  ell: number;
  level: number;
  loss: number;
  bands: { above: Anchors; below: Anchors };
  standard: string;
  /** It set its domain's deduction. */
  sets: boolean;
}

export interface FindingAssessment {
  /** Index in `Debrief.events`. */
  event: number | null;
  rule: string;
  level: Level;
  ell: number;
  loss: number;
  sets: boolean;
}

export interface DomainAssessment {
  id: Domain;
  title: string;
  weight: number;
  /** Fraction of the weight lost; null: n/a, its weight shared out. */
  loss: number | null;
  /** Points it cost. */
  points: number;
  level: number;
  criteria: CriterionAssessment[];
  findings: FindingAssessment[];
  sets_by: string | null;
}

export interface LandingAssessment {
  t_s: number;
  status: "scored" | "insufficient";
  score: number | null;
  raw: number | null;
  grade: Grade | null;
  confidence: Confidence | null;
  class: ScoringClass | null;
  partial: string[];
  missing: string[];
  /** false: a touch-and-go, scored without its rollout, not setting the flight grade. */
  full_stop: boolean;
  caps: CapNote[];
  stable_from_ft: number | null;
  speeds: { vref_kt: number; vapp_kt: number; source: "profile" | "selected"; approximate: boolean; basis: string } | null;
  domains: DomainAssessment[];
}

/**
 * How the assessment was reached (docs/scoring.md §7.5): each landing in six domains, the caps and the rest of the flight.
 * `method: "points"` (no scored landing, or an older backend without `landings`) grades by the parts and bands.
 */
export interface AssessmentDetail {
  method?: "landing-2" | "points";
  score?: number | null;
  grade?: Grade;
  confidence?: Confidence | null;
  caps?: CapNote[];
  graded_by?: number | null;
  landings?: LandingAssessment[];
  /** Indices in `Debrief.events` outside every landing. */
  other_findings?: number[];
  legacy_points?: number;
  legacy_findings?: number;
  parts: AssessmentPart[];
  bands: GradeBand[];
  grades?: { good: number; fair: number };
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
  assessment: Assessment;
}

export interface PhaseSpan {
  phase: string;
  start_s: number;
  end_s: number;
  /** What opened it, with the value there, e.g. "descending through 1000 ft above the aerodrome (998 ft AAL)". */
  trigger: string;
}

export interface RunwayFix {
  icao: string;
  runway: string;
  along_m: number;
  cross_m: number;
  lda_m: number;
  tdz_m: number;
  aiming_point_m: number;
  in_tdz: boolean;
  long_landing: boolean;
  short_landing: boolean;
  /** "In TDZ", "Long" or "Short". */
  zone: string;
}

/** Along the runway (negative headwind = tailwind) and across it (positive = from the right). */
export interface TouchdownWind { from_deg: number; speed_kt: number; headwind_kt: number; crosswind_kt: number }

export interface Touchdown {
  t_s: number;
  bounce: boolean;
  sink_fpm: number;
  sim_sink_fpm?: number;
  peak_g: number;
  pitch_deg?: number;
  roll_deg?: number;
  ias_kt?: number;
  /** Wind at contact on the runway; absent without a runway fix or wind data. */
  wind?: TouchdownWind;
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
  /** The event in words, e.g. "111 kt at flaps 12°, limit 85 kt". */
  detail?: string;
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

/** A type-profile value with the document it comes from. */
export interface Sourced {
  value: number;
  source: string;
  /** The closest value found, not the type's own published one; `note` says how. */
  approximate?: boolean;
  note?: string;
}

export interface Debrief {
  flight: FlightSummary & {
    lfl_version: number;
    profile: string;
    class?: "light" | "transport";
    /** The profile's values by rule name, e.g. `hard_landing_g`, `tail_landing_pitch`. */
    profile_limits?: Record<string, Sourced>;
    /**
     * The phase boundaries in effect (docs/phases.md), e.g. `flare_ft`, `approach_ft`; `source` is the profile and
     * its document ("PC12 profile, FAA AIM 4-3-3 …") or the class default ("light class default").
     */
    phase_values: Record<string, Sourced>;
    damaged_at?: number;
    assessment_detail: AssessmentDetail;
  };
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

export type CheckState = "pass" | "warn" | "fail";

export interface SetupFix {
  action: "install" | "remove" | "pick_community";
  package?: string;
  label: string;
}

export interface SetupCheck {
  id: string;
  title: string;
  state: CheckState;
  detail: string;
  paths?: string[];
  files?: string[];
  fixes?: SetupFix[];
}

export interface SimInstall {
  store: "ms_store" | "steam";
  user_cfg: string;
  community: string;
}

export interface SetupStatus {
  checks: SetupCheck[];
  sims: SimInstall[];
  community: string | null;
  choice: "found" | "install" | "folder";
}

// Landing challenges (docs/challenges.md) and their local attempts (docs/challenge-protocol.md B5, B8).

/** A challenge file kept in the local library. */
export interface ChallengeEntry {
  key: string;            // "<id>-v<version>"
  id: string;
  version: number;
  title: string;
  airport: string;
  runway: string;
  sha256: string;         // of the stored definition bytes
}

type Band = { min?: number; max?: number };

/** The challenge JSON as written (docs/challenges.md). */
export interface ChallengeDefinition {
  id: string;
  version: number;
  title: string;
  window?: { from: string; to: string };
  airport: string;
  runway: string;
  aircraft?: { classes?: ("light" | "transport")[]; types?: string[] };
  start: { distance_nm: number; height_ft: number; ias_kt: number; gear_down?: boolean; flaps?: number; mode: "teleport" | "self" };
  gate: { distance_nm: number };
  conditions?: {
    wind?: { from_deg: number; speed_kt: number; tolerance_deg: number; tolerance_kt: number };
    crosswind_kt?: Band;
    tailwind_kt?: Band;
    preset?: string;
  };
  rules?: { ap_off_by_height_ft?: number; stop_on_runway?: boolean };
  scoring: { version: string };
}

export interface Briefing extends ChallengeEntry {
  definition: ChallengeDefinition;
}

export interface PreflightLine {
  name: "sim" | "aircraft" | "weather" | "ranking";
  state: "ok" | "warn" | "fail" | "unknown";
  text: string;
  fix?: string;
}

export type AttemptState = "created" | "placing" | "failed_start" | "flying" | "approach" | "rollout" | "finished" | "graded_local" | "sliced" | "done" | "abandoned";

/** `attempt.json`. */
export interface AttemptRecord {
  id: string;
  key: string;
  challenge: string;
  version: number;
  title: string;
  definition_sha256: string;
  n: number;              // attempt number on this challenge on this PC
  state: AttemptState;
  created_at: string;
  updated_at: string;
  flight_id: string | null;
  ref?: string | null;    // the attempt marker's reference (A7); null for a local attempt
  begin_s: number | null;
  touchdown_s: number | null;
  reason: string | null;  // why it finished or was abandoned
}

/** The local grade (`challenge::Attempt`). */
export interface AttemptResult {
  challenge: string;
  challenge_version: number;
  aircraft?: { icao: string | null; class: string | null };
  marker?: { ref: string | null; begin_s: number; end_s: number | null } | null;
  valid: boolean;
  checks: { name: string; ok: boolean; detail: string }[];
  gate_s: number | null;
  touchdown_s: number | null;
  score: { version: string; total: number; parts: { name: string; weight: number; score: number; value: number | null; unit: string }[] } | null;
}

/** One line of `log.jsonl`. */
export interface AttemptLogLine {
  at: string;
  seq: number;
  event: string;
  [field: string]: unknown;
}

export interface AttemptView extends AttemptRecord {
  result: AttemptResult | null;
  log: AttemptLogLine[];
}
