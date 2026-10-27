import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { useBackendEvent } from "./use-backend";

// Challenge sites (docs/challenge-protocol.md B3): only when the app is built with its `connect` feature.

export interface SiteView {
  origin: string;
  /** In ASCII (punycode for an IDN host): always shown, the name never replaces it. */
  host: string;
  /** Self-declared by the site. */
  name: string;
  handle: string | null;
  signed_in: boolean;
  /** Attempts on this site not uploaded yet. */
  pending: number;
  /** The code to enter on the site while a device sign-in waits. */
  device: { user_code: string; verification_uri: string } | null;
  account_url: string | null;
}

export interface SiteChallenge {
  id: string;
  version: number;
  title: string;
  state: string;
  opens_at: string | null;
  closes_at: string | null;
  /** The library key, once taken. */
  key: string | null;
}

export type SiteState =
  | "issuing" | "issued" | "practice" | "upload_pending" | "upload_retry" | "queued" | "scoring" | "scored" | "rejected"
  | "failed" | "expired" | "removed" | "waiting" | "upload_refused" | "upload_abandoned" | "abandoning" | "abandoned";

export interface SiteAttemptStatus {
  id: string;
  status: string;
  verified: boolean;
  unverified: string[];
  rank: number | null;
  result: { score?: { total: number } } | null;
  url: string | null;
}

/** An attempt's site part; `id` is the local attempt id. */
export interface SiteAttempt {
  id: string;
  origin: string;
  /** `<name> (<host>)`. */
  site: string;
  state: SiteState;
  /** The app's own plain copy (A6). */
  text: string;
  /** The site's own message, for Details. */
  detail: string | null;
  status: SiteAttemptStatus | null;
  url: string | null;
  upload_by: string | null;
  log: { at: string; seq: number; event: string; [field: string]: unknown }[];
}

export interface SitesApi {
  /** Whether this build has the site client; false (or a missing command) hides every site part. */
  connectAvailable(): Promise<boolean>;
  sitesList(): Promise<SiteView[]>;
  siteAdd(origin: string): Promise<SiteView>;
  /** Browser sign-in, or a code to enter on the site (`device`). Resolves when signed in. */
  siteSignIn(origin: string, device: boolean): Promise<SiteView>;
  siteSignInCancel(origin: string): Promise<void>;
  siteSignOut(origin: string): Promise<SiteView>;
  siteRemove(origin: string): Promise<void>;
  siteChallenges(origin: string): Promise<SiteChallenge[]>;
  /** Takes a site's challenge into the library; returns its library key. */
  siteChallengeTake(origin: string, id: string): Promise<{ key: string; title: string }>;
  siteAttempts(): Promise<SiteAttempt[]>;
  /** Uploads or polls now; no id: every queued attempt (the screen opening). */
  attemptUpload(id: string | null): Promise<void>;
  /** Opens a page of a connected site in the browser. */
  siteOpen(url: string): Promise<void>;
}

export type Tone = "neutral" | "success" | "info" | "warning" | "danger";

/** The badge of a site state; the text beside it is the backend's copy. */
export const SITE_STATE: Record<SiteState, { label: string; tone: Tone }> = {
  issuing: { label: "Issuing", tone: "info" },
  issued: { label: "Ranked attempt", tone: "info" },
  practice: { label: "Practice", tone: "neutral" },
  upload_pending: { label: "Upload queued", tone: "warning" },
  upload_retry: { label: "Retrying upload", tone: "warning" },
  queued: { label: "Scoring", tone: "info" },
  scoring: { label: "Scoring", tone: "info" },
  scored: { label: "Scored", tone: "success" },
  rejected: { label: "Rejected", tone: "danger" },
  failed: { label: "Site failed", tone: "danger" },
  expired: { label: "Expired", tone: "neutral" },
  removed: { label: "Removed", tone: "neutral" },
  waiting: { label: "Waiting", tone: "info" },
  upload_refused: { label: "Upload refused", tone: "danger" },
  upload_abandoned: { label: "Not uploaded", tone: "neutral" },
  abandoning: { label: "Abandoning", tone: "neutral" },
  abandoned: { label: "Abandoned", tone: "neutral" },
};

/** Can be retried by hand. */
export const RETRYABLE: SiteState[] = ["upload_pending", "upload_retry", "queued", "scoring", "waiting"];

/** Whether the site client is built in; checked once. */
export function useConnect() {
  const [available, setAvailable] = useState(false);
  useEffect(() => { api.connectAvailable().then(setAvailable, () => setAvailable(false)); }, []);
  return available;
}

/** Loads `load` when `on`, and again on every `site-changed` and `attempt-changed`. */
function useLive<T>(on: boolean, load: () => Promise<T>, empty: T) {
  const [data, setData] = useState<T>(empty);
  const refresh = useCallback(() => { if (on) load().then(setData, () => {}); }, [on, load]);
  useEffect(refresh, [refresh]);
  useBackendEvent("site-changed", refresh);
  useBackendEvent("attempt-changed", refresh);
  return { data, refresh };
}

const listSites = () => api.sitesList();
const listAttempts = () => api.siteAttempts();

export function useSites(on: boolean) {
  return useLive<SiteView[]>(on, listSites, []);
}

/** The site part of every attempt, by local id; opening the screen polls attempts still scoring. */
export function useSiteAttempts(on: boolean) {
  useEffect(() => { if (on) api.attemptUpload(null).catch(() => {}); }, [on]);
  const { data } = useLive<SiteAttempt[]>(on, listAttempts, []);
  return new Map(data.map((a) => [a.id, a]));
}
