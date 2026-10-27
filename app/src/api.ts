// The backend contract (docs/debrief-api.md). Inside Tauri it calls the Rust commands and listens to its events;
// in a plain browser it reads mock JSON from /mock so the UI can be developed without the app.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { mockChallenges } from "./lib/challenges-mock";
import { mockSetup } from "./lib/setup-mock";
import { mockSites } from "./lib/sites-mock";
import type { DeepLink, LinkOpened, SiteAttempt, SiteChallenge, SiteSettings, SitesApi, SiteView } from "./lib/sites";
import type { AttemptRecord, AttemptView, Briefing, ChallengeEntry, Debrief, FlightSummary, LandingNotice, OpenFlight, PreflightLine, RecorderStatus, SetupStatus, StorageUsage } from "./types";

export type Unlisten = () => void;

export interface BackendEvents {
  "recorder-status": RecorderStatus;
  "flight-saved": FlightSummary;
  landing: LandingNotice;
  "open-flight": OpenFlight;
  "attempt-changed": AttemptRecord;
  /** A site or an attempt's site part changed (connect builds only). */
  "site-changed": { origin?: string; attempt?: string };
  /** A link from a website arrived; the window is brought up. */
  "deep-link": null;
  /** A click on a site's verdict notification asks the app to show that challenge. */
  "open-challenge": { key: string };
}

/** Landing challenges, local only: the library, the pre-flight card and the attempts. */
export interface ChallengesApi {
  challengesList(): Promise<ChallengeEntry[]>;
  /** Asks for a challenge file; null when the dialog is cancelled. Errors are in plain words. */
  challengeImport(): Promise<ChallengeEntry | null>;
  challengeRemove(key: string): Promise<void>;
  challengeBriefing(key: string): Promise<Briefing>;
  challengePreflight(key: string): Promise<PreflightLine[]>;
  /** Creates an attempt and places the aircraft at the start. */
  attemptStart(key: string): Promise<AttemptRecord>;
  /** Abandons the attempt if still open and starts the same definition again. */
  attemptFlyAgain(id: string): Promise<AttemptRecord>;
  /** Ends the attempt and grades it from its flight. */
  attemptFinish(id: string): Promise<AttemptRecord>;
  attemptAbandon(id: string): Promise<AttemptRecord>;
  attemptsList(): Promise<AttemptView[]>;
}

export interface Api extends ChallengesApi, SitesApi {
  readonly live: boolean;
  listFlights(): Promise<FlightSummary[]>;
  debrief(id: string): Promise<Debrief>;
  /** Removes the flight's recording files for good; the flight being recorded is refused. */
  deleteFlight(id: string): Promise<void>;
  recorderStatus(): Promise<RecorderStatus>;
  openFlightsFolder(): Promise<void>;
  storageUsage(): Promise<StorageUsage>;
  setupStatus(): Promise<SetupStatus>;
  setupInstall(pkg: string): Promise<SetupStatus>;
  setupRemove(pkg: string): Promise<SetupStatus>;
  /** null goes back to the Community folder of the install found. */
  setupUseCommunity(path: string | null): Promise<SetupStatus>;
  setupPickCommunity(): Promise<SetupStatus>;
  on<K extends keyof BackendEvents>(event: K, handler: (payload: BackendEvents[K]) => void): Promise<Unlisten>;
}

const tauriApi: Api = {
  live: true,
  listFlights: () => invoke<FlightSummary[]>("list_flights"),
  debrief: (id) => invoke<Debrief>("debrief", { id }),
  deleteFlight: (id) => invoke<void>("delete_flight", { id }),
  recorderStatus: () => invoke<RecorderStatus>("recorder_status"),
  openFlightsFolder: () => invoke<void>("open_flights_folder"),
  storageUsage: () => invoke<StorageUsage>("storage_usage"),
  setupStatus: () => invoke<SetupStatus>("setup_status"),
  setupInstall: (pkg) => invoke<SetupStatus>("setup_install", { package: pkg }),
  setupRemove: (pkg) => invoke<SetupStatus>("setup_remove", { package: pkg }),
  setupUseCommunity: (path) => invoke<SetupStatus>("setup_use_community", { path }),
  setupPickCommunity: () => invoke<SetupStatus>("setup_pick_community"),
  challengesList: () => invoke<ChallengeEntry[]>("challenges_list"),
  challengeImport: () => invoke<ChallengeEntry | null>("challenge_import"),
  challengeRemove: (key) => invoke<void>("challenge_remove", { key }),
  challengeBriefing: (key) => invoke<Briefing>("challenge_briefing", { key }),
  challengePreflight: (key) => invoke<PreflightLine[]>("challenge_preflight", { key }),
  attemptStart: (key) => invoke<AttemptRecord>("attempt_start", { key }),
  attemptFlyAgain: (id) => invoke<AttemptRecord>("attempt_fly_again", { id }),
  attemptFinish: (id) => invoke<AttemptRecord>("attempt_finish", { id }),
  attemptAbandon: (id) => invoke<AttemptRecord>("attempt_abandon", { id }),
  attemptsList: () => invoke<AttemptView[]>("attempts_list"),
  connectAvailable: () => invoke<boolean>("connect_available").catch(() => false),
  sitesList: () => invoke<SiteView[]>("sites_list"),
  siteAdd: (origin) => invoke<SiteView>("site_add", { origin }),
  siteSignIn: (origin, device) => invoke<SiteView>("site_sign_in", { origin, device }),
  siteSignInCancel: (origin) => invoke<void>("site_sign_in_cancel", { origin }),
  siteSignOut: (origin) => invoke<SiteView>("site_sign_out", { origin }),
  siteRemove: (origin) => invoke<void>("site_remove", { origin }),
  siteChallenges: (origin) => invoke<SiteChallenge[]>("site_challenges", { origin }),
  siteChallengeTake: (origin, id) => invoke<ChallengeEntry>("site_challenge_take", { origin, id }),
  siteAttempts: () => invoke<SiteAttempt[]>("site_attempts"),
  attemptUpload: (id) => invoke<void>("attempt_upload", { id }),
  siteOpen: (url) => invoke<void>("site_open", { url }),
  deepLinkPending: () => invoke<DeepLink | null>("deep_link_pending").catch(() => null),
  deepLinkConsent: (seq, allow) => invoke<SiteView | null>("deep_link_consent", { seq, allow }),
  deepLinkOpen: (seq, replace) => invoke<LinkOpened>("deep_link_open", { seq, replace }),
  deepLinkDone: (seq) => invoke<void>("deep_link_done", { seq }).catch(() => {}),
  siteSettings: () => invoke<SiteSettings>("site_settings"),
  siteSettingsSet: (settings) => invoke<SiteSettings>("site_settings_set", { settings }),
  on<K extends keyof BackendEvents>(event: K, handler: (payload: BackendEvents[K]) => void) {
    return listen<BackendEvents[K]>(event, (message) => handler(message.payload));
  },
};

/** First JSON file that exists. Vite answers a missing file with index.html, so the content type decides. */
async function firstJson<T>(paths: string[]): Promise<T> {
  for (const path of paths) {
    const response = await fetch(path);
    if (response.ok && response.headers.get("content-type")?.includes("json")) return (await response.json()) as T;
  }
  throw new Error(`No mock data at ${paths.join(" or ")}`);
}

const params = new URLSearchParams(window.location.search);
// Real backend output wins over the synthetic sample (scripts/make-sample-mock.mjs); `?samples` shows only the sample.
const mockFiles = (name: string) => (params.has("samples") ? [`/mock/sample-${name}`] : [`/mock/${name}`, `/mock/sample-${name}`]);
/** `?many=150` repeats the mock list to that length, to try search and sort at scale. Copies open their original's debrief. */
const many = Number(params.get("many") ?? 0);
const COPY = "~";
/** Mock flights "deleted" in this page session. */
const deleted = new Set<string>();

const mockApi: Api = {
  live: false,
  async listFlights() {
    const flights = await firstJson<FlightSummary[]>(mockFiles("flights.json"));
    if (!(many > flights.length)) return flights.filter((f) => !deleted.has(f.id));
    return Array.from({ length: many }, (_, index) => {
      const base = flights[index % flights.length]!;
      if (index < flights.length) return base;
      const started = base.started_utc ? new Date(Date.parse(base.started_utc) - index * 86_400_000 * 0.7).toISOString() : undefined;
      return { ...base, id: `${base.id}${COPY}${index}`, started_utc: started };
    }).filter((f) => !deleted.has(f.id));
  },
  async deleteFlight(id) {
    deleted.add(id);
  },
  debrief: (id) => firstJson<Debrief>(mockFiles(`debrief-${id.split(COPY)[0]}.json`)),
  async recorderStatus() {
    const state = params.get("status");
    if (state === "recording") return { state, flight_id: "flight-mock", elapsed_s: 754 };
    if (state === "connected") return { state };
    if (state === "error") return { state, message: "SimConnect closed the connection" };
    return { state: "waiting_for_sim" };
  },
  async openFlightsFolder() {
    throw new Error("Opening the flights folder needs the desktop app.");
  },
  async storageUsage() {
    const flights = await firstJson<FlightSummary[]>(mockFiles("flights.json"));
    return { flights: flights.length, bytes: flights.length * 1_800_000, folder: "Documents\\FOQA-Sim\\flights" };
  },
  ...mockSetup(params.get("setup")),
  ...mockChallenges(params.get("status")),
  ...mockSites(params.get("sites")),
  on: async () => () => {},
};

export const api: Api = isTauri() ? tauriApi : mockApi;
