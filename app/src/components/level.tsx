import type { CSSProperties } from "react";
import { Badge } from "@ui/components/badge/badge";
import { LEVEL_LABEL, LEVELS } from "../lib/format";
import { levelVar } from "../lib/palette";
import type { Level } from "../types";

const levelStyle = (level: Level) => ({
  "--badge-color": levelVar(level),
  "--badge-foreground": levelVar(level),
  "--badge-background": `color-mix(in oklab, ${levelVar(level)} 12%, var(--surface))`,
  "--badge-border": `color-mix(in oklab, ${levelVar(level)} 34%, var(--border))`,
}) as CSSProperties;

/** Event severity as an annunciator-coloured pill; no level reads as "Clean". */
export function LevelBadge({ level, size = "sm" }: { level?: Level; size?: "sm" | "md" }) {
  if (!level) return <Badge size={size} tone="neutral" animated={false}>Clean</Badge>;
  return <Badge size={size} animated={false} style={levelStyle(level)}>{LEVEL_LABEL[level]}</Badge>;
}

/** Per-level counts as small coloured tallies, most severe first; zeros are left out. */
export function EventCounts({ counts }: { counts: Record<Level, number> }) {
  const present = LEVELS.filter((level) => counts[level] > 0);
  if (!present.length) return <span className="text-xs text-fg-3">No events</span>;
  return (
    <span className="inline-flex items-center gap-2.5" aria-label={present.map((level) => `${counts[level]} ${LEVEL_LABEL[level].toLowerCase()}`).join(", ")}>
      {present.map((level) => (
        <span key={level} className="readout inline-flex items-center gap-1 text-xs" title={`${LEVEL_LABEL[level]} events`}>
          <span className="size-2 rounded-[2px]" style={{ background: levelVar(level) }} aria-hidden="true" />
          {counts[level]}
        </span>
      ))}
    </span>
  );
}
