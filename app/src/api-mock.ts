// The mock backend for a plain browser: reads JSON from /mock so the UI can be developed without the app.
// Loaded on first use (api.ts), so the desktop build never runs it.
import { mockChallenges } from "./lib/challenges-mock";
import { mockSetup } from "./lib/setup-mock";
import { mockSites } from "./lib/sites-mock";
import type { Api } from "./api";
import type { Debrief, FlightSummary, Trends } from "./types";

/** First JSON file that exists. Vite answers a missing file with index.html, so the content type decides. */
async function firstJson<T>(paths: string[]): Promise<T> {
  for (const path of paths) {
    const response = await fetch(path);
    if (response.ok && response.headers.get("content-type")?.includes("json")) return (await response.json()) as T;
  }
  throw new Error(`No mock data at ${paths.join(" or ")}`);
}

/** Hands a file to the browser's downloads: the mock's stand-in for a save dialog. */
function download(blob: Blob, name: string) {
  const url = URL.createObjectURL(blob);
  const link = Object.assign(document.createElement("a"), { href: url, download: name });
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  return name;
}

/** A mock export made by `fdm` (public/mock/export); Vite answers a missing one with index.html. */
async function mockExport(name: string) {
  const response = await fetch(`/mock/export/${name}`);
  if (!response.ok || response.headers.get("content-type")?.includes("html")) {
    throw new Error(`The mock has ${name.endsWith(".parquet") ? "no Parquet export" : "exports of c172-pattern only"}; the desktop app exports every flight.`);
  }
  return download(await response.blob(), name);
}

const params = new URLSearchParams(window.location.search);
// Real backend output wins over the synthetic sample (scripts/make-sample-mock.mjs); `?samples` shows only the sample.
const mockFiles = (name: string) => (params.has("samples") ? [`/mock/sample-${name}`] : [`/mock/${name}`, `/mock/sample-${name}`]);
/** `?many=150` repeats the mock list to that length, to try search and sort at scale. Copies open their original's debrief. */
const many = Number(params.get("many") ?? 0);
const COPY = "~";
/** Mock flights "deleted" in this page session. */
const deleted = new Set<string>();

export const mockApi: Api = {
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
  /** The sample, narrowed to the filter: its landings, counts and mean score follow; the other tables stay as sampled. */
  async trends({ aircraft, airport, days }) {
    const all = await firstJson<Trends>(["/mock/trends.json"]);
    const newest = Date.parse(all.landings.at(-1)?.started_utc ?? "") || 0;
    const rows = all.landings.filter((l) => (!aircraft || l.aircraft === aircraft) && (!airport || l.icao === airport)
      && (days === undefined || Date.parse(l.started_utc ?? "") >= newest - days * 86_400_000));
    const scores = rows.filter((l) => l.score !== undefined && l.full_stop).map((l) => l.score!);
    const mean = scores.length ? scores.reduce((a, b) => a + b, 0) / scores.length : null;
    const keep = (rows.length === all.landings.length);
    return { ...all, filter: { aircraft, airport, days }, flights: rows.length, recordings: keep ? all.recordings : { total: rows.length, counted: rows.length, ground_only: 0, too_short: 0, partial: 0 }, landing_count: rows.length, scored: scores.length, hours: Math.round(rows.length * 9.5) / 10,
      landings: rows, score: keep ? all.score : { mean, median: mean, recent: null, earlier: null },
      by_aircraft: all.by_aircraft.filter((r) => !aircraft || r.icao === aircraft), by_airport: all.by_airport.filter((r) => !airport || r.icao === airport) };
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
  checkUpdate: async () => null,
  async installUpdate() {
    throw new Error("Updating needs the desktop app.");
  },
  async openFlightsFolder() {
    throw new Error("Opening the flights folder needs the desktop app.");
  },
  async storageUsage() {
    const flights = await firstJson<FlightSummary[]>(mockFiles("flights.json"));
    return { flights: flights.length, bytes: flights.length * 1_800_000, folder: "Documents\\FOQA-Sim\\flights" };
  },
  exportFlight: (id, format) => mockExport(`${id.split(COPY)[0]}.${format}`),
  exportLogbook: () => mockExport("logbook.csv"),
  saveImage: async (png, name) => download(png, name),
  ...mockSetup(params.get("setup")),
  ...mockChallenges(params.get("status")),
  ...mockSites(params.get("sites")),
  on: async () => () => {},
};
