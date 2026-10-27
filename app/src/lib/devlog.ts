import { getVersion } from "@tauri-apps/api/app";
import pkg from "../../package.json";
import { api } from "../api";

export interface DevlogEntry { version: string; date: string; title: string; body: string }

const SEMVER = /^(\d+)\.(\d+)\.(\d+)$/;
const parts = (v: string) => v.match(SEMVER)!.slice(1).map(Number);
export const compareVersions = (a: string, b: string) => parts(a).reduce((cmp, n, i) => cmp || Math.sign(n - parts(b)[i]!), 0);

/** `---` frontmatter of `key: value` lines, then the Markdown body. A file without a valid version is skipped. */
function parse(raw: string): DevlogEntry | null {
  const match = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/.exec(raw.trim());
  if (!match) return null;
  const meta: Record<string, string> = Object.fromEntries(match[1]!.split(/\r?\n/).flatMap((line) => {
    const kv = /^(\w+):\s*(.*)$/.exec(line);
    return kv ? [[kv[1]!, kv[2]!.trim()]] : [];
  }));
  if (!SEMVER.test(meta.version ?? "")) return null;
  return { version: meta.version!, date: meta.date ?? "", title: meta.title ?? "", body: match[2]!.trim() };
}

// The owner's release notes, bundled at build time: devlog/<version>.md at the repo root.
const files = import.meta.glob("../../../devlog/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
export const DEVLOG: DevlogEntry[] = Object.values(files).flatMap((raw) => parse(raw) ?? []).sort((a, b) => compareVersions(b.version, a.version));

const SEEN = "foqa-sim.devlog-seen";

/** The running app's version: Tauri's, or package.json in the browser mock. */
export const appVersion = () => (api.live ? getVersion() : Promise.resolve(pkg.version));

/** Entries to announce: newer than the last version the user saw, up to the running app. Null on a first install (the version is only stored). */
export function unseenEntries(current: string): DevlogEntry[] | null {
  try {
    const seen = localStorage.getItem(SEEN);
    if (!seen || !SEMVER.test(seen)) {
      localStorage.setItem(SEEN, current);
      return null;
    }
    return DEVLOG.filter((e) => compareVersions(e.version, seen) > 0 && compareVersions(e.version, current) <= 0);
  } catch { return null; }
}

export function markSeen(version: string) {
  try { localStorage.setItem(SEEN, version); } catch { /* storage unavailable: the dialog may show again */ }
}
