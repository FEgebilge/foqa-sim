import { useSyncExternalStore } from "react";

export type Route = { screen: "flights" } | { screen: "setup" } | { screen: "devlog" } | { screen: "trends" } | { screen: "debrief"; id: string } | { screen: "challenges"; key?: string };

/** Hash routes, so the Tauri webview and a static build need no server rewrites: #/, #/setup, #/devlog, #/trends, #/flight/<id>
 * and #/challenges[/<key>]. */
export function parse(hash: string): Route {
  // A hand-typed or truncated link (`#/flight/%`) must never throw while rendering: it opens the flights list.
  try {
    if (hash === SETUP_HREF) return { screen: "setup" };
    if (hash === DEVLOG_HREF) return { screen: "devlog" };
    if (hash === TRENDS_HREF) return { screen: "trends" };
    const challenge = /^#\/challenges(?:\/(.+))?$/.exec(hash);
    if (challenge) return { screen: "challenges", key: challenge[1] ? decodeURIComponent(challenge[1]) : undefined };
    const match = /^#\/flight\/(.+)$/.exec(hash);
    if (match) return { screen: "debrief", id: decodeURIComponent(match[1]!) };
  } catch (e) {
    if (!(e instanceof URIError)) throw e;
  }
  return { screen: "flights" };
}

const subscribe = (notify: () => void) => {
  window.addEventListener("hashchange", notify);
  return () => window.removeEventListener("hashchange", notify);
};

export function useRoute(): Route {
  const hash = useSyncExternalStore(subscribe, () => window.location.hash);
  return parse(hash);
}

export const FLIGHTS_HREF = "#/";
export const SETUP_HREF = "#/setup";
export const DEVLOG_HREF = "#/devlog";
export const TRENDS_HREF = "#/trends";
export const flightHref = (id: string) => `#/flight/${encodeURIComponent(id)}`;
export const CHALLENGES_HREF = "#/challenges";
export const challengeHref = (key: string) => `${CHALLENGES_HREF}/${encodeURIComponent(key)}`;
export const openFlight = (id: string) => { window.location.hash = flightHref(id).slice(1); };
export const openFlights = () => { window.location.hash = "/"; };
export const openSetup = () => { window.location.hash = SETUP_HREF.slice(1); };
export const openDevlog = () => { window.location.hash = DEVLOG_HREF.slice(1); };
