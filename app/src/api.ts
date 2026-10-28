// The backend contract (docs/debrief-api.md). Inside Tauri it calls the Rust commands and listens to its events;
// in a plain browser it reads mock JSON from /mock (api-mock.ts) so the UI can be developed without the app.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DeepLink, LinkOpened, SiteAttempt, SiteChallenge, SiteSettings, SitesApi, SiteView } from "./lib/sites";
import type { AppUpdate, AttemptRecord, AttemptView, Briefing, ChallengeEntry, Debrief, FlightSummary, LandingNotice, OpenFlight, PreflightLine, RecorderStatus, SetupStatus, StorageUsage, TrendFilter, Trends } from "./types";

export type Unlisten = () => void;

/** What the pilot wrote about a flight: a title, free notes and tags; saved beside the recording. */
export interface FlightMeta { title?: string; notes?: string; tags: string[] }

/** The flight exports of `fdm export` (docs/exports.md). */
export type ExportFormat = "csv" | "kml" | "gpx" | "parquet";

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
  /** One pilot's flights over time (docs/trends.md); an absent filter field means all. */
  trends(filter: TrendFilter): Promise<Trends>;
  /** The pilot's title, notes and tags of a flight; empty for a flight never written about. */
  flightMetaGet(id: string): Promise<FlightMeta>;
  /** Replaces the flight's title, notes and tags. */
  flightMetaSet(id: string, meta: FlightMeta): Promise<void>;
  /** Removes the flight's recording files for good; the flight being recorded is refused. */
  deleteFlight(id: string): Promise<void>;
  recorderStatus(): Promise<RecorderStatus>;
  openFlightsFolder(): Promise<void>;
  /** Opens the folder holding the app's log file, for a recorder error. */
  openLogFolder(): Promise<void>;
  storageUsage(): Promise<StorageUsage>;
  setupStatus(): Promise<SetupStatus>;
  setupInstall(pkg: string): Promise<SetupStatus>;
  setupRemove(pkg: string): Promise<SetupStatus>;
  /** null goes back to the Community folder of the install found. */
  setupUseCommunity(path: string | null): Promise<SetupStatus>;
  setupPickCommunity(): Promise<SetupStatus>;
  /** Saves the flight in a format other tools read, where the user picks; the file, null when cancelled. `identity` adds the aircraft's title and tail number. */
  exportFlight(id: string, format: ExportFormat, identity: boolean): Promise<string | null>;
  /** Saves the flight list as a Little Navmap logbook CSV; the file, null when cancelled. */
  exportLogbook(identity: boolean): Promise<string | null>;
  /** Asks the release feed for a newer version; null when this is the newest. Rejects when the feed cannot be reached. */
  checkUpdate(): Promise<AppUpdate | null>;
  /** Downloads and installs the update `checkUpdate` found, reporting bytes done and the total when known. The installer closes the app and starts it again. */
  installUpdate(onProgress: (done: number, total?: number, finished?: boolean) => void): Promise<void>;
  /** Saves a PNG the window drew (the landing card); the file, null when cancelled. */
  saveImage(png: Blob, name: string): Promise<string | null>;
  on<K extends keyof BackendEvents>(event: K, handler: (payload: BackendEvents[K]) => void): Promise<Unlisten>;
}

/** The update `checkUpdate` found, for `installUpdate`. */
let found: import("@tauri-apps/plugin-updater").Update | null = null;

const tauriApi: Api = {
  live: true,
  listFlights: () => invoke<FlightSummary[]>("list_flights"),
  debrief: (id) => invoke<Debrief>("debrief", { id }),
  trends: (filter) => invoke<Trends>("trends", { filter }),
  deleteFlight: (id) => invoke<void>("delete_flight", { id }),
  flightMetaGet: (id) => invoke<FlightMeta>("flight_meta_get", { id }),
  flightMetaSet: (id, meta) => invoke<void>("flight_meta_set", { id, meta }),
  recorderStatus: () => invoke<RecorderStatus>("recorder_status"),
  openFlightsFolder: () => invoke<void>("open_flights_folder"),
  openLogFolder: () => invoke<void>("open_log_folder"),
  storageUsage: () => invoke<StorageUsage>("storage_usage"),
  setupStatus: () => invoke<SetupStatus>("setup_status"),
  setupInstall: (pkg) => invoke<SetupStatus>("setup_install", { package: pkg }),
  setupRemove: (pkg) => invoke<SetupStatus>("setup_remove", { package: pkg }),
  setupUseCommunity: (path) => invoke<SetupStatus>("setup_use_community", { path }),
  setupPickCommunity: () => invoke<SetupStatus>("setup_pick_community"),
  exportFlight: (id, format, identity) => invoke<string | null>("export_flight", { id, format, identity }),
  exportLogbook: (identity) => invoke<string | null>("export_logbook", { identity }),
  saveImage: async (png, name) => invoke<string | null>("save_image", { name, png: Array.from(new Uint8Array(await png.arrayBuffer())) }),
  async checkUpdate() {
    const { check } = await import("@tauri-apps/plugin-updater");
    found = await check();
    return found && { version: found.version, notes: found.body ?? "" };
  },
  async installUpdate(onProgress) {
    if (!found) throw new Error("Check for updates first.");
    let done = 0;
    let total: number | undefined;
    // The installer restarts the app hidden in the tray; the mark makes that start show its window.
    await invoke<void>("update_mark");
    try {
      await found.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength;
        else if (event.event === "Progress") done += event.data.chunkLength;
        onProgress(done, total, event.event === "Finished");
      });
    } catch (e) {
      // A failed update must not leave the mark to raise the window at a later start.
      await invoke<void>("update_mark_clear").catch(() => {});
      throw e;
    }
  },
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

/** The browser mock, loaded on the first call: only a plain browser ever needs it. */
const lazyMock = (): Api =>
  new Proxy({ live: false } as Api, {
    get: (target, key: keyof Api) =>
      key in target ? target[key] : async (...args: unknown[]) => {
        const { mockApi } = await import("./api-mock");
        return (mockApi[key] as (...a: unknown[]) => unknown)(...args);
      },
  });

export const api: Api = isTauri() ? tauriApi : lazyMock();
