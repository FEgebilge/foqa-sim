import { useCallback, useEffect, useState } from "react";
import { api } from "../../api";
import { useNotify } from "../../lib/context";
import { CHALLENGES_HREF, challengeHref } from "../../lib/route";
import type { DeepLink } from "../../lib/sites";
import { useBackendEvent } from "../../lib/use-backend";
import { ConfirmDialog } from "../ConfirmDialog";

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
const go = (href: string) => { window.location.hash = href.slice(1); };

/** A link from a website (docs/challenge-protocol.md A3, B2): it never acts by itself. An unknown site asks
 * "Connect to <host>?" before anything is fetched from it; a site of the user's opens the challenge's briefing,
 * after a sign-in the user starts. Placing and uploading still need the user. */
export function DeepLinkDialog() {
  const notify = useNotify();
  const [link, setLink] = useState<DeepLink | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const close = useCallback(() => {
    setLink(null);
    setBusy(null);
    setError(null);
    void api.deepLinkDone();
  }, []);

  /** Signed in: the challenge taken into the library and its briefing shown. */
  const open = useCallback(async (l: DeepLink) => {
    if (!l.origin) return close();
    try {
      if (!l.signed_in) {
        setBusy(`Sign in on ${l.host} in your browser…`);
        await api.siteSignIn(l.origin, false);
      }
      if (l.kind === "challenge" && l.id) {
        setBusy("Opening the challenge…");
        const entry = await api.siteChallengeTake(l.origin, l.id);
        go(challengeHref(entry.key));
        notify({ title: `Opened “${entry.title}”`, body: `From ${l.host}. Check the briefing, then place the aircraft.` });
      } else {
        notify({ title: `Connected to ${l.host}`, body: "Its challenges are under Sites." });
      }
      close();
    } catch (e) {
      setBusy(null);
      setError(message(e));
    }
  }, [close, notify]);

  const arrive = useCallback((l: DeepLink | null) => {
    if (!l) return;
    go(CHALLENGES_HREF);
    if (l.state === "refused" || l.state === "disabled") {
      notify({ tone: "error", title: l.state === "disabled" ? "Sites are disabled in this build" : "This link couldn't be opened", body: l.text });
      void api.deepLinkDone();
      return;
    }
    setError(null);
    setBusy(null);
    setLink(l);
    // The user's own, signed-in site: the briefing opens; nothing is placed or uploaded.
    if (l.state === "open" && l.signed_in) void open(l);
  }, [notify, open]);

  useEffect(() => { api.deepLinkPending().then(arrive, () => {}); }, [arrive]);
  useBackendEvent("deep-link", arrive);

  const allow = async () => {
    if (!link?.origin) return;
    if (link.state === "open") return void open(link);
    setBusy(`Connecting to ${link.host}…`);
    try {
      const site = await api.deepLinkConsent(link.origin, true);
      if (!site) return close();
      const connected: DeepLink = { ...link, state: "open", signed_in: site.signed_in, name: site.name || undefined };
      setLink(connected);
      await open(connected);
    } catch (e) {
      setBusy(null);
      setError(message(e));
    }
  };
  const deny = () => {
    if (link?.state === "ask" && link.origin && !busy) void api.deepLinkConsent(link.origin, false).catch(() => {});
    if (busy && link?.origin) void api.siteSignInCancel(link.origin).catch(() => {});
    close();
  };

  const asking = link !== null && !(link.state === "open" && link.signed_in && !error);
  const what = link?.kind === "challenge" ? <>the challenge <span className="readout">{link.id}</span>{link.v ? `, version ${link.v}` : ""}</> : "its challenges";
  return (
    <ConfirmDialog open={asking} variant="primary" busy={busy !== null}
      title={link?.state === "ask" ? `Connect to ${link.host}?` : `Sign in to ${link?.host ?? "the site"}`}
      confirmLabel={error ? "Try again" : link?.state === "ask" ? "Connect" : "Sign in"} cancelLabel={busy ? "Cancel" : "Not now"}
      onConfirm={() => void allow()} onCancel={deny}>
      {link?.state === "ask" ? (
        <p>A website asked FOQA-Sim to open {what} from <b className="readout">{link.host}</b>, a site you haven't connected.
          Connect only if you trust this address: FOQA-Sim then reads its challenge list and asks you to sign in there, so it can rank your attempts.</p>
      ) : (
        <p>{link?.name ? `${link.name} ` : ""}(<span className="readout">{link?.host}</span>) is one of your sites, but you're signed out. Sign in to open {what}.</p>
      )}
      {busy ? <p className="mt-2 text-fg-3">{busy}</p> : null}
      {error ? <p className="mt-2 text-danger" role="alert">{error}</p> : null}
    </ConfirmDialog>
  );
}
