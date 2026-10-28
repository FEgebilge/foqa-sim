import { useMemo, type ReactNode } from "react";
import { ArrowDownRight, ArrowRight, ArrowUpRight, Trophy } from "lucide-react";
import { Button } from "@ui/components/button/button";
import { flightDate, flightTimeUtc, lengthM, measured, number } from "../../lib/format";
import { flightHref } from "../../lib/route";
import { direction, GRADE_VAR } from "../../lib/trends";
import type { FlightSummary } from "../../types";
import { EventCounts, GradeTag } from "../level";
import { byNewest, MIN_COMPARE, ordinal, personal, scoredLanding, typeOf, type Compare } from "./latest";

/** The most recent counted flight, against your own recent flights: the reason to open the app after every landing. */
export function LatestFlight({ flights }: { flights: FlightSummary[] }) {
  const ordered = useMemo(() => byNewest(flights), [flights]);
  const context = useMemo(() => personal(ordered), [ordered]);
  const latest = ordered[0];
  if (!latest || !context) return null;

  const { assessment, aircraft, departure, arrival } = latest;
  const type = typeOf(latest);
  const landing = scoredLanding(latest);
  const events = Object.values(latest.event_counts).reduce((a, b) => a + b, 0);
  const route = departure && arrival ? (departure === arrival ? `${departure} local` : `${departure} → ${arrival}`) : departure ?? arrival ?? "No airport";

  return (
    <section className="panel grid gap-x-8 gap-y-5 p-5 md:grid-cols-2 xl:grid-cols-[minmax(0,1fr)_minmax(0,1.5fr)_minmax(0,1fr)]" aria-labelledby="latest-title">
      <div className="flex min-w-0 flex-col gap-3">
        <div>
          <p className="eyebrow">Latest flight · {flightDate(latest.started_utc)} {flightTimeUtc(latest.started_utc)}</p>
          <h2 id="latest-title" className="mt-1 truncate font-display text-xl font-semibold tracking-tight">{route}</h2>
          <p className="truncate text-sm text-fg-2" title={aircraft?.title}>{type ?? "Unknown type"}{aircraft?.title && aircraft.title !== type ? ` · ${aircraft.title}` : ""}</p>
        </div>
        <div className="flex items-end gap-3">
          {assessment.score !== undefined ? (
            <span className="readout text-4xl leading-none font-semibold">{number(assessment.score, 1)}<span className="ml-1 text-sm font-normal text-fg-3">/ 100</span></span>
          ) : <span className="text-sm text-fg-2">No scored landing</span>}
          <GradeTag grade={assessment.grade} size="md" />
        </div>
        <div className="flex flex-col gap-1 text-sm">
          {context.type ? <Delta compare={context.type} digits={1} unit="" higherIsBetter label={`your last ${context.type.n} ${type}`} /> : null}
          {context.overall && (context.overall.n !== context.type?.n || context.overall.mean !== context.type.mean) ? <Delta compare={context.overall} digits={1} unit="" higherIsBetter label={`your last ${context.overall.n} in any type`} /> : null}
          {context.rank && context.rank.of > 1 ? (
            <p className="flex items-center gap-1.5 text-fg-2">
              {context.rank.place === 1 ? <Trophy size={14} className="text-accent" aria-hidden="true" /> : null}
              {context.rank.place === 1 ? `Your best ${type} landing of ${context.rank.of}` : `${ordinal(context.rank.place)} best of ${context.rank.of} in the ${type}`}
            </p>
          ) : !context.type && assessment.score !== undefined ? <p className="text-fg-3">Comparisons start once you have {MIN_COMPARE} earlier scored {type ?? ""} flights.</p> : null}
        </div>
      </div>

      <dl className="grid min-w-0 grid-cols-2 content-start gap-x-6 gap-y-4 lg:grid-cols-3">
        {landing ? (
          <>
            <Figure label="Sink rate" value={measured(landing.sink_fpm, "fpm")}>{context.sink ? <Delta compare={context.sink} digits={0} unit=" fpm" label="your mean" small /> : null}</Figure>
            <Figure label="Peak load" value={measured(landing.peak_g, "g")}>{context.g ? <Delta compare={context.g} digits={2} unit=" g" label="your mean" small /> : null}</Figure>
            {landing.along_m !== undefined ? <Figure label="Touchdown" value={lengthM(landing.along_m)}>{landing.zone ? <span className="text-xs text-fg-3">past the threshold · {landing.zone}</span> : null}</Figure> : null}
            {landing.cross_m !== undefined ? <Figure label="Centreline" value={landing.cross_m === 0 ? lengthM(0) : `${lengthM(Math.abs(landing.cross_m), 1)} ${landing.cross_m > 0 ? "R" : "L"}`} /> : null}
            <Figure label="Bounces" value={number(landing.bounces)} />
          </>
        ) : <Figure label="Landings" value={number(latest.landings)} />}
        <Figure label="Events" value={number(events)}><EventCounts counts={latest.event_counts} /></Figure>
      </dl>

      <div className="flex min-w-0 flex-col justify-between gap-4 md:col-span-2 xl:col-span-1">
        {context.spark.length > 1 ? (
          <figure className="flex flex-col gap-1.5">
            <figcaption className="eyebrow">Your last {context.spark.length} {type} scores</figcaption>
            <Sparkline values={context.spark} mean={context.type?.mean} grade={assessment.grade} />
          </figure>
        ) : <span />}
        <Button href={flightHref(latest.id)} className="self-start xl:self-end">Open debrief<ArrowRight size={15} aria-hidden="true" /></Button>
      </div>
    </section>
  );
}

const Figure = ({ label, value, children }: { label: string; value: string; children?: ReactNode }) => (
  <div className="min-w-0">
    <dt className="eyebrow">{label}</dt>
    <dd className="readout mt-1 text-lg">{value}</dd>
    {children ? <dd className="mt-0.5">{children}</dd> : null}
  </div>
);

/** "▲ +4.2 vs your last 10 C172": the arrow carries better or worse in colour; the text stays in text colour. */
function Delta({ compare, digits, unit, label, higherIsBetter = false, small }: { compare: Compare; digits: number; unit: string; label: string; higherIsBetter?: boolean; small?: boolean }) {
  const shown = number(compare.delta, digits);
  const zero = Number(shown) === 0;
  const verdict = zero ? "same" : direction(compare.delta, higherIsBetter, 0);
  const Icon = verdict === "same" ? ArrowRight : compare.delta > 0 ? ArrowUpRight : ArrowDownRight;
  const color = verdict === "better" ? "var(--success)" : verdict === "worse" ? "var(--danger)" : "var(--text-muted)";
  return (
    <p className={`flex flex-wrap items-center gap-x-1 ${small ? "text-xs text-fg-3" : "text-fg-2"}`} title={`${verdict === "same" ? "Same as" : verdict === "better" ? "Better than" : "Worse than"} ${label}: ${number(compare.mean, digits)}${unit}`}>
      <Icon size={small ? 13 : 15} style={{ color }} aria-hidden="true" />
      <span className="readout whitespace-nowrap text-fg">{compare.delta > 0 && !zero ? "+" : ""}{shown}{unit}</span> vs {label}
      <span className="sr-only">, {verdict === "same" ? "the same" : verdict}</span>
    </p>
  );
}

/** The scores oldest to newest on a 0-100 scale, your mean dashed, the latest marked in its grade's colour. */
function Sparkline({ values, mean, grade }: { values: number[]; mean?: number; grade: FlightSummary["assessment"]["grade"] }) {
  const W = 240, H = 64, PAD = 6;
  const lo = Math.max(0, Math.min(...values, mean ?? 100) - 5), hi = Math.min(100, Math.max(...values, mean ?? 0) + 5);
  const x = (i: number) => PAD + (i * (W - 2 * PAD)) / (values.length - 1);
  const y = (v: number) => H - PAD - ((v - lo) / Math.max(1, hi - lo)) * (H - 2 * PAD);
  const last = values.length - 1;
  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="h-16 w-full max-w-[22rem]" role="img" aria-label={`Scores, oldest first: ${values.map((v) => number(v, 1)).join(", ")}`}>
      {mean !== undefined ? <line x1={PAD} x2={W - PAD} y1={y(mean)} y2={y(mean)} stroke="var(--text-muted)" strokeWidth={1} strokeDasharray="3 3" vectorEffect="non-scaling-stroke" /> : null}
      <polyline points={values.map((v, i) => `${x(i)},${y(v)}`).join(" ")} fill="none" stroke="var(--accent)" strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
      {values.map((v, i) => (
        <circle key={i} cx={x(i)} cy={y(v)} r={i === last ? 4.5 : 2.5} fill={i === last ? `var(${GRADE_VAR[grade]})` : "var(--accent)"} stroke="var(--surface)" strokeWidth={i === last ? 2 : 1}>
          <title>{`${i === last ? "This flight" : `${last - i} flight${last - i === 1 ? "" : "s"} before`}: ${number(v, 1)}`}</title>
        </circle>
      ))}
    </svg>
  );
}
