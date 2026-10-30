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
  /** The site FOQA-Sim ships with (Pilot Aptitude): it can't be removed. */
  built_in: boolean;
  /** Why its challenges can't be used yet, e.g. "Challenges aren't open on Pilot Aptitude yet." */
  closed: string | null;
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

/** A link from a website (docs/challenge-protocol.md A3, B2), as the backend asks about it. */
export interface DeepLink {
  /** `ask`: the site isn't the user's, "Connect to <host>?"; `open`: it is, "Open …?";
   * `refused`: the link is invalid; `disabled`: a build without sites. `text` says why for the last two. */
  state: "ask" | "open" | "refused" | "disabled";
  /** Names the link in every answer; a refused or disabled one has none. */
  seq?: number;
  kind?: "challenge" | "connect";
  origin?: string;
  /** In ASCII (punycode for an IDN host): shown before any name. */
  host?: string;
  /** Self-declared, known sites only. */
  name?: string;
  id?: string;
  v?: number;
  signed_in: boolean;
  text?: string;
}

export interface SitesApi {
  /** Whether this build has the site client; false (or a missing command) hides every site part. */
  connectAvailable(): Promise<boolean>;
  /** The built-in site first; its document is asked for again while it has none. */
  sitesList(): Promise<SiteView[]>;
  /** Whether "Add site" is offered: the developer switch only. */
  siteAddAllowed(): Promise<boolean>;
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
  /** What to ask about next, each link given once: asked while no link is shown (also in builds without sites). */
  deepLinkPending(): Promise<DeepLink | null>;
  /** The answer to "Connect to <host>?" for link `seq`; allowed, the site added. */
  deepLinkConsent(seq: number, allow: boolean): Promise<SiteView | null>;
  /** The click on Open (or Replace) for link `seq`: its challenge taken, or another copy kept in the library. */
  deepLinkOpen(seq: number, replace: boolean): Promise<LinkOpened>;
  /** Link `seq` was acted on or dismissed. */
  deepLinkDone(seq: number | undefined): Promise<void>;
  siteSettings(): Promise<SiteSettings>;
  siteSettingsSet(settings: SiteSettings): Promise<SiteSettings>;
}

/** What a link's Open did: `entry` taken (or kept unchanged), or `conflict`: another copy of it is in the library. */
export interface LinkOpened {
  entry: { key: string; title: string } | null;
  conflict: { key: string; title: string; version: number } | null;
}

/** The global settings of Challenges → Sites (B3), kept by the app. */
export interface SiteSettings {
  /** "Open challenge links from websites". */
  links: boolean;
  /** "Notify when a site scores an attempt". */
  notify: boolean;
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
