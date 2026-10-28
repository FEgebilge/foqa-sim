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
  /** The headline, in plain words. */
  title: string;
  /** Cause and consequence in one sentence: "Bounce (level 2) holds this landing below Good (max 84.9)." */
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
  /** `value` as shown: "+12 kt", "1.48 g", "L2". */
  value_text: string;
  /** "above 500 ft" for a criterion graded on one half of the approach window; null for `all`. */
  height_text: string | null;
  unit: string | null;
  /** Continuous level 0-3. */
  ell: number;
  level: number;
  loss: number;
  bands: { above: Anchors; below: Anchors };
  /** Each anchor as shown, like `value_text`. */
  bands_text: { above: (string | null)[]; below: (string | null)[] };
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
  /** The title in a word or two, for small bars. */
  short: string;
  /** What the domain looks at, in one line. */
  summary: string;
  weight: number;
  /** Fraction of the weight lost; null: n/a, its weight shared out. */
  loss: number | null;
  /** Points it cost. */
  points: number;
  level: number;
  criteria: CriterionAssessment[];
  findings: FindingAssessment[];
  sets_by: string | null;
  /** The item that set the deduction, in words: "Descent rate 1,562 fpm above 300 ft" or the event's title. */
  set_by_text: string | null;
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
  /** Why an insufficient landing is not scored, in plain words. */
  note: string | null;
  /** Class, reference speeds and stable height, in plain words. */
  facts: string[];
  /** false: a touch-and-go, scored without its rollout, not setting the flight grade. */
  full_stop: boolean;
  caps: CapNote[];
  stable_from_ft: number | null;
  /** The approach window's split height for its class; null when insufficient. */
  split_ft: number | null;
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
  /** Which items were not graded for lack of data, in plain words; null when none. */
  partial_note?: string | null;
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
  /** One per landing, in time order (docs/trends.md); empty on summaries from before it existed. */
  landings_detail?: LandingFacts[];
  /** Each rule that fired at low or above: its worst level and how often. */
  rule_counts?: Record<string, { level: Level; count: number }>;
}

/** What Trends keeps of one landing (docs/trends.md). */
export interface LandingFacts {
  t_s: number;
  full_stop: boolean;
  icao?: string;
  runway?: string;
  /** landing-2, 0-100. */
  score?: number;
  grade?: Grade;
  confidence?: Confidence;
  /** Domain id to level 0-3; null: not applicable. */
  domains: Record<string, number | null>;
  sink_fpm: number;
  peak_g: number;
  /** Past the threshold; `cross_m` is positive right of the centreline. */
  along_m?: number;
  cross_m?: number;
  zone?: "In TDZ" | "Long" | "Short";
  /** The matched runway's touchdown zone length, aiming point and landing distance available. */
  tdz_m?: number;
  aiming_point_m?: number;
  lda_m?: number;
  crosswind_kt?: number;
  tailwind_kt?: number;
  bounces: number;
}

/** What the Trends screen filters by; absent fields mean all. */
export interface TrendFilter {
  aircraft?: string;
  airport?: string;
  days?: number;
}

export interface TrendLanding extends LandingFacts {
  flight_id: string;
  started_utc: string | null;
  aircraft: string | null;
}

export interface TrendDomain {
  id: string;
  title: string;
  short: string;
  graded: number;
  mean_level: number | null;
  level_2_plus: number;
}

export interface TrendEvent {
  rule: string;
  title: string;
  level: Level;
  flights: number;
  count: number;
  per_100: number;
  /** Null under 5 flights in either half. */
  recent_per_100: number | null;
  earlier_per_100: number | null;
}

export interface Trends {
  filter: TrendFilter;
  /** What the filters can be set to, from all flights. */
  options: { aircraft: { icao: string; flights: number }[]; airports: { icao: string; landings: number }[] };
  flights: number;
  hours: number;
  landing_count: number;
  scored: number;
  /** Scored full-stop landings; recent/earlier are null under 5 in each half. */
  score: { mean: number | null; median: number | null; recent: number | null; earlier: number | null };
  /** Oldest first: the chart and the footprint. */
  landings: TrendLanding[];
  /** In landing-2 order. */
  domains: TrendDomain[];
  /** Most frequent first, then by level. */
  events: TrendEvent[];
  by_aircraft: { icao: string; flights: number; landings: number; mean_score: number | null; events_per_flight: number }[];
  by_airport: { icao: string; landings: number; mean_score: number | null; in_tdz: number | null; mean_cross_m: number | null }[];
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
  /** Magnetic heading. */
  hdg_deg: SeriesValues;
  /** Radio height, on-ground bias removed; null above 2500 ft. */
  ra_ft: SeriesValues;
  flap_deg: SeriesValues;
  /** The profile's flap-speed limit at `flap_deg`. */
  vfe_kt: SeriesValues;
  /** `gear.extended`, 0 = up to 100 = down (%). */
  gear_ext: SeriesValues;
  /** 1 engaged, 0 not. */
  ap_engaged: SeriesValues;
  at_engaged: SeriesValues;
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
  slice_failed?: string;  // why its slice can never be cut (no_marker, no_flight): done, never uploaded
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
