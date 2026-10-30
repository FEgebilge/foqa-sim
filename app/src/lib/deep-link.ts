import type { DeepLink } from "./sites";

// What a link from a website asks (docs/challenge-protocol.md A3, B2). Only a signed-in site's challenge link acts
// at once (it flies); every other question says what its button does, from what the link says only (host, id,
// version); a title comes after the click.

/** The question for a link, or null when there is nothing to do (a known, signed-in site's connect link). */
export interface LinkPrompt {
  title: string;
  confirm: string;
  /** What happens: connect then sign in and fly, sign in then fly, or fly at once (no question). */
  action: "connect" | "sign_in" | "open";
}

/** The challenge a link names, in words: "the challenge abc, version 3" or "its challenges". */
export const linkWhat = (link: DeepLink) =>
  link.kind === "challenge" && link.id ? `the challenge ${link.id}${link.v ? `, version ${link.v}` : ""}` : "its challenges";

export function linkPrompt(link: DeepLink): LinkPrompt | null {
  const host = link.host ?? "the site";
  if (link.state === "ask") return { title: `Connect to ${host}?`, confirm: "Connect", action: "connect" };
  if (link.state !== "open") return null;
  const challenge = link.kind === "challenge" && link.id !== undefined;
  if (!link.signed_in) return { title: `Sign in to ${host}?`, confirm: challenge ? "Sign in and fly" : "Sign in", action: "sign_in" };
  return challenge ? { title: `Fly ${link.id} from ${host}`, confirm: "Fly", action: "open" } : null;
}

/** A link the window only tells about (invalid, or sites disabled), with its headline. */
export const linkNotice = (link: DeepLink) =>
  link.state === "refused" ? "This link couldn't be opened" : link.state === "disabled" ? "Sites are disabled in this build" : null;
