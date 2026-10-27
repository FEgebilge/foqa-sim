import { Badge } from "@ui/components/badge/badge";
import type { DevlogEntry } from "../../lib/devlog";
import { Markdown } from "../../lib/markdown";

const dateLabel = (iso: string) => {
  const d = new Date(`${iso}T00:00:00`);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" });
};

export function DevlogEntryView({ entry, installed }: { entry: DevlogEntry; installed?: boolean }) {
  return (
    <article className="flex flex-col gap-3">
      <header className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <h2 className="font-display text-lg font-semibold tracking-tight"><span className="readout">v{entry.version}</span>{entry.title ? <span className="text-fg-2"> · {entry.title}</span> : null}</h2>
        {entry.date ? <time dateTime={entry.date} className="text-xs text-fg-3">{dateLabel(entry.date)}</time> : null}
        {installed ? <Badge tone="info" size="sm">Installed</Badge> : null}
      </header>
      <Markdown source={entry.body} />
    </article>
  );
}
