import type { DeepLink, LinkOpened } from "./sites";

// What a link from a website asks (docs/challenge-protocol.md A3, B2). It never acts by itself: each question
// says what its button does, from what the link says only (host, id, version); a title comes after the click.

/** The question for a link, or null when there is nothing to do (a known, signed-in site's connect link). */
export interface LinkPrompt {
  title: string;
  confirm: string;
  /** What the button does: connect then sign in and open, sign in then open, open, or replace the library's copy. */
  action: "connect" | "sign_in" | "open" | "replace";
}

/** The challenge a link names, in words: "the challenge abc, version 3" or "its challenges". */
export const linkWhat = (link: DeepLink) =>
  link.kind === "challenge" && link.id ? `the challenge ${link.id}${link.v ? `, version ${link.v}` : ""}` : "its challenges";

export function linkPrompt(link: DeepLink, conflict: LinkOpened["conflict"]): LinkPrompt | null {
  const host = link.host ?? "the site";
  if (conflict) return { title: `Replace your copy of “${conflict.title}”?`, confirm: "Replace", action: "replace" };
  if (link.state === "ask") return { title: `Connect to ${host}?`, confirm: "Connect", action: "connect" };
  if (link.state !== "open") return null;
  const challenge = link.kind === "challenge" && link.id !== undefined;
  if (!link.signed_in) return { title: `Sign in to ${host}?`, confirm: challenge ? "Sign in and open" : "Sign in", action: "sign_in" };
  return challenge ? { title: `Open ${link.id} from ${host}?`, confirm: "Open", action: "open" } : null;
}

/** A link the window only tells about (invalid, or sites disabled), with its headline. */
export const linkNotice = (link: DeepLink) =>
  link.state === "refused" ? "This link couldn't be opened" : link.state === "disabled" ? "Sites are disabled in this build" : null;
