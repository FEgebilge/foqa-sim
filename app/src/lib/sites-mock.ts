import type { SiteAttempt, SiteChallenge, SiteSettings, SitesApi, SiteView } from "./sites";

// Sites in the browser: one example site (a reserved `.example` host, not a real one) standing for the built-in one,
// changed in memory so the Sites panel can be clicked through. `?sites=none` starts with its challenges not open,
// `?sites=dev` offers Add site (the developer switch), `?sites=off` is a build without `connect`.

const ORIGIN = "https://landings.example";
const settle = (ms = 600) => new Promise((resolve) => window.setTimeout(resolve, ms));

export function mockSites(mode: string | null): SitesApi {
  const closed = mode === "none";
  const sites: SiteView[] = [{
    origin: ORIGIN, host: "landings.example", name: "Example Landings", handle: closed ? null : "ege", signed_in: !closed,
    pending: closed ? 0 : 1, device: null, account_url: closed ? null : `${ORIGIN}/#/account`, built_in: true,
    closed: closed ? "Challenges aren't open on Example Landings yet." : null,
  }];
  const challenges: SiteChallenge[] = [
    { id: "ltfj-06-gusty-2026w44", version: 1, title: "Sabiha Gökçen 06, crosswind", state: "open", opens_at: "2026-10-26T00:00:00Z", closes_at: "2026-11-02T00:00:00Z", key: "ltfj-06-gusty-2026w44-v1" },
    { id: "lfpg-27r-self", version: 1, title: "Paris CDG 27R, fly it yourself", state: "open", opens_at: null, closes_at: null, key: null },
    { id: "egll-27l-night-2026w45", version: 1, title: "Heathrow 27L at night", state: "upcoming", opens_at: "2026-11-02T00:00:00Z", closes_at: null, key: null },
  ];
  const at = "2026-10-27T18:05:59.000Z";
  let settings: SiteSettings = { links: true, notify: true };
  // `?link=…` plays one link arriving at start-up, given once.
  let link = new URLSearchParams(window.location.search).get("link");
  const attempts: SiteAttempt[] = [
    {
      id: "6f1c2d3e-4a5b-4c6d-8e7f-9a0b1c2d3e4f", origin: ORIGIN, site: "Example Landings (landings.example)", state: "scored",
      text: "Scored 87.4, #3", detail: null, upload_by: "2026-10-28T18:02:11Z", url: `${ORIGIN}/#/challenges/ltfj-06-gusty-2026w44/attempts/9c1e`,
      status: { id: "9c1e", status: "scored", verified: true, unverified: [], rank: 3, result: { score: { total: 87.4 } }, url: null },
      log: [
        { at: "2026-10-27T18:02:11.300Z", seq: 1, event: "issued", attempt_id: "9c1e", fly_within_s: 1800 },
        { at, seq: 2, event: "upload_queued" },
        { at, seq: 3, event: "upload", try: 1, http: 202, status: "queued", ms: 840 },
        { at: "2026-10-27T18:06:09.000Z", seq: 4, event: "server_status", status: "scored", score: 87.4, rank: 3 },
      ],
    },
    {
      id: "0b9a8c7d-6e5f-4a3b-9c2d-1e0f9a8b7c6d", origin: ORIGIN, site: "Example Landings (landings.example)", state: "upload_retry",
      text: "Can't reach landings.example. Will retry until 28 Oct 17:40.", detail: "connection refused", upload_by: "28 Oct 17:40", url: null, status: null,
      log: [
        { at: "2026-10-27T17:40:00.000Z", seq: 1, event: "issued", attempt_id: "77aa" },
        { at, seq: 2, event: "upload", try: 1, error: "network", retry_in_s: 5 },
      ],
    },
  ];
  const site = (origin: string) => {
    const found = sites.find((s) => s.origin === origin);
    if (!found) throw new Error(`${origin} isn't one of your sites.`);
    return found;
  };

  return {
    connectAvailable: async () => mode !== "off",
    sitesList: async () => sites.map((s) => ({ ...s })),
    siteAddAllowed: async () => mode === "dev",
    async siteAdd(origin) {
      await settle();
      const url = new URL(origin.trim());
      if (url.protocol !== "https:") throw new Error("A site must use https.");
      if (url.pathname !== "/" || url.search || url.hash) throw new Error("Give the site's address only, like https://example.org.");
      if (!url.hostname.endsWith(".example")) throw new Error(`${url.host} doesn't publish FOQA challenges.`);
      const added: SiteView = { origin: url.origin, host: url.host, name: "", handle: null, signed_in: false, pending: 0, device: null, account_url: null, built_in: false, closed: null };
      sites.push(added);
      return added;
    },
    async siteSignIn(origin, device) {
      const s = site(origin);
      if (s.closed) { await settle(); throw new Error(s.closed); }
      if (device) s.device = { user_code: "WDJB-MJHT", verification_uri: `${origin}/#/device` };
      await settle(device ? 2500 : 1200);
      Object.assign(s, { signed_in: true, handle: "ege", device: null });
      return { ...s };
    },
    async siteSignInCancel(origin) { site(origin).device = null; },
    async siteSignOut(origin) {
      await settle();
      return { ...Object.assign(site(origin), { signed_in: false }) };
    },
    async siteRemove(origin) {
      await settle();
      if (site(origin).built_in) throw new Error(`${site(origin).name} comes with FOQA-Sim; sign out instead.`);
      sites.splice(sites.indexOf(site(origin)), 1);
    },
    async siteChallenges(origin) {
      site(origin);
      await settle();
      return challenges.map((c) => ({ ...c }));
    },
    async siteChallengeTake(_origin, id) {
      await settle();
      const c = challenges.find((x) => x.id === id)!;
      c.key = `${c.id}-v${c.version}`;
      return { key: c.key, title: c.title };
    },
    siteAttempts: async () => attempts.map((a) => ({ ...a })),
    async attemptUpload(id) {
      const a = attempts.find((x) => x.id === id);
      if (!a) return;
      await settle(1500);
      Object.assign(a, { state: "queued", text: "Uploaded. Scoring…", detail: null });
    },
    async siteOpen(url) { window.open(url, "_blank", "noopener"); },
    // `?link=ask|open|signed-out|refused|disabled` plays a link from a website arriving at start-up.
    async deepLinkPending() {
      const state = link;
      link = null;
      if (!settings.links) return null;
      if (state === "refused") return { state, signed_in: false, text: "The link's site address isn't one FOQA-Sim can connect to." };
      if (state === "disabled") return { state, signed_in: false, text: "Sites are disabled in this build." };
      if (state !== "ask" && state !== "open" && state !== "signed-out") return null;
      const origin = state === "ask" ? "https://xn--pilotaptitud-yjb.example" : ORIGIN;
      if (state === "signed-out") site(ORIGIN).signed_in = false;
      return { state: state === "ask" ? "ask" : "open", seq: 1, kind: "challenge", origin, host: new URL(origin).host, signed_in: state === "open", id: "ltfj-06-gusty-2026w44", v: 1 };
    },
    async deepLinkConsent(_seq, allow) {
      await settle();
      if (!allow) return null;
      const origin = "https://xn--pilotaptitud-yjb.example";
      const added: SiteView = { origin, host: new URL(origin).host, name: "Look-alike", handle: null, signed_in: false, pending: 0, device: null, account_url: null, built_in: false, closed: null };
      sites.push(added);
      return added;
    },
    // The mock challenge is in the library already with other bytes: Open asks to replace it.
    async deepLinkOpen(_seq, replace) {
      await settle();
      const c = challenges[0];
      const entry = { key: `${c.id}-v${c.version}`, title: c.title };
      return replace ? { entry, conflict: null } : { entry: null, conflict: { ...entry, version: c.version } };
    },
    async deepLinkDone() {},
    siteSettings: async () => ({ ...settings }),
    async siteSettingsSet(next) {
      settings = { ...next };
      return { ...settings };
    },
  };
}
