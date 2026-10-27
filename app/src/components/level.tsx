import type { CSSProperties } from "react";
import { Badge } from "@ui/components/badge/badge";
import { assessmentText, GRADE_LABEL, LEVEL_LABEL, LEVELS, number } from "../lib/format";

import { levelVar } from "../lib/palette";
import type { Assessment, Grade, Level } from "../types";

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

const GRADE_TONE = { good: "success", fair: "warning", poor: "danger" } as const satisfies Record<Grade, string>;

/** The flight's grade with its landing-2 score beside it (the points of a flight without a scored landing); the tooltip says what it is. */
export function GradeBadge({ assessment, size = "sm" }: { assessment: Assessment; size?: "sm" | "md" }) {
  const scored = assessment.score !== undefined;
  return (
    <span className="inline-flex items-center gap-1.5" title={assessmentText(assessment)}>
      <Badge size={size} tone={GRADE_TONE[assessment.grade]} animated={false}>{GRADE_LABEL[assessment.grade]}</Badge>
      <span className="readout text-xs text-fg-3">
        {scored ? number(assessment.score!, 1) : `${assessment.points} pt${assessment.points === 1 ? "" : "s"}`}
        {assessment.confidence === "partial" ? <span title="Partial: some expected data was missing"> ·p</span> : null}
      </span>
    </span>
  );
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

/** A grade as a coloured pill; `label` replaces the grade's name. */
export const GradeTag = ({ grade, size = "sm", label }: { grade: Grade; size?: "sm" | "md"; label?: string }) =>
  <Badge size={size} tone={GRADE_TONE[grade]} animated={false}>{label ?? GRADE_LABEL[grade]}</Badge>;
