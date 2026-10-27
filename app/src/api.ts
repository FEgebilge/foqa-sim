// The backend contract (docs/debrief-api.md). Inside Tauri it calls the Rust commands and listens to its events;
// in a plain browser it reads mock JSON from /mock so the UI can be developed without the app.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { mockSetup } from "./lib/setup-mock";
import type { Debrief, FlightSummary, LandingNotice, OpenFlight, RecorderStatus, SetupStatus, StorageUsage } from "./types";

export type Unlisten = () => void;

export interface BackendEvents {
  "recorder-status": RecorderStatus;
  "flight-saved": FlightSummary;
  landing: LandingNotice;
  "open-flight": OpenFlight;
}

export interface Api {
  readonly live: boolean;
  listFlights(): Promise<FlightSummary[]>;
  debrief(id: string): Promise<Debrief>;
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
  recorderStatus: () => invoke<RecorderStatus>("recorder_status"),
  openFlightsFolder: () => invoke<void>("open_flights_folder"),
  storageUsage: () => invoke<StorageUsage>("storage_usage"),
  setupStatus: () => invoke<SetupStatus>("setup_status"),
  setupInstall: (pkg) => invoke<SetupStatus>("setup_install", { package: pkg }),
  setupRemove: (pkg) => invoke<SetupStatus>("setup_remove", { package: pkg }),
  setupUseCommunity: (path) => invoke<SetupStatus>("setup_use_community", { path }),
  setupPickCommunity: () => invoke<SetupStatus>("setup_pick_community"),
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

const mockApi: Api = {
  live: false,
  async listFlights() {
    const flights = await firstJson<FlightSummary[]>(mockFiles("flights.json"));
    if (!(many > flights.length)) return flights;
    return Array.from({ length: many }, (_, index) => {
      const base = flights[index % flights.length]!;
      if (index < flights.length) return base;
      const started = base.started_utc ? new Date(Date.parse(base.started_utc) - index * 86_400_000 * 0.7).toISOString() : undefined;
      return { ...base, id: `${base.id}${COPY}${index}`, started_utc: started };
    });
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
  on: async () => () => {},
};

export const api: Api = isTauri() ? tauriApi : mockApi;
