import type { ReactNode } from "react";
import { Gauge } from "lucide-react";
import { EmptyState } from "@ui/components/empty-state/empty-state";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { duration, fuelUnit, fuelValue, number } from "../../lib/format";
import { RIDE_BANDS, percent, rideStep } from "../../lib/skills";
import type { Trends } from "../../types";
import { Measure } from "../Measure";
import { Panel } from "./Panel";

type Skills = NonNullable<Trends["skills"]>;

const Tile = ({ label, value, tip, note, children }: { label: string; value: unknown; tip: string; note?: ReactNode; children: ReactNode }) => (
  <div className="bg-surface px-4 py-3">
    <dt className="eyebrow">{label}</dt>
    <dd className="mt-1 flex flex-wrap items-baseline gap-x-2 gap-y-0.5"><span className="readout text-xl"><Measure value={value} missing={tip}>{children}</Measure></span>{value == null ? null : note}</dd>
  </div>
);

/** The "Flying" row: how the matching flights were flown, as medians and shares rather than a score (docs/skills.md). */
export function SkillsRow({ skills }: { skills: Trends["skills"] }) {
  const empty = !skills || (skills.ap_share === null && skills.ap_off_ft_median === null && skills.vertical_rms_median === null && skills.descent_level_s_median === null && skills.fuel_kg_h.every((f) => f.median === null));
  return (
    <Panel title="Flying" why="How your flights were flown, as measures rather than a score: turbulence, the aircraft and ATC move every one, and more hand-flying is not by itself better flying.">
      {empty ? <EmptyState size="sm" icon={<Gauge size={18} />} title="No flying measures yet" description="They come from autopilot, ride, fuel and control data in the recordings; flights without those parameters leave them out instead of counting zero." /> : <Body skills={skills} />}
    </Panel>
  );
}

function Body({ skills }: { skills: Skills }) {
  const step = skills.vertical_rms_median === null ? -1 : rideStep(skills.vertical_rms_median);
  const band = step < 0 ? undefined : RIDE_BANDS[step]!;
  const fuel = skills.fuel_kg_h;
  return (
    <div className="grid gap-px bg-line-subtle lg:grid-cols-[minmax(0,3fr)_minmax(260px,1fr)]">
      <dl className="grid grid-cols-2 gap-px bg-line-subtle sm:grid-cols-4 lg:grid-cols-2 2xl:grid-cols-4">
        <Tile label="Autopilot share" value={skills.ap_share} tip="not recorded" note={<span className="text-xs text-fg-3">of airborne time</span>}>{percent(skills.ap_share ?? 0)}</Tile>
        <Tile label="Autopilot off at" value={skills.ap_off_ft_median} tip="not recorded, or no hand-flown landing with the autopilot on earlier in the approach" note={<span className="text-xs text-fg-3">ft RA, median of hand-flown landings{skills.autolands ? ` · ${skills.autolands} autoland${skills.autolands > 1 ? "s" : ""}` : ""}</span>}>{number(skills.ap_off_ft_median ?? 0)}</Tile>
        <Tile label="Ride, vertical RMS" value={skills.vertical_rms_median} tip="not recorded"
          note={band ? <Tooltip content={`Comfort proxy, not ISO 2631 compliance. ${band.band}, ${band.range} m/s².`}><span className="cursor-help text-xs" style={{ color: band.color }} tabIndex={0}>{band.band}</span></Tooltip> : undefined}>
          {number(skills.vertical_rms_median ?? 0, 2)}<span className="ml-1 text-xs font-normal text-fg-3">m/s²</span>
        </Tile>
        <Tile label="Descent level-offs" value={skills.descent_level_s_median} tip="not recorded, or no flight had a descent" note={<span className="text-xs text-fg-3">median</span>}>{duration(skills.descent_level_s_median ?? 0)}</Tile>
      </dl>
      <div className="bg-surface px-4 py-3">
        <h3 className="eyebrow">Fuel burn by type, median</h3>
        {fuel.length ? (
          <table className="mt-1.5 w-full text-sm">
            <tbody>
              {fuel.map((f) => (
                <tr key={f.icao} className="border-t border-line-subtle first:border-t-0">
                  <th scope="row" className="py-1 text-left font-normal text-fg-2">{f.icao}</th>
                  <td className="readout py-1 text-right"><Measure value={f.median}>{fuelValue(f.median ?? 0)}<span className="ml-1 text-xs text-fg-3">{fuelUnit("h")}</span></Measure></td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : <p className="mt-1.5 text-xs text-fg-3">No fuel recorded.</p>}
        <p className="mt-1.5 text-[11px] text-fg-3">Per type only: different aircraft are not comparable.</p>
      </div>
    </div>
  );
}
