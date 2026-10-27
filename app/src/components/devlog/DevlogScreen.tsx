import { useEffect, useMemo, useState } from "react";
import { ChevronRight, Newspaper } from "lucide-react";
import { Badge } from "@ui/components/badge/badge";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import { appVersion, DEVLOG, type DevlogEntry } from "../../lib/devlog";
import { Markdown } from "../../lib/markdown";

const OPEN_KEY = "foqa-sim.devlog-open";
const fmt = (iso: string, opts: Intl.DateTimeFormatOptions) => {
  const d = new Date(`${iso}T00:00:00`);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString(undefined, opts);
};
const shortDate = (iso: string) => fmt(iso, { year: "numeric", month: "short", day: "numeric" });

/**
 * Hierarchy: version series (major.minor) > versions. Releases are sparse (a handful per month at best), so a
 * Year > Month layer would hold one or two entries each and just add clicks; the series is the unit people
 * think in ("0.2"), and each group header still carries its date range, which covers the by-date need.
 */
interface Series { key: string; entries: DevlogEntry[]; range: string }
const seriesOf = (entries: DevlogEntry[]): Series[] => {
  const groups = new Map<string, DevlogEntry[]>();
  for (const e of entries) {
    const key = e.version.split(".").slice(0, 2).join(".");
    groups.set(key, [...(groups.get(key) ?? []), e]);
  }
  return [...groups].map(([key, list]) => {
    const dates = list.map((e) => e.date).filter(Boolean).sort();
    const first = dates[0], last = dates[dates.length - 1];
    return { key, entries: list, range: !first ? "" : first === last ? shortDate(first) : `${shortDate(first)} – ${shortDate(last!)}` };
  });
};

/** Open/closed state survives navigation within the session; the newest entry (and its series) default to open. */
function useOpenState(defaults: Record<string, boolean>) {
  const [open, setOpen] = useState<Record<string, boolean>>(() => {
    try { return { ...defaults, ...JSON.parse(sessionStorage.getItem(OPEN_KEY) ?? "{}") }; } catch { return defaults; }
  });
  const set = (key: string, value: boolean) => setOpen((prev) => {
    const next = { ...prev, [key]: value };
    try { sessionStorage.setItem(OPEN_KEY, JSON.stringify(next)); } catch { /* storage unavailable: state is per page view */ }
    return next;
  });
  return [open, set] as const;
}

function Disclosure({ id, open, onToggle, summary, children, nested }: {
  id: string; open: boolean; onToggle: (open: boolean) => void; summary: React.ReactNode; children: React.ReactNode; nested?: boolean;
}) {
  return (
    <details id={id} open={open} onToggle={(e) => { if (e.currentTarget.open !== open) onToggle(e.currentTarget.open); }} className="group scroll-mt-4">
      <summary className={`flex cursor-pointer list-none items-center gap-2 rounded-md px-3 py-2 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-accent [&::-webkit-details-marker]:hidden ${nested ? "text-sm" : ""}`}>
        <ChevronRight size={14} aria-hidden className="shrink-0 text-fg-3 transition-transform group-open:rotate-90" />
        {summary}
      </summary>
      {open ? children : null}
    </details>
  );
}

/** Release notes as a collapsible tree: a group index on the left (wide screens), the tree on the right. */
export function DevlogScreen() {
  const [current, setCurrent] = useState<string>();
  useEffect(() => { appVersion().then(setCurrent, () => undefined); }, []);
  const series = useMemo(() => seriesOf(DEVLOG), []);
  const newest = DEVLOG[0]?.version;
  const [open, setOpen] = useOpenState({ ...(series[0] ? { [`s:${series[0].key}`]: true } : {}), ...(newest ? { [`v:${newest}`]: true } : {}) });

  const jump = (key: string) => {
    setOpen(`s:${key}`, true);
    requestAnimationFrame(() => document.getElementById(`s-${key}`)?.scrollIntoView({ block: "start", behavior: "smooth" }));
  };

  return (
    <main className="flex w-full flex-col gap-5 px-6 py-6 2xl:px-8">
      <header>
        <p className="eyebrow">Release notes</p>
        <h1 className="mt-1 font-display text-2xl font-semibold tracking-tight">What's new</h1>
        <p className="mt-1 text-sm text-fg-2">What changed in each version of FOQA-Sim, newest first.</p>
      </header>
      {DEVLOG.length ? (
        <div className="grid items-start gap-6 lg:grid-cols-[13rem_minmax(0,1fr)]">
          <nav aria-label="Version series" className="panel sticky top-4 hidden flex-col gap-0.5 p-2 lg:flex">
            {series.map((s) => (
              <button key={s.key} type="button" onClick={() => jump(s.key)} className="flex items-center justify-between rounded-md px-3 py-1.5 text-left text-sm text-fg-2 outline-none hover:bg-muted hover:text-fg focus-visible:ring-2 focus-visible:ring-accent">
                <span className="readout">{s.key}.x</span><span className="text-xs text-fg-3">{s.entries.length}</span>
              </button>
            ))}
          </nav>
          <div className="panel flex flex-col gap-1 p-2">
            {series.map((s) => (
              <Disclosure key={s.key} id={`s-${s.key}`} open={!!open[`s:${s.key}`]} onToggle={(v) => setOpen(`s:${s.key}`, v)}
                summary={<><span className="font-display font-semibold"><span className="readout">{s.key}.x</span></span>
                  <span className="text-xs text-fg-3">{s.entries.length} {s.entries.length === 1 ? "release" : "releases"}{s.range ? ` · ${s.range}` : ""}</span></>}>
                <div className="ml-4 flex flex-col gap-0.5 border-l border-line pl-2">
                  {s.entries.map((e) => (
                    <Disclosure key={e.version} nested id={`v-${e.version}`} open={!!open[`v:${e.version}`]} onToggle={(v) => setOpen(`v:${e.version}`, v)}
                      summary={<>
                        <span className="readout font-medium">v{e.version}</span>
                        {e.title ? <span className="text-fg-2">· {e.title}</span> : null}
                        {e.date ? <time dateTime={e.date} className="ml-auto text-xs text-fg-3">{shortDate(e.date)}</time> : null}
                        {e.version === current ? <Badge tone="info" size="sm">Installed</Badge> : null}
                      </>}>
                      <div className="max-w-prose px-3 pb-4 pt-2"><Markdown source={e.body} /></div>
                    </Disclosure>
                  ))}
                </div>
              </Disclosure>
            ))}
          </div>
        </div>
      ) : <div className="panel"><EmptyState icon={<Newspaper size={20} />} title="No release notes yet" description="Notes appear here as versions are released." /></div>}
    </main>
  );
}
