import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../api";
import { useNotify } from "../../lib/context";
import { linkNotice, linkPrompt, linkWhat } from "../../lib/deep-link";
import { challengeHref } from "../../lib/route";
import type { DeepLink, LinkOpened } from "../../lib/sites";
import { useBackendEvent } from "../../lib/use-backend";
import { ConfirmDialog } from "../ConfirmDialog";

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
const go = (href: string) => { window.location.hash = href.slice(1); };

/** A link from a website (docs/challenge-protocol.md A3, B2): it never acts by itself. Nothing is fetched, kept or
 * opened until the user clicks: an unknown site asks "Connect to <host>?", a signed-out one "Sign in", one of the
 * user's "Open <id> from <host>?". One link is asked about at a time; a newer one waits until this one is closed.
 * Placing and uploading still need the user. */
export function DeepLinkDialog() {
  const notify = useNotify();
  const [link, setLink] = useState<DeepLink | null>(null);
  const [conflict, setConflict] = useState<LinkOpened["conflict"]>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** The link shown; the backend gives the next one only after it is done. */
  const shown = useRef<DeepLink | null>(null);
  /** A `deepLinkPending` call is out; `again`: a link arrived meanwhile. */
  const asking = useRef(false);
  const again = useRef(false);

  const show = (l: DeepLink | null) => { shown.current = l; setLink(l); };

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
        if (!linkPrompt(l, null)) {
          notify({ title: `${l.host} is one of your sites`, body: "Its challenges are under Sites." });
          await api.deepLinkDone(l.seq);
          continue;
        }
        setConflict(null);
        setError(null);
        setBusy(null);
        show(l);
        return;
      }
    } finally {
      asking.current = false;
      again.current = false;
    }
  }, [notify]);

  useEffect(() => { void next(); }, [next]);
  useBackendEvent("deep-link", useCallback(() => { void next(); }, [next]));

  const close = () => {
    const seq = shown.current?.seq;
    show(null);
    setConflict(null);
    setBusy(null);
    setError(null);
    void api.deepLinkDone(seq).then(next);
  };

  const confirm = async () => {
    const l = shown.current;
    const prompt = l && linkPrompt(l, conflict);
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
      const opened = await api.deepLinkOpen(l.seq, prompt.action === "replace");
      if (opened.conflict) {
        setBusy(null);
        setConflict(opened.conflict);
        return;
      }
      if (opened.entry) {
        go(challengeHref(opened.entry.key));
        notify({ title: `Opened “${opened.entry.title}”`, body: `From ${l.host}. Check the briefing, then place the aircraft.` });
      }
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
    // Keeping one's own copy shows it.
    if (conflict && !busy) go(challengeHref(conflict.key));
    close();
  };

  const prompt = link ? linkPrompt(link, conflict) : null;
  const host = <b className="readout">{link?.host}</b>;
  const site = link?.name ? <>{link.name} ({host})</> : host;
  const what = link ? linkWhat(link) : "";
  return (
    <ConfirmDialog open={prompt !== null} variant="primary" busy={busy !== null}
      title={prompt?.title ?? ""} confirmLabel={error ? "Try again" : prompt?.confirm ?? "OK"}
      cancelLabel={busy ? "Cancel" : prompt?.action === "replace" ? "Keep mine" : "Not now"}
      onConfirm={() => void confirm()} onCancel={deny}>
      {prompt?.action === "connect" ? (
        <p>A website asked FOQA-Sim to open {what} from <b className="readout">{link?.host}</b>, a site you haven't connected.
          Connect only if you trust this address: FOQA-Sim then reads its challenge list, asks you to sign in there and opens the challenge, so it can rank your attempts.</p>
      ) : prompt?.action === "sign_in" ? (
        <p>{site} is one of your sites, but you're signed out. Sign in to open {what}.</p>
      ) : prompt?.action === "open" ? (
        <p>A website asked FOQA-Sim to open {what} from {site}, one of your sites. Open adds it to your library and shows its briefing; placing the aircraft still needs you.</p>
      ) : conflict ? (
        <p>Your library already has another copy of “{conflict.title}”, version {conflict.version}. Replace it with the copy from <b className="readout">{link?.host}</b>? Its attempts stay.</p>
      ) : null}
      {busy ? <p className="mt-2 text-fg-3">{busy}</p> : null}
      {error ? <p className="mt-2 text-danger" role="alert">{error}</p> : null}
    </ConfirmDialog>
  );
}
