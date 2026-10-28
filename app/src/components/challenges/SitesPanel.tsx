import { useCallback, useEffect, useState, type FormEvent } from "react";
import { ChevronRight, Download, Globe, KeyRound, LogIn, LogOut, Plus, Trash2 } from "lucide-react";
import { Badge } from "@ui/components/badge/badge";
import { Button } from "@ui/components/button/button";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { api } from "../../api";
import { useNotify } from "../../lib/context";
import { flightDate } from "../../lib/format";
import { challengeHref } from "../../lib/route";
import { useLoad } from "../../lib/use-backend";
import type { SiteChallenge, SiteSettings, SiteView } from "../../lib/sites";
import { ConfirmDialog } from "../ConfirmDialog";

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** Challenges → Sites (docs/challenge-protocol.md B3): add a site by its address, sign in, take its challenges. */
export function SitesPanel({ sites, onChanged }: { sites: SiteView[]; onChanged: () => void }) {
  const notify = useNotify();
  const [address, setAddress] = useState("");
  const [adding, setAdding] = useState(false);
  const [leaving, setLeaving] = useState<{ site: SiteView; remove: boolean } | null>(null);
  const [busy, setBusy] = useState(false);

  const add = (event: FormEvent) => {
    event.preventDefault();
    if (!address.trim()) return;
    setAdding(true);
    api.siteAdd(address)
      .then((site) => { setAddress(""); notify({ title: `Added ${site.name || site.host}`, body: "Sign in to fly its challenges ranked." }); onChanged(); },
        (error: unknown) => notify({ tone: "error", title: "Could not add the site", body: message(error) }))
      .finally(() => setAdding(false));
  };
  const leave = () => {
    if (!leaving) return;
    const { site, remove } = leaving;
    setBusy(true);
    (remove ? api.siteRemove(site.origin) : api.siteSignOut(site.origin).then(() => {}))
      .then(onChanged, (error: unknown) => notify({ tone: "error", title: remove ? "Could not remove the site" : "Could not sign out", body: message(error) }))
      .finally(() => { setBusy(false); setLeaving(null); });
  };

  return (
    <section aria-labelledby="sites-title" className="panel overflow-hidden">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-line px-4 py-3">
        <div>
          <h2 id="sites-title" className="font-display text-sm font-semibold">Sites</h2>
          <p className="text-xs text-fg-3">Websites that publish challenges and rank your uploaded attempts. Without one, everything stays on this PC.</p>
        </div>
        <form className="flex items-center gap-2" onSubmit={add}>
          <label className="sr-only" htmlFor="site-address">Site address</label>
          <input id="site-address" type="url" inputMode="url" placeholder="https://example.org" value={address} onChange={(e) => setAddress(e.target.value)}
            className="readout w-64 rounded-md border border-line bg-surface px-3 py-1.5 text-sm text-fg placeholder:text-fg-3 focus-visible:outline-2 focus-visible:outline-[var(--accent)]" />
          <Button type="submit" variant="secondary" size="sm" loading={adding} disabled={!address.trim()}><Plus size={14} aria-hidden="true" />Add site</Button>
        </form>
      </div>
      {sites.length ? (
        <ul className="divide-y divide-line">
          {sites.map((site) => <SiteRow key={site.origin} site={site} onChanged={onChanged} onLeave={(remove) => setLeaving({ site, remove })} />)}
        </ul>
      ) : <p className="px-4 py-3 text-sm text-fg-3">No sites. Add one by its address to fly its challenges ranked.</p>}
      <Settings />
      <ConfirmDialog open={leaving !== null} busy={busy} title={leaving?.remove ? "Remove this site?" : "Sign out of this site?"}
        confirmLabel={leaving?.remove ? "Remove site" : "Sign out"} onConfirm={leave} onCancel={() => setLeaving(null)}>
        {leaving ? <LeaveText site={leaving.site} remove={leaving.remove} /> : null}
      </ConfirmDialog>
    </section>
  );
}

const SETTINGS: { key: keyof SiteSettings; label: string; hint: string }[] = [
  { key: "links", label: "Open challenge links from websites", hint: "Off, a site's “Fly this challenge” link is ignored." },
  { key: "notify", label: "Notify when a site scores an attempt", hint: "A Windows notification while FOQA-Sim isn't in front." },
];

/** The global switches of Sites (B3), kept by the app. */
function Settings() {
  const notify = useNotify();
  const [settings, setSettings] = useState<SiteSettings | null>(null);
  useEffect(() => { api.siteSettings().then(setSettings, () => setSettings(null)); }, []);
  if (!settings) return null;
  const change = (key: keyof SiteSettings, on: boolean) => {
    api.siteSettingsSet({ ...settings, [key]: on })
      .then(setSettings, (error: unknown) => notify({ tone: "error", title: "Could not change the setting", body: message(error) }));
  };
  return (
    <fieldset className="flex flex-wrap gap-x-6 gap-y-2 border-t border-line px-4 py-3">
      <legend className="sr-only">Site settings</legend>
      {SETTINGS.map(({ key, label, hint }) => (
        <label key={key} className="flex items-start gap-2 text-sm">
          <input type="checkbox" className="mt-0.5 accent-[var(--accent)]" checked={settings[key]} onChange={(e) => change(key, e.target.checked)} />
          <span>{label}<span className="block text-xs text-fg-3">{hint}</span></span>
        </label>
      ))}
    </fieldset>
  );
}

function LeaveText({ site, remove }: { site: SiteView; remove: boolean }) {
  const n = site.pending;
  const pending = n
    ? `${n} attempt${n === 1 ? " hasn't" : "s haven't"} uploaded yet. ${n === 1 ? "It'll" : "They'll"} upload when you sign back in${site.handle ? ` as ${site.handle}` : ""}; signing in as someone else discards ${n === 1 ? "it" : "them"}. `
    : "";
  return <>{pending}{remove ? `FOQA-Sim forgets ${site.host} and its sign-in. Attempts and their logs stay.` : `FOQA-Sim revokes its access to ${site.host} and deletes the sign-in from this PC.`}</>;
}

function SiteRow({ site, onChanged, onLeave }: { site: SiteView; onChanged: () => void; onLeave: (remove: boolean) => void }) {
  const notify = useNotify();
  const [signing, setSigning] = useState<"browser" | "device" | null>(null);
  const [open, setOpen] = useState(false);
  const signIn = (device: boolean) => {
    setSigning(device ? "device" : "browser");
    api.siteSignIn(site.origin, device)
      .then((s) => notify({ title: `Signed in to ${s.host}${s.handle ? ` as ${s.handle}` : ""}` }),
        (error: unknown) => notify({ tone: "error", title: "Not signed in", body: message(error) }))
      .finally(() => { setSigning(null); onChanged(); });
  };

  return (
    <li>
      <div className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3 px-4 py-3">
        <button type="button" className="rounded p-1 text-fg-3 hover:bg-muted hover:text-fg" aria-expanded={open} aria-label={`${open ? "Hide" : "Show"} challenges of ${site.host}`} onClick={() => setOpen(!open)}>
          <ChevronRight size={15} className={`transition-transform ${open ? "rotate-90" : ""}`} aria-hidden="true" />
        </button>
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <Globe size={14} className="text-fg-3" aria-hidden="true" />
            {site.name ? <span className="text-sm font-medium">{site.name}</span> : null}
            <span className="readout text-xs text-fg-2">{site.host}</span>
            {site.signed_in ? <Badge size="sm" tone="success">{site.handle ? `Signed in as ${site.handle}` : "Signed in"}</Badge> : <Badge size="sm" tone="neutral">Signed out</Badge>}
            {site.pending ? <span className="text-xs" style={{ color: "var(--warning)" }}>{site.pending} to upload</span> : null}
          </div>
          {site.device ? (
            <p className="mt-1.5 text-sm text-fg-2">
              Enter <span className="readout rounded bg-muted px-1.5 py-0.5 text-base font-semibold tracking-widest text-fg">{site.device.user_code}</span> at <span className="readout">{site.device.verification_uri}</span>
            </p>
          ) : null}
        </div>
        <div className="flex flex-wrap items-center justify-end gap-1.5">
          {signing ? (
            <Button variant="ghost" size="sm" onClick={() => api.siteSignInCancel(site.origin)}>Cancel sign-in</Button>
          ) : site.signed_in ? (
            <Button variant="ghost" size="sm" onClick={() => onLeave(false)}><LogOut size={14} aria-hidden="true" />Sign out</Button>
          ) : (
            <>
              <Button variant="primary" size="sm" onClick={() => signIn(false)}><LogIn size={14} aria-hidden="true" />Sign in</Button>
              <Tooltip content="For when the browser is on another device: sign in there with a code">
                <Button variant="ghost" size="sm" onClick={() => signIn(true)}><KeyRound size={14} aria-hidden="true" />Use a code</Button>
              </Tooltip>
            </>
          )}
          <Tooltip content="Remove this site" side="left">
            <button type="button" className="rounded p-1.5 text-fg-3 hover:bg-muted hover:text-danger" aria-label={`Remove ${site.host}`} onClick={() => onLeave(true)}>
              <Trash2 size={14} aria-hidden="true" />
            </button>
          </Tooltip>
        </div>
      </div>
      {signing === "browser" ? <p className="px-4 pb-3 pl-14 text-xs text-fg-3">Finish signing in in your browser; this waits up to 5 minutes.</p> : null}
      {open ? <SiteChallenges key={site.origin} site={site} onTaken={onChanged} /> : null}
    </li>
  );
}

function SiteChallenges({ site, onTaken }: { site: SiteView; onTaken: () => void }) {
  const notify = useNotify();
  const load = useCallback(() => api.siteChallenges(site.origin), [site.origin]);
  const { state, patch } = useLoad(load);
  const [taking, setTaking] = useState<string | null>(null);
  const take = (c: SiteChallenge) => {
    setTaking(c.id);
    api.siteChallengeTake(site.origin, c.id)
      .then((entry) => {
        patch((old) => old.map((x) => (x.id === c.id ? { ...x, key: entry.key } : x)));
        onTaken();
        window.location.hash = challengeHref(entry.key).slice(1);
      }, (error: unknown) => notify({ tone: "error", title: "Could not take the challenge", body: message(error) }))
      .finally(() => setTaking(null));
  };

  if (state.status === "loading") return <div className="mx-4 mb-3 h-10 animate-pulse rounded-md bg-muted" aria-label="Loading the site's challenges" />;
  if (state.status === "error") return <p className="px-4 pb-3 pl-14 text-sm" style={{ color: "var(--danger)" }}>{state.message}</p>;
  const list = state.data;
  if (!list.length) return <p className="px-4 pb-3 pl-14 text-sm text-fg-3">{site.host} has no open challenges right now.</p>;
  return (
    <ul className="mx-4 mb-3 ml-14 divide-y divide-line rounded-md border border-line">
      {list.map((c) => (
        <li key={`${c.id}-${c.version}`} className="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2">
          <span className="min-w-0 flex-1 text-sm">{c.title}<span className="readout ml-2 text-xs text-fg-3">v{c.version}</span></span>
          <span className="text-xs text-fg-3">{c.state === "upcoming" && c.opens_at ? `Opens ${flightDate(c.opens_at)}` : c.closes_at ? `Closes ${flightDate(c.closes_at)}` : c.state}</span>
          {c.key ? (
            <Button variant="ghost" size="sm" href={challengeHref(c.key)}>Open</Button>
          ) : (
            <Button variant="secondary" size="sm" onClick={() => take(c)} loading={taking === c.id} disabled={c.state !== "open" || (taking !== null && taking !== c.id)}>
              <Download size={14} aria-hidden="true" />Add to library
            </Button>
          )}
        </li>
      ))}
    </ul>
  );
}
