import { Info } from "lucide-react";
import { clock, phaseName } from "../../lib/format";
import { phaseRules } from "../../lib/phases";
import type { Debrief, PhaseSpan } from "../../types";

const CICTT = "https://www.intlaviationstandards.org/Documents/PhaseofFlightDefinitions.pdf";

/** How the phases were found, behind a small disclosure: each phase's start and end in this flight's values, and where those came from. */
export function PhasesPanel({ phases, values }: { phases: PhaseSpan[]; values: Debrief["flight"]["phase_values"] }) {
  const rules = phaseRules(phases, values);
  if (!rules.length) return null;
  return (
    <details className="text-xs text-fg-2">
      <summary className="inline-flex cursor-pointer list-none items-center gap-1 text-fg-3 hover:text-fg"><Info size={13} aria-hidden="true" />How phases are found</summary>
      <div className="mt-2 flex flex-col gap-3 rounded-[var(--radius-control)] border border-line-subtle bg-bg p-3">
        <p className="max-w-3xl">
          Phases follow the ICAO/CAST <a href={CICTT} target="_blank" rel="noreferrer" className="underline hover:text-fg">CICTT</a> definitions, measured from the recorded data. They matter because each rule only looks inside its own phases, so a boundary moves what gets found.
        </p>
        <div className="overflow-x-auto">
          <table className="w-full min-w-[640px] text-left">
            <thead className="text-fg-3">
              <tr><th className="pb-1 pr-4 font-normal">Phase</th><th className="pb-1 pr-4 font-normal">Starts when</th><th className="pb-1 pr-4 font-normal">Ends when</th><th className="pb-1 pr-4 font-normal">Values from</th><th className="pb-1 text-right font-normal">This flight</th></tr>
            </thead>
            <tbody>
              {rules.map((r) => (
                <tr key={r.phase} className="border-t border-line-subtle align-top">
                  <td className="py-1.5 pr-4 whitespace-nowrap text-fg">
                    <span className="mr-1.5 inline-block size-2 rounded-full" style={{ background: `var(--phase-${r.phase}, var(--phase-unknown))` }} aria-hidden="true" />{phaseName(r.phase)}
                  </td>
                  <td className="py-1.5 pr-4">{r.starts}</td>
                  <td className="py-1.5 pr-4">{r.ends}</td>
                  <td className="py-1.5 pr-4 text-fg-3">{r.sources.length ? r.sources.join("; ") : "fixed"}</td>
                  <td className="readout py-1.5 text-right whitespace-nowrap text-fg-3">{r.seen.map((p) => `${clock(p.start_s)}–${clock(p.end_s)}`).join(", ")}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </details>
  );
}
