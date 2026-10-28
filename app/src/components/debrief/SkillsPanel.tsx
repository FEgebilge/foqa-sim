import type { CSSProperties, ReactNode } from "react";
import { Activity, Fuel, Gauge, Info, Plane, Waves } from "lucide-react";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { duration, number, phaseName } from "../../lib/format";
import { BANK_LIMIT_DEG, RIDE_BANDS, ROTATION_RATE_DPS, percent, rideStepOf } from "../../lib/skills";
import type { Skills } from "../../types";
import { Measure } from "../Measure";
import { Term } from "../Term";

const dot = (phase: string): CSSProperties => ({ background: `var(--phase-${phase}, var(--phase-unknown))` });
const allNull = (...values: unknown[]) => values.every((v) => v === null || v === undefined);
const share = (part: number | null, total: number | null) => (part === null || !total ? null : part / total);
const AT_COLOR = "color-mix(in oklab, var(--phase-Approach) 85%, var(--surface))";

/**
 * Flying skills (docs/skills.md): how the flight was flown, in four short sections. These are measures, not a grade:
 * turbulence, the aircraft and ATC move every one, and more hand-flying is not by itself better flying.
 */
export function SkillsPanel({ skills }: { skills: Skills }) {
  const { automation: a, ride: r, fuel: f, control: c } = skills;
  const sections: ReactNode[] = [];
  if (!allNull(a.airborne_s, a.ap_s, a.at_s, a.ap_off_ft, a.manual_final_s) || a.by_phase.length)
    sections.push(<Section key="auto" icon={<Plane size={14} />} title="Automation" means="How much of the flight the autopilot and autothrottle flew, and where you took over on the approach." source="EASA EOFDM precursor guidance, LOC29: autopilot and autothrottle engagement, disconnect height, time to manual input."><AutomationBody a={a} /></Section>);
  if (!allNull(r.vertical_rms, r.lateral_rms, r.sideslip_rms_deg, r.rotation_rate_dps) || Object.keys(r.max_bank_deg).length)
    sections.push(<Section key="ride" icon={<Waves size={14} />} title="Ride quality" means="How smooth the flight would feel in the cabin, and how gently it was banked and rotated." source="A band-limited comfort proxy, not ISO 2631 compliance: normal load is sampled at 8 Hz and lateral at 4 Hz, not the 0.5–80 Hz seat measurement. Bands: ISO 2631-1 Annex C. The 30° bank line is an estimate (common airline SOP). Rotation: Airbus, “What about rotation technique”, 2020."><RideBody r={r} /></Section>);
  if (!allNull(f.fuel_used_kg, f.fuel_kg_h, f.fuel_kg_nm, f.taxi_fuel_kg, f.descent_level_s, f.descent_levels))
    sections.push(<Section key="fuel" icon={<Fuel size={14} />} title="Fuel and descent" means="What the flight burned, and how much of the descent was flown level instead of down." source="ICAO A39-WP/431 §4.6 for the recorded fuel figures (no plan is recorded, so nothing is compared with planned fuel). Level-offs: EUROCONTROL Vertical Flight Efficiency note §3.1, within ±300 fpm for 10 s or more."><FuelBody f={f} /></Section>);
  if (!allNull(c.manual_s, c.reversals_per_min?.pitch, c.reversals_per_min?.roll))
    sections.push(<Section key="ctl" icon={<Activity size={14} />} title="Control activity" means="How often the controls changed direction while you hand-flew. A busy stick is not wrong in turbulence." source="NASA NTRS 20140005730, eq. 2: direction changes above 0.5 % of full travel, per hand-flown minute. A detection setting, not a limit."><ControlBody c={c} /></Section>);
  if (!sections.length) return null;

  return (
    <section className="panel flex flex-col overflow-hidden" aria-labelledby="skills-title">
      <header className="flex flex-wrap items-baseline gap-x-4 gap-y-1 border-b border-line-subtle px-4 py-2.5">
        <h2 id="skills-title" className="eyebrow flex items-center gap-1.5"><Gauge size={13} aria-hidden="true" />Flying skills</h2>
        <p className="min-w-0 text-xs text-fg-2">Measures, not a grade. Turbulence, the aircraft and ATC move every number here, and more hand-flying is not by itself better flying (FAA SAFO 13002).</p>
      </header>
      <div className="grid gap-x-8 gap-y-6 p-4 [grid-template-columns:repeat(auto-fit,minmax(min(100%,270px),1fr))]">{sections}</div>
    </section>
  );
}

function Section({ icon, title, means, source, children }: { icon: ReactNode; title: string; means: string; source: string; children: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-3">
      <div>
        <h3 className="flex items-center gap-1.5 font-display text-sm font-semibold"><span className="text-accent" aria-hidden="true">{icon}</span>{title}
          <Tooltip content={<span className="block max-w-[340px] text-xs">{source}</span>}><button type="button" className="text-fg-3 hover:text-fg" aria-label={`Source for ${title}`}><Info size={13} aria-hidden="true" /></button></Tooltip>
        </h3>
        <p className="mt-0.5 text-xs text-fg-3">{means}</p>
      </div>
      {children}
    </div>
  );
}

function Stat({ label, value, unit, tip, className = "", children }: { label: ReactNode; value: unknown; unit?: ReactNode; tip?: string; className?: string; children: ReactNode }) {
  return (
    <div className={`min-w-0 ${className}`}>
      <dt className="text-[11px] text-fg-3">{label}</dt>
      <dd className="readout text-lg leading-tight"><Measure value={value} missing={tip}>{children}{unit ? <span className="ml-1 text-xs font-normal text-fg-3">{unit}</span> : null}</Measure></dd>
    </div>
  );
}

const Stats = ({ children }: { children: ReactNode }) => <dl className="grid grid-cols-2 gap-x-4 gap-y-2.5">{children}</dl>;

function AutomationBody({ a }: { a: Skills["automation"] }) {
  const ap = share(a.ap_s, a.airborne_s), at = share(a.at_s, a.airborne_s);
  const total = a.by_phase.reduce((sum, p) => sum + p.s, 0) || 1;
  return (
    <>
      <Stats>
        <Stat label="Autopilot engaged" value={ap}>{percent(ap ?? 0)}</Stat>
        <Stat label="Autothrottle engaged" value={at}>{percent(at ?? 0)}</Stat>
        <Stat label="Autopilot off at" value={a.autoland ? 0 : a.ap_off_ft} tip="not recorded, or the autopilot was not on in the final approach" unit={a.autoland ? undefined : <>ft <Term id="ra">RA</Term></>}>{a.autoland ? <Term id="autoland">autoland</Term> : number(a.ap_off_ft ?? 0)}</Stat>
        <Stat label="Hand-flown final" value={a.manual_final_s}>{duration(a.manual_final_s ?? 0)}</Stat>
      </Stats>
      {a.by_phase.length ? (
        <div className="flex flex-col gap-2">
          <div className="flex h-8 gap-px" role="img" aria-label={`Autopilot share by phase: ${a.by_phase.map((p) => `${phaseName(p.phase)} ${p.ap_s === null ? "not recorded" : percent(share(p.ap_s, p.s) ?? 0)}`).join(", ")}`}>
            {a.by_phase.map((p, i) => (
              <Tooltip key={`${p.phase}-${i}`} content={`${phaseName(p.phase)}, ${duration(p.s)}: autopilot ${p.ap_s === null ? "not recorded" : percent(share(p.ap_s, p.s) ?? 0)}, autothrottle ${p.at_s === null ? "not recorded" : percent(share(p.at_s, p.s) ?? 0)}`}>
                <span className="flex min-w-[3px] cursor-default flex-col gap-px" style={{ flexGrow: p.s / total, flexBasis: 0 }} tabIndex={0}>
                  <Track fill={share(p.ap_s, p.s)} color="var(--accent)" />
                  <Track fill={share(p.at_s, p.s)} color={AT_COLOR} />
                </span>
              </Tooltip>
            ))}
          </div>
          <ul className="flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-fg-2">
            {a.by_phase.map((p, i) => <li key={`${p.phase}-${i}`} className="flex items-center gap-1"><span className="size-2 rounded-full" style={dot(p.phase)} aria-hidden="true" />{phaseName(p.phase)}</li>)}
            <li className="flex items-center gap-1.5 text-fg-3 sm:ml-auto"><i className="inline-block h-1.5 w-3 rounded-sm bg-accent" aria-hidden="true" />AP<i className="inline-block h-1.5 w-3 rounded-sm" style={{ background: AT_COLOR }} aria-hidden="true" />AT</li>
          </ul>
        </div>
      ) : null}
    </>
  );
}

/** A thin bar filled to a share; a dashed empty track when the share is not recorded. */
function Track({ fill, color }: { fill: number | null; color: string }) {
  return (
    <span className={`relative block flex-1 overflow-hidden rounded-[2px] ${fill === null ? "border border-dashed border-line" : "bg-muted"}`}>
      {fill ? <span className="absolute inset-y-0 left-0" style={{ width: `${fill * 100}%`, background: color }} /> : null}
    </span>
  );
}

function RideBody({ r }: { r: Skills["ride"] }) {
  const step = rideStepOf(r.ride_band);
  const banks = Object.entries(r.max_bank_deg);
  const bankMax = Math.max(45, ...banks.map(([, v]) => v * 1.1));
  return (
    <>
      <div className="flex flex-col gap-1.5">
        <div className="flex flex-wrap items-baseline justify-between gap-x-2 gap-y-0.5 text-xs">
          <span className="text-fg-2">Vertical ride{" "}
            <Tooltip content="A band-limited comfort proxy, not an ISO 2631 measurement."><span className="cursor-help rounded-sm border border-dashed border-line px-1 text-[10px] text-fg-3" tabIndex={0}>comfort proxy</span></Tooltip>
          </span>
          <Measure value={r.vertical_rms}><span className="readout"><b className="font-semibold">{number(r.vertical_rms ?? 0, 2)}</b> <span className="text-fg-3">m/s² <Term id="rms">RMS</Term>{step >= 0 ? ` · ${RIDE_BANDS[step]!.band}` : ""}</span></span></Measure>
        </div>
        <ol className="grid grid-cols-6 gap-px" aria-label="ISO 2631-1 comfort bands">
          {RIDE_BANDS.map((b, i) => (
            <li key={b.band} className="flex min-w-0 flex-col items-center gap-0.5" aria-current={i === step || undefined} title={`${b.band}: ${b.range} m/s²`}>
              <span className="h-2 w-full rounded-[2px]" style={{ background: b.color, opacity: step < 0 || i === step ? 1 : 0.28 }} />
              <span className={`h-1.5 text-[9px] leading-none ${i === step ? "text-fg" : "text-transparent"}`} aria-hidden="true">▲</span>
              <span className={`w-full truncate text-center text-[10px] ${i === step ? "font-semibold text-fg" : "text-fg-3"}`}>{b.short}</span>
            </li>
          ))}
        </ol>
      </div>
      <Stats>
        <Stat label={<>Lateral load <Term id="rms">RMS</Term></>} value={r.lateral_rms} unit="m/s²">{number(r.lateral_rms ?? 0, 2)}</Stat>
        <Stat label={<>Sideslip <Term id="rms">RMS</Term></>} value={r.sideslip_rms_deg} unit="°" tip="not recorded (measured below 10,000 ft, outside the rolls)">{number(r.sideslip_rms_deg ?? 0, 1)}</Stat>
      </Stats>
      <div className="flex flex-col gap-1">
        <div className="text-[11px] text-fg-3">Peak bank per phase · line at {BANK_LIMIT_DEG}° (estimate)</div>
        {banks.length
          ? banks.map(([phase, deg]) => <RefBar key={phase} label={phaseName(phase)} swatch={dot(phase)} value={deg} max={bankMax} at={BANK_LIMIT_DEG} unit="°" over={deg > BANK_LIMIT_DEG} />)
          : <span className="text-sm"><Measure value={null}>{null}</Measure></span>}
      </div>
      <div className="flex flex-col gap-1">
        <div className="text-[11px] text-fg-3">Rotation rate · line at ~{ROTATION_RATE_DPS}°/s (transport class)</div>
        {r.rotation_rate_dps === null
          ? <span className="text-sm"><Measure value={null} missing="not recorded (measured on transport class only)">{null}</Measure></span>
          : <RefBar label="Peak, to 35 ft" value={r.rotation_rate_dps} max={6} at={ROTATION_RATE_DPS} unit="°/s" over={r.rotation_rate_dps > ROTATION_RATE_DPS * 1.5} />}
      </div>
    </>
  );
}

/** A value on a bar with a reference tick; `over` shows it in the caution colour, never as a verdict. */
function RefBar({ label, swatch, value, max, at, unit, over }: { label: string; swatch?: CSSProperties; value: number; max: number; at: number; unit: string; over: boolean }) {
  return (
    <div className="grid grid-cols-[minmax(5.5rem,7rem)_minmax(0,1fr)_3.75rem] items-center gap-x-2 text-xs">
      <span className="flex min-w-0 items-center gap-1.5 truncate text-fg-2">{swatch ? <span className="size-2 shrink-0 rounded-full" style={swatch} aria-hidden="true" /> : null}{label}</span>
      <span className="relative h-2 rounded-[2px] bg-muted" aria-hidden="true">
        <span className="absolute inset-y-0 left-0 rounded-[2px]" style={{ width: `${Math.min(100, (value / max) * 100)}%`, background: over ? "var(--level-medium)" : "var(--accent)" }} />
        <span className="absolute -inset-y-0.5 w-px bg-fg-2" style={{ left: `${(at / max) * 100}%` }} />
      </span>
      <span className="readout text-right" style={over ? { color: "var(--level-medium)" } : undefined}>{number(value, 1)}{unit}</span>
    </div>
  );
}

function FuelBody({ f }: { f: Skills["fuel"] }) {
  const levels = f.descent_levels === null ? "" : `in ${f.descent_levels} ${f.descent_levels === 1 ? "level segment" : "level segments"}`;
  return (
    <Stats>
      <Stat label="Fuel used" value={f.fuel_used_kg} unit="kg" tip="not recorded, or fuel rose during the flight (refuelled or slewed)">{number(f.fuel_used_kg ?? 0)}</Stat>
      <Stat label="Burn rate" value={f.fuel_kg_h} unit="kg/h">{number(f.fuel_kg_h ?? 0)}</Stat>
      <Stat label="Per distance" value={f.fuel_kg_nm} unit="kg/nm">{number(f.fuel_kg_nm ?? 0, 1)}</Stat>
      <Stat label="Taxi fuel" value={f.taxi_fuel_kg} unit="kg">{number(f.taxi_fuel_kg ?? 0)}</Stat>
      <Stat className="col-span-2" label="Level-offs in descent" value={f.descent_level_s} unit={levels} tip="not recorded, or the flight has no descent">{duration(f.descent_level_s ?? 0)}</Stat>
    </Stats>
  );
}

function ControlBody({ c }: { c: Skills["control"] }) {
  const rev = c.reversals_per_min;
  return (
    <Stats>
      <Stat label="Pitch reversals" value={rev?.pitch} unit="/min">{number(rev?.pitch ?? 0, 1)}</Stat>
      <Stat label="Roll reversals" value={rev?.roll} unit="/min">{number(rev?.roll ?? 0, 1)}</Stat>
      <Stat className="col-span-2" label="Hand-flown airborne" value={c.manual_s}>{duration(c.manual_s ?? 0)}</Stat>
    </Stats>
  );
}
