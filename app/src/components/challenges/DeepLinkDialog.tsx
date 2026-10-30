import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../api";
import { useNotify } from "../../lib/context";
import { OPEN_STATES } from "../../lib/attempts";
import { linkNotice, linkPrompt, linkWhat } from "../../lib/deep-link";
import { challengeHref } from "../../lib/route";
import type { DeepLink } from "../../lib/sites";
import { useBackendEvent } from "../../lib/use-backend";
import { ConfirmDialog } from "../ConfirmDialog";

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
const go = (href: string) => { window.location.hash = href.slice(1); };

/** A link from a website (docs/challenge-protocol.md A3, B2). From a site the pilot is signed in to, "Fly this
 * challenge" was the consent: the challenge opens and the aircraft is placed at once (`fly`). Any other site asks
 * first: an unknown one "Connect to <host>?", a signed-out one "Sign in"; nothing is fetched or kept before that
 * click. One link is handled at a time; a newer one waits until this one is done. */
export function DeepLinkDialog() {
  const notify = useNotify();
  const [link, setLink] = useState<DeepLink | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** The link shown; the backend gives the next one only after it is done. */
  const shown = useRef<DeepLink | null>(null);
  /** A `deepLinkPending` call is out; `again`: a link arrived meanwhile. */
  const asking = useRef(false);
  const again = useRef(false);

  const show = (l: DeepLink | null) => { shown.current = l; setLink(l); };

  /** "Fly this challenge" on a site the pilot is signed in to: the click on the site was the consent. The site's copy
   * is opened (replacing a local one; attempts stay) and, when the pre-flight check has nothing red, the aircraft is
   * placed and the ranked attempt starts at once; otherwise the briefing shows what to fix. */
  const fly = useCallback(async (l: DeepLink) => {
    try {
      const opened = await api.deepLinkOpen(l.seq!, true);
      const entry = opened.entry;
      if (!entry) return;
      go(challengeHref(entry.key));
      const red = (await api.challengePreflight(entry.key)).find((line) => line.state === "fail");
      if (red) {
        notify({ tone: "error", title: `Opened “${entry.title}”`, body: `${red.text}${red.fix ? ` ${red.fix}` : ""}` });
        return;
      }
      notify({ title: `Flying “${entry.title}”`, body: `From ${l.host}. Placing the aircraft at the start…` });
      // The click asks for a new attempt: one still open makes way (this challenge's flies again from its clean start).
      const open = (await api.attemptsList()).find((a) => OPEN_STATES.includes(a.state));
      if (open?.key === entry.key) {
        await api.attemptFlyAgain(open.id);
        return;
      }
      if (open) await api.attemptAbandon(open.id);
      await api.attemptStart(entry.key);
    } catch (e) {
      notify({ tone: "error", title: "Could not fly the challenge", body: message(e) });
    }
  }, [notify]);

  /** Asks the backend for what to show next, while nothing is shown; notices are told and skipped. */
  const next = useCallback(async () => {
    if (shown.current) return;
    if (asking.current) { again.current = true; return; }
    asking.current = true;
    try {
      for (;;) {
        const l = await api.deepLinkPending();
        if (!l) {
          if (again.current) { again.current = false; continue; }
          return;
        }
        const notice = linkNotice(l);
        if (notice) { notify({ tone: "error", title: notice, body: l.text }); continue; }
        const prompt = linkPrompt(l);
        if (!prompt) {
          notify({ title: `${l.host} is one of your sites`, body: "Its challenges are under Sites." });
          await api.deepLinkDone(l.seq);
          continue;
        }
        if (prompt.action === "open") {
          await fly(l);
          await api.deepLinkDone(l.seq);
          continue;
        }
        setError(null);
        setBusy(null);
        show(l);
        return;
      }
    } finally {
      asking.current = false;
      again.current = false;
    }
  }, [notify, fly]);

  useEffect(() => { void next(); }, [next]);
  useBackendEvent("deep-link", useCallback(() => { void next(); }, [next]));

  const close = () => {
    const seq = shown.current?.seq;
    show(null);
    setBusy(null);
    setError(null);
    void api.deepLinkDone(seq).then(next);
  };

  const confirm = async () => {
    const l = shown.current;
    const prompt = l && linkPrompt(l);
    if (!l?.origin || l.seq === undefined || !prompt) return close();
    setError(null);
    try {
      let current = l;
      if (prompt.action === "connect") {
        setBusy(`Connecting to ${l.host}…`);
        const site = await api.deepLinkConsent(l.seq, true);
        if (!site) return close();
        current = { ...l, state: "open", signed_in: site.signed_in, name: site.name || undefined };
        show(current);
      }
      if (!current.signed_in) {
        setBusy(`Sign in on ${l.host} in your browser…`);
        await api.siteSignIn(l.origin, false);
        current = { ...current, signed_in: true };
        show(current);
      }
      if (l.kind !== "challenge") {
        notify({ title: `Connected to ${l.host}`, body: "Its challenges are under Sites." });
        return close();
      }
      setBusy("Opening the challenge…");
      await fly(current);
      close();
    } catch (e) {
      setBusy(null);
      setError(message(e));
    }
  };
  const deny = () => {
    const l = shown.current;
    if (l?.state === "ask" && l.seq !== undefined && !busy) void api.deepLinkConsent(l.seq, false).catch(() => {});
    if (busy && l?.origin) void api.siteSignInCancel(l.origin).catch(() => {});
    close();
  };

  const prompt = link ? linkPrompt(link) : null;
  const host = <b className="readout">{link?.host}</b>;
  const site = link?.name ? <>{link.name} ({host})</> : host;
  const what = link ? linkWhat(link) : "";
  return (
    <ConfirmDialog open={prompt !== null} variant="primary" busy={busy !== null}
      title={prompt?.title ?? ""} confirmLabel={error ? "Try again" : prompt?.confirm ?? "OK"}
      cancelLabel={busy ? "Cancel" : "Not now"}
      onConfirm={() => void confirm()} onCancel={deny}>
      {prompt?.action === "connect" ? (
        <p>A website asked FOQA-Sim to open {what} from <b className="readout">{link?.host}</b>, a site you haven't connected.
          Connect only if you trust this address: FOQA-Sim then reads its challenge list, asks you to sign in there and places your aircraft at the challenge's start, so it can rank your attempts.</p>
      ) : prompt?.action === "sign_in" ? (
        <p>{site} is one of your sites, but you're signed out. Sign in to fly {what}.</p>
      ) : null}
      {busy ? <p className="mt-2 text-fg-3">{busy}</p> : null}
      {error ? <p className="mt-2 text-danger" role="alert">{error}</p> : null}
    </ConfirmDialog>
  );
}
