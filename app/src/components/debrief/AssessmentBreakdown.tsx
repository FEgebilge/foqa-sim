import { Info } from "lucide-react";
import { GRADE_LABEL } from "../../lib/format";
import type { AssessmentDetail, Grade } from "../../types";
import { LevelBadge } from "../level";

const GRADE_BG = { good: "bg-success/30", fair: "bg-warning/30", poor: "bg-danger/30" } as const satisfies Record<Grade, string>;
const pts = (n: number) => `${n} pt${n === 1 ? "" : "s"}`;

/** The rule in words, from the data: points per level (the parts show them) and where each grade ends. */
function rule({ parts, bands }: AssessmentDetail) {
  const levels = (["high", "medium", "low"] as const).flatMap((l) => { const p = parts.find((x) => x.level === l); return p ? [`${l} ${p.points}`] : []; }).join(", ");
  const grades = bands.map((b, i) => `${GRADE_LABEL[b.grade]} ${b.up_to === undefined ? `from ${(bands[i - 1]?.up_to ?? 0) + 1}` : `up to ${b.up_to}`}`).join(", ");
  return `Each rule counts once, at its worst level${levels ? `: ${levels}` : ""}. ${grades}.`;
}

/** The points on the grade scale: a band per grade, sized by its reach, and a marker where this flight landed. */
function Scale({ bands, points }: { bands: AssessmentDetail["bands"]; points: number }) {
  const max = Math.max(points, (bands.findLast((b) => b.up_to !== undefined)?.up_to ?? 0) + 2);
  const edges = bands.map((b) => b.up_to ?? max);
  return (
    <div className="relative pt-3" role="img" aria-label={`${pts(points)} on a scale of ${max}`}>
      <div className="flex h-2 gap-px overflow-hidden rounded-full">
        {bands.map((b, i) => <span key={b.grade} className={GRADE_BG[b.grade]} style={{ width: `${((edges[i] - (edges[i - 1] ?? 0)) / max) * 100}%` }} />)}
      </div>
      <div className="mt-1.5 flex flex-wrap gap-x-3 text-[10px] text-fg-3">
        {bands.map((b, i) => (
          <span key={b.grade} className="flex items-center gap-1 whitespace-nowrap">
            <span className={`size-1.5 rounded-full ${GRADE_BG[b.grade]}`} />
            {GRADE_LABEL[b.grade]}{b.up_to === undefined ? ` ${(bands[i - 1]?.up_to ?? 0) + 1}+` : ` ≤${b.up_to}`}
          </span>
        ))}
      </div>
      <span className="readout absolute top-0 -translate-x-1/2 text-[10px] font-medium text-fg" style={{ left: `${(points / max) * 100}%` }}>▼</span>
    </div>
  );
}

/** How the flight's grade came about, behind a small disclosure: the rule and scale beside each rule's worst level and points, across the header's width (stacked when narrow). */
export function AssessmentBreakdown({ detail, points }: { detail: AssessmentDetail; points: number }) {
  return (
    <details className="text-xs text-fg-2">
      <summary className="inline-flex cursor-pointer list-none items-center gap-1 text-fg-3 hover:text-fg"><Info size={13} aria-hidden="true" />How this was graded</summary>
      <div className="mt-2 grid gap-x-8 gap-y-3 rounded-[var(--radius-control)] border border-line-subtle bg-bg p-3 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
        <div className="flex flex-col gap-3">
          <p>{rule(detail)}</p>
          <Scale bands={detail.bands} points={points} />
        </div>
        {detail.parts.length ? (
          <table className="w-full self-start">
            <tbody>
              {detail.parts.map((p) => (
                <tr key={p.rule} className={`border-t border-line-subtle ${p.points ? "" : "text-fg-3"}`}>
                  <td className="py-1 pr-3">{p.title}</td>
                  <td className="py-1 pr-3"><LevelBadge level={p.level} /></td>
                  <td className="readout py-1 pr-3 text-right">×{p.count}</td>
                  <td className="readout py-1 text-right">{pts(p.points)}</td>
                </tr>
              ))}
            </tbody>
            <tfoot><tr className="border-t border-line text-fg"><td className="py-1 font-medium" colSpan={3}>Total</td><td className="readout py-1 text-right font-medium">{pts(points)}</td></tr></tfoot>
          </table>
        ) : <p className="text-fg-3">No rule fired.</p>}
      </div>
    </details>
  );
}
