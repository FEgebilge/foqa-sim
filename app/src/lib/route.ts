import { useSyncExternalStore } from "react";

export type Route = { screen: "flights" } | { screen: "debrief"; id: string };

/** Hash routes, so the Tauri webview and a static build need no server rewrites: #/ and #/flight/<id>. */
function parse(hash: string): Route {
  const match = /^#\/flight\/(.+)$/.exec(hash);
  return match ? { screen: "debrief", id: decodeURIComponent(match[1]!) } : { screen: "flights" };
}

const subscribe = (notify: () => void) => {
  window.addEventListener("hashchange", notify);
  return () => window.removeEventListener("hashchange", notify);
};

export function useRoute(): Route {
  const hash = useSyncExternalStore(subscribe, () => window.location.hash);
  return parse(hash);
}

export const flightHref = (id: string) => `#/flight/${encodeURIComponent(id)}`;
export const openFlight = (id: string) => { window.location.hash = flightHref(id).slice(1); };
export const openFlights = () => { window.location.hash = "/"; };
