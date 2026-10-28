import { Tooltip } from "@ui/components/tooltip/tooltip";
import { number } from "../../lib/format";
import { GRADE_VAR, levelTone } from "../../lib/trends";
import type { TrendDomain } from "../../types";

/** The six landing areas: bar length is the mean level (0 clean to 3 worst), the line below says how often one was at level 2 or worse. */
export function DomainBars({ domains }: { domains: TrendDomain[] }) {
  return (
    <ul className="flex flex-col gap-3.5 p-4">
      {domains.map((d) => {
        const share = d.graded ? d.level_2_plus / d.graded : null;
        return (
          <li key={d.id} className="grid grid-cols-[minmax(4.5rem,7rem)_minmax(0,1fr)_3rem] items-center gap-x-3 gap-y-1">
            <Tooltip content={d.title}><span className="cursor-default truncate text-sm font-medium">{d.short}</span></Tooltip>
            <div className="h-2.5 overflow-hidden rounded-full bg-muted" role="img" aria-label={d.mean_level === null ? `${d.title}: not graded` : `${d.title}: mean level ${number(d.mean_level, 1)} of 3`}>
              {d.mean_level !== null ? <div className="h-full rounded-full" style={{ width: `${Math.max(2, (d.mean_level / 3) * 100)}%`, background: `var(${GRADE_VAR[levelTone(d.mean_level)]})` }} /> : null}
            </div>
            <span className="readout text-right text-sm">{d.mean_level === null ? "–" : number(d.mean_level, 1)}</span>
            <span className="col-start-2 col-end-4 text-[11px] text-fg-3">
              {share === null ? "Not graded yet" : <><span className="readout text-fg-2">{Math.round(share * 100)}%</span> at level 2+ · {d.level_2_plus} of {d.graded} landings</>}
            </span>
          </li>
        );
      })}
    </ul>
  );
}
