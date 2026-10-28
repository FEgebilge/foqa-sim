"use client";

import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import type { KeyboardEvent as ReactKeyboardEvent, CSSProperties, FocusEvent as ReactFocusEvent, ReactNode } from "react";
import { AnimatePresence, animate, motion, useIsPresent, useMotionValue, useReducedMotion, useTransform } from "motion/react";
import type { Transition, Variants } from "motion/react";
import { CalendarDays, ChevronDown, ChevronLeft, ChevronRight, X } from "lucide-react";
import { useArcLocale, useLabels } from "@ui/lib/arc-provider";
import { dayOf, dayStart, useZoneToday, weekdayHeader, weekStartOf, type WeekStart } from "@ui/lib/dates";
import { FieldMessage } from "@ui/lib/field-message";
import type { RangeValue } from "@ui/lib/filter-schema";
import { motionTokens } from "@ui/lib/motion-tokens";
import { FloatingLayer, useFloating } from "@ui/lib/floating";
import styles from "./date-range-picker.module.css";

/** An inclusive range of whole days. */
export interface DateRange {
  start: Date;
  end: Date;
}

/** A shortcut in the preset rail. `range` receives the viewer's local today. */
export interface DateRangePreset {
  label: string;
  range: (today: Date) => DateRange;
}

/**
 * A range picker whose trigger grows into the panel it opens. The panel lives in the floating layer (lib/floating), so a
 * sidebar or sheet never clips it. It shows two months beside a preset rail, or one month under a wrapping preset row
 * when the viewport is narrow. The range highlight stretches across each week as you hover
 * or move with the keyboard, the ends glide between days, months slide in the direction you travel, and on Apply the
 * formatted label flies back into the trigger as the surface shrinks around it.
 * Use it for reports, filters, and bookings where a start and end date are chosen together.
 */
export interface DateRangePickerProps {
  /** Controlled value. Pass `null` for no selection. */
  value?: DateRange | null;
  /** Uncontrolled starting value. */
  defaultValue?: DateRange | null;
  /** Called with the applied range, or `null` when the clear button empties it. */
  onChange?: (range: DateRange | null) => void;
  /** Accessible name of the trigger and the dialog. Defaults to `labels.label` ("Date range"). */
  label?: string;
  placeholder?: string;
  /** The rail: your own list, or `"default"` / `"filter"` for the built-in sets in the current language. */
  presets?: DateRangePreset[] | "default" | "filter";
  minDate?: Date;
  maxDate?: Date;
  /** 0 is Sunday, 1 is Monday. Defaults to the locale's first day of the week (Monday in tr). */
  weekStartsOn?: WeekStart;
  /** Defaults to the provider's locale. */
  locale?: string;
  /** The day "today" is read in. Defaults to the provider's time zone, then the viewer's. */
  timeZone?: string;
  /** Intl fields for the trigger and footer date, such as `{ day: "2-digit", month: "2-digit", year: "numeric" }` for 15.03.2026. */
  format?: Intl.DateTimeFormatOptions;
  labels?: Partial<DateRangePickerLabels>;
  /** Force one or two months. `auto` picks two when the boundary is wide enough. */
  months?: "auto" | 1 | 2;
  /** The element whose width decides one or two months. Defaults to the viewport: the panel floats above any sidebar or scroll area. */
  boundary?: () => HTMLElement | null;
  className?: string;
  /** Shows an X in the trigger that empties the value and reports `null`. */
  clearable?: boolean;
  /** "sm" matches the compact form controls. */
  size?: "sm" | "md";
  /** The trigger fills its container instead of hugging the value. */
  block?: boolean;
  /** The label is only the accessible name by default (the look is unchanged). Pass `false` to show it above the trigger like other fields. */
  hideLabel?: boolean;
  error?: string;
}

type Size = { w: number; h: number };
type Bezier = [number, number, number, number];

const DAY = 864e5;
const startOfDay = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate());
const monthStart = (date: Date) => new Date(date.getFullYear(), date.getMonth(), 1);
const monthEnd = (date: Date) => new Date(date.getFullYear(), date.getMonth() + 1, 0);
const addDays = (date: Date, amount: number) => new Date(date.getFullYear(), date.getMonth(), date.getDate() + amount);
const addMonths = (date: Date, amount: number) => new Date(date.getFullYear(), date.getMonth() + amount, 1);
const shiftMonths = (date: Date, amount: number) => new Date(date.getFullYear(), date.getMonth() + amount, Math.min(date.getDate(), new Date(date.getFullYear(), date.getMonth() + amount + 1, 0).getDate()));
const dayDiff = (a: Date, b: Date) => Math.round((startOfDay(a).getTime() - startOfDay(b).getTime()) / DAY);
const monthDiff = (a: Date, b: Date) => (a.getFullYear() - b.getFullYear()) * 12 + a.getMonth() - b.getMonth();
const sameDay = (a?: Date | null, b?: Date | null) => Boolean(a && b && dayDiff(a, b) === 0);
const keyOf = (date: Date) => `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
const fromKey = (key: string) => { const [year, month, day] = key.split("-").map(Number); return new Date(year!, month! - 1, day!); };
const ordered = (a: Date, b: Date): DateRange => dayDiff(a, b) <= 0 ? { start: a, end: b } : { start: b, end: a };
const sameRange = (a?: DateRange | null, b?: DateRange | null) => Boolean(a && b && sameDay(a.start, b.start) && sameDay(a.end, b.end));
const clampDate = (date: Date, min?: Date, max?: Date) => min && dayDiff(date, min) < 0 ? startOfDay(min) : max && dayDiff(date, max) > 0 ? startOfDay(max) : date;

/** The default `format`, one object so the formatters are not rebuilt on every render. */
const SHORT_DATE: Intl.DateTimeFormatOptions = { month: "short", day: "numeric", year: "numeric" };
const PRESET_RANGES = {
  today: (today: Date) => ({ start: today, end: today }),
  yesterday: (today: Date) => ({ start: addDays(today, -1), end: addDays(today, -1) }),
  last7Days: (today: Date) => ({ start: addDays(today, -6), end: today }),
  last30Days: (today: Date) => ({ start: addDays(today, -29), end: today }),
  thisMonth: (today: Date) => ({ start: monthStart(today), end: today }),
  lastMonth: (today: Date) => ({ start: addMonths(today, -1), end: monthEnd(addMonths(today, -1)) }),
  thisQuarter: (today: Date) => ({ start: new Date(today.getFullYear(), Math.floor(today.getMonth() / 3) * 3, 1), end: today }),
  yearToDate: (today: Date) => ({ start: new Date(today.getFullYear(), 0, 1), end: today }),
  thisYear: (today: Date) => ({ start: new Date(today.getFullYear(), 0, 1), end: today }),
};
type PresetKey = keyof typeof PRESET_RANGES;
const PRESET_SETS: Record<"default" | "filter", PresetKey[]> = {
  default: ["today", "yesterday", "last7Days", "last30Days", "thisMonth", "lastMonth", "thisQuarter", "yearToDate"],
  filter: ["today", "last7Days", "last30Days", "thisMonth", "lastMonth", "thisYear"],
};

export interface DateRangePickerLabels {
  label: string;
  placeholder: string;
  noDates: string;
  /** "1 day" / "5 days". */
  days: (count: number) => string;
  /** Live-region text while the first end is chosen, given the formatted start. */
  chooseEnd: (start: string) => string;
  pickEnd: string;
  presets: string;
  previousMonth: string;
  nextMonth: string;
  today: string;
  todayOn: (date: string) => string;
  cancel: string;
  apply: string;
  /** The clear button's accessible name, given the picker label. */
  clear: (label: string) => string;
  presetToday: string; presetYesterday: string; presetLast7Days: string; presetLast30Days: string;
  presetThisMonth: string; presetLastMonth: string; presetThisQuarter: string; presetYearToDate: string; presetThisYear: string;
}
const DEFAULT_LABELS: DateRangePickerLabels = {
  label: "Date range", placeholder: "Select dates", noDates: "No dates", days: count => `${count} ${count === 1 ? "day" : "days"}`,
  chooseEnd: start => `Start ${start}. Choose an end date.`, pickEnd: "Pick an end date", presets: "Presets", previousMonth: "Previous month", nextMonth: "Next month",
  today: "Today", todayOn: date => `Today, ${date}`, cancel: "Cancel", apply: "Apply", clear: label => `Clear ${label}`,
  presetToday: "Today", presetYesterday: "Yesterday", presetLast7Days: "Last 7 days", presetLast30Days: "Last 30 days", presetThisMonth: "This month",
  presetLastMonth: "Last month", presetThisQuarter: "This quarter", presetYearToDate: "Year to date", presetThisYear: "This year",
};
declare module "@ui/lib/arc-provider" { interface ArcLabels { "date-range-picker": DateRangePickerLabels } }

const capital = (key: string) => key[0]!.toUpperCase() + key.slice(1);
/** The built-in rail, labelled from `labels` (English by default): `"filter"` is the shorter set for filter sidebars. */
export const dateRangePresets = (set: "default" | "filter", labels: Partial<DateRangePickerLabels> = {}): DateRangePreset[] => {
  const words: Record<string, unknown> = { ...DEFAULT_LABELS, ...labels };
  return PRESET_SETS[set].map(key => ({ label: words[`preset${capital(key)}`] as string, range: PRESET_RANGES[key] }));
};

/** A shorter rail for filter sidebars. */
export const filterDateRangePresets = dateRangePresets("filter");

/** Filter contract bridge: epoch ms, from = start of the first day, to = last millisecond of the last day. */
export const dateRangeToValue = (range: DateRange | null | undefined, timeZone?: string): RangeValue | null =>
  range ? { from: dayStart(range.start, timeZone), to: dayStart(addDays(range.end, 1), timeZone) - 1 } : null;
/** Open-ended values (only one end set) cannot be shown by a closed range, so they map to `null`. */
export const valueToDateRange = (value: RangeValue | null | undefined, timeZone?: string): DateRange | null =>
  value?.from != null && value.to != null ? { start: dayOf(value.from, timeZone), end: dayOf(value.to, timeZone) } : null;

export const defaultDateRangePresets = dateRangePresets("default");

/** The viewer's local date (lib/use-today.ts, shared with calendar). Re-exported so existing imports from date-range-picker keep working. */
export { useToday } from "@ui/lib/use-today";

const noop = () => () => {};
function useReducedFlag() {
  const hydrated = useSyncExternalStore(noop, () => true, () => false);
  return !!useReducedMotion() && hydrated;
}

const { blur } = motionTokens;
const enterEase = [...motionTokens.ease.enter] as Bezier;
const standardEase = [...motionTokens.ease.standard] as Bezier;
/** Duration springs restated as stiffness and damping, so every retarget keeps the velocity already in flight. */
const physical = (visualDuration: number, bounce: number): Transition => {
  const root = 2 * Math.PI / (visualDuration * 1.2);
  return { type: "spring", stiffness: root * root, damping: 2 * (1 - bounce) * root, mass: 1 };
};
const GROW = physical(.48, .12), SHRINK = physical(.38, 0), STRETCH = physical(.3, .04), GLIDE = physical(.3, .1), SLIDE = physical(.4, .06);
const TRIGGER_RADIUS = 18, PANEL_RADIUS = 26, WIDE_CELL = 36, WIDE_MIN = 712, EDGE = 8;

const roll: Variants = {
  enter: (direction: number) => ({ opacity: 0, y: `${direction * .45}em`, filter: `blur(${blur.subtle}px)` }),
  center: { opacity: 1, y: "0em", filter: "blur(0px)", transition: { duration: .26, ease: enterEase } },
  exit: (direction: number) => ({ opacity: 0, y: `${direction * -.45}em`, filter: `blur(${blur.subtle}px)`, transition: { duration: .14, ease: standardEase } }),
};
const fade: Variants = { enter: { opacity: 0 }, center: { opacity: 1, y: "0em", filter: "blur(0px)", transition: { duration: .12 } }, exit: { opacity: 0, transition: { duration: .08 } } };
/** Panel content arrives just behind the growing surface and leaves ahead of it. */
const faceIn: Variants = {
  hidden: { opacity: 0, y: -4, filter: `blur(${blur.soft}px)` },
  shown: { opacity: 1, y: 0, filter: "blur(0px)", transition: { duration: .26, ease: enterEase, delay: .07 } },
  gone: { opacity: 0, y: -2, filter: `blur(${blur.soft}px)`, transition: { duration: .12, ease: standardEase } },
};
const faceFade: Variants = { hidden: { opacity: 0 }, shown: { opacity: 1, transition: { duration: .12 } }, gone: { opacity: 0, transition: { duration: .08 } } };
const slide: Variants = {
  enter: (direction: number) => ({ opacity: 0, x: direction * 48, filter: `blur(${blur.soft}px)` }),
  center: { opacity: 1, x: 0, filter: "blur(0px)", transition: { x: SLIDE, opacity: { duration: .22, ease: enterEase }, filter: { duration: .24, ease: enterEase } } },
  exit: (direction: number) => ({ opacity: 0, x: direction * -36, filter: `blur(${blur.soft}px)`, transition: { x: SLIDE, opacity: { duration: .14, ease: standardEase }, filter: { duration: .14, ease: standardEase } } }),
};

/** Words roll one by one: a later date rises from below, an earlier one drops from above; words that stay the same hold still. */
function Rolling({ text, direction, reduced }: { text: string; direction: number; reduced: boolean }) {
  const words = text.split(/\s+/).filter(Boolean);
  return <span className={styles.rolling} aria-hidden="true">
    {words.map((word, index) => <span key={index} className={styles.word}>
      <AnimatePresence initial={false} mode="popLayout" custom={direction}>
        <motion.span key={word} className={styles.wordInner} custom={direction} variants={reduced ? fade : roll} initial="enter" animate="center" exit="exit">{word}</motion.span>
      </AnimatePresence>
    </span>)}
  </span>;
}

/** Remembers which way a range moved, so labels roll in time's direction. */
function useDirection(range: DateRange | null) {
  const time = range ? range.start.getTime() * 2 + range.end.getTime() : null;
  const [previous, setPrevious] = useState(time);
  const [direction, setDirection] = useState(1);
  if (time !== previous) { setPrevious(time); setDirection(time === null || previous === null || time >= previous ? 1 : -1); }
  return direction;
}

type Segment = { left: number; width: number } | null;
const cellIn = (date: Date, first: Date) => { const index = dayDiff(date, first); return { row: Math.floor(index / 7), col: index % 7 }; };

/** One week's slice of the highlight. It grows out of the edge the range arrives from, so hovering reads as stretching. */
function Bar({ row, segment, reduced }: { row: number; segment: Segment; reduced: boolean }) {
  const left = useMotionValue(segment?.left ?? 0), width = useMotionValue(segment?.width ?? 0), opacity = useMotionValue(segment ? 1 : 0);
  const leftPct = useTransform(left, value => `${value}%`), widthPct = useTransform(width, value => `${value}%`);
  const wasEmpty = useRef(!segment);
  useLayoutEffect(() => {
    if (!segment) { wasEmpty.current = true; animate(opacity, 0, { duration: reduced ? 0 : .14 }); return; }
    if (wasEmpty.current || reduced) { left.jump(segment.left); width.jump(segment.width); }
    else { animate(left, segment.left, STRETCH); animate(width, segment.width, STRETCH); }
    wasEmpty.current = false;
    animate(opacity, 1, { duration: reduced ? 0 : .16 });
  }, [left, opacity, reduced, segment, width]);
  return <motion.span className={styles.bar} style={{ left: leftPct, width: widthPct, opacity, top: `calc(${row} * var(--cell) + 3px)` }} />;
}

/** A solid end of the range. It glides from day to day and only fades when the end leaves this month. */
function Thumb({ cell, reduced }: { cell: { row: number; col: number } | null; reduced: boolean }) {
  const x = useMotionValue(cell?.col ?? 0), y = useMotionValue(cell?.row ?? 0), opacity = useMotionValue(cell ? 1 : 0), scale = useMotionValue(cell ? 1 : .8);
  const transform = useTransform(() => `translate(${x.get() * 100}%, ${y.get() * 100}%) scale(${scale.get()})`);
  const hidden = useRef(!cell);
  useLayoutEffect(() => {
    if (!cell) { hidden.current = true; animate(opacity, 0, { duration: reduced ? 0 : .12 }); animate(scale, .8, { duration: reduced ? 0 : .12 }); return; }
    if (hidden.current || reduced) { x.jump(cell.col); y.jump(cell.row); }
    else { animate(x, cell.col, GLIDE); animate(y, cell.row, GLIDE); }
    hidden.current = false;
    animate(opacity, 1, { duration: reduced ? 0 : .16 });
    animate(scale, 1, reduced ? { duration: 0 } : GLIDE);
  }, [cell, opacity, reduced, scale, x, y]);
  return <motion.span className={styles.thumb} style={{ transform, opacity }} />;
}

interface MonthProps {
  month: Date;
  range: DateRange | null;
  tabbable: string;
  today?: Date;
  minDate?: Date;
  maxDate?: Date;
  weekStartsOn: WeekStart;
  reduced: boolean;
  formatters: { locale: string; title: Intl.DateTimeFormat; day: Intl.DateTimeFormat; weekday: Intl.DateTimeFormat; weekdayLong: Intl.DateTimeFormat };
  onPick: (date: Date) => void;
  onHover: (date: Date) => void;
  onKey: (event: ReactKeyboardEvent<HTMLButtonElement>, date: Date) => void;
  onFocusDay: (date: Date) => void;
  idBase: string;
}

function Month({ month, range, tabbable, today, minDate, maxDate, weekStartsOn, reduced, formatters, onPick, onHover, onKey, onFocusDay, idBase }: MonthProps) {
  const first = useMemo(() => addDays(month, -((month.getDay() - weekStartsOn + 7) % 7)), [month, weekStartsOn]);
  const rows = useMemo(() => Array.from({ length: 6 }, (_, row) => Array.from({ length: 7 }, (_, col) => addDays(first, row * 7 + col))), [first]);
  const titleId = `${idBase}-${keyOf(month)}`;
  const inMonth = (date: Date) => monthDiff(date, month) === 0;

  const lo = range?.start, hi = range?.end;
  const loKey = lo ? keyOf(lo) : "", hiKey = hi ? keyOf(hi) : "";
  /** Each week's slice of the range, in percent of the row. Keyed by the range's days so bars only move when it does. */
  const segments = useMemo<Segment[]>(() => {
    if (!loKey || !hiKey) return rows.map(() => null);
    const start = fromKey(loKey), end = fromKey(hiKey);
    const unit = 100 / 7;
    const colOf = (date: Date) => dayDiff(date, first) % 7;
    return rows.map(row => {
      const days = row.filter(date => monthDiff(date, month) === 0);
      if (!days.length) return null;
      const rowFirst = days[0]!, rowLast = days[days.length - 1]!;
      const from = dayDiff(start, rowFirst) > 0 ? start : rowFirst, to = dayDiff(end, rowLast) < 0 ? end : rowLast;
      if (dayDiff(from, to) <= 0) { const a = colOf(from), b = colOf(to); return { left: a * unit, width: (b - a + 1) * unit }; }
      // The week sits wholly before or after the range: collapse at the edge the range would grow in from.
      if (dayDiff(rowLast, start) < 0) return { left: (colOf(rowLast) + 1) * unit, width: 0 };
      return { left: colOf(rowFirst) * unit, width: 0 };
    });
  }, [first, hiKey, loKey, month, rows]);
  const startCell = useMemo(() => loKey && monthDiff(fromKey(loKey), month) === 0 ? cellIn(fromKey(loKey), first) : null, [loKey, month, first]);
  const endCell = useMemo(() => hiKey && hiKey !== loKey && monthDiff(fromKey(hiKey), month) === 0 ? cellIn(fromKey(hiKey), first) : null, [hiKey, loKey, month, first]);

  return <div className={styles.month}>
    <p id={titleId} className={styles.monthTitle}>{formatters.title.format(month)}</p>
    <div role="grid" aria-labelledby={titleId} className={styles.grid}>
      <div role="row" className={styles.weekdays}>
        {rows[0]!.map(date => <span key={date.getDay()} role="columnheader" aria-label={formatters.weekdayLong.format(date)}>{weekdayHeader(formatters.weekday.format(date), formatters.locale)}</span>)}
      </div>
      <div className={styles.weeks}>
        <span className={styles.layer} aria-hidden="true">
          {segments.map((segment, row) => <Bar key={row} row={row} segment={segment} reduced={reduced} />)}
          <Thumb cell={startCell} reduced={reduced} />
          <Thumb cell={endCell} reduced={reduced} />
        </span>
        {rows.map((row, index) => <div key={index} role="row" className={styles.week}>
          {row.map(date => {
            if (!inMonth(date)) return <span key={keyOf(date)} role="gridcell" className={styles.blank} />;
            const key = keyOf(date);
            const disabled = Boolean((minDate && dayDiff(date, minDate) < 0) || (maxDate && dayDiff(date, maxDate) > 0));
            const inRange = Boolean(lo && hi && dayDiff(date, lo) >= 0 && dayDiff(date, hi) <= 0);
            const edge = sameDay(date, lo) || sameDay(date, hi);
            const isToday = sameDay(date, today);
            return <span key={key} role="gridcell" aria-selected={inRange} className={styles.cell}>
              <button type="button" className={styles.day} data-date={key} data-edge={edge || undefined} data-range={inRange || undefined} data-today={isToday || undefined}
                tabIndex={key === tabbable ? 0 : -1} disabled={disabled} aria-current={isToday ? "date" : undefined}
                aria-label={formatters.day.format(date)}
                onClick={() => onPick(date)} onPointerEnter={() => { if (!disabled) onHover(date); }}
                onFocus={() => onFocusDay(date)} onKeyDown={event => onKey(event, date)}>
                <span>{date.getDate()}</span>
              </button>
            </span>;
          })}
        </div>)}
      </div>
    </div>
  </div>;
}

/** The visible months travel together; a leaving set turns inert so focus and queries only find the current one. */
function Months({ children, direction, reduced }: { children: ReactNode; direction: number; reduced: boolean }) {
  const present = useIsPresent();
  return <motion.div className={styles.months} data-current={present || undefined} inert={!present || undefined} custom={direction}
    variants={reduced ? fade : slide} initial="enter" animate="center" exit="exit">{children}</motion.div>;
}

export function DateRangePicker({ value, defaultValue = null, onChange, label: labelProp, placeholder: placeholderProp, presets = "default", minDate, maxDate, weekStartsOn: weekStartProp, locale: localeProp, timeZone: zoneProp, format: labelFormat = SHORT_DATE, labels, months = "auto", boundary, className, clearable = false, size = "md", block = false, hideLabel = true, error }: DateRangePickerProps) {
  const t = useLabels("date-range-picker", DEFAULT_LABELS, labels);
  const arc = useArcLocale();
  const locale = localeProp ?? arc.locale, timeZone = zoneProp ?? arc.timeZone;
  const weekStartsOn = weekStartProp ?? weekStartOf(locale);
  const label = labelProp ?? t.label, placeholder = placeholderProp ?? t.placeholder;
  const reduced = useReducedFlag();
  const today = useZoneToday(timeZone);
  const uid = useId();
  const [inner, setInner] = useState<DateRange | null>(defaultValue);
  const committed = value !== undefined ? value : inner;

  const [open, setOpen] = useState(false);
  const [compact, setCompact] = useState(false);
  const [panelWidth, setPanelWidth] = useState(0);
  const [view, setView] = useState<Date>(() => monthStart(committed?.end ?? new Date(2000, 0, 1)));
  const [direction, setDirection] = useState(1);
  const [draft, setDraft] = useState<DateRange | null>(committed);
  const [anchor, setAnchor] = useState<Date | null>(null);
  const [hover, setHover] = useState<Date | null>(null);
  const [focusKey, setFocusKey] = useState("");
  // The surface sits in the floating layer from the moment it opens until it has shrunk back onto the trigger.
  const [lifted, setLifted] = useState(false);
  if (open && !lifted) setLifted(true);
  // Today always leads the rail, so a custom preset list never loses it.
  const list = useMemo(() => typeof presets === "string" ? dateRangePresets(presets, t) : presets, [presets, t]);
  const todayLabel = t.presetToday.trim().toLowerCase();
  const rail = useMemo(() => !list.length || list.some(preset => preset.label.trim().toLowerCase() === todayLabel) ? list : [{ label: t.presetToday, range: PRESET_RANGES.today }, ...list], [list, t.presetToday, todayLabel]);

  const formatters = useMemo(() => ({
    locale,
    label: new Intl.DateTimeFormat(locale, labelFormat),
    title: new Intl.DateTimeFormat(locale, { month: "long", year: "numeric" }),
    day: new Intl.DateTimeFormat(locale, { weekday: "long", month: "long", day: "numeric", year: "numeric" }),
    weekday: new Intl.DateTimeFormat(locale, { weekday: "short" }),
    weekdayLong: new Intl.DateTimeFormat(locale, { weekday: "long" }),
  }), [locale, labelFormat]);
  const format = useCallback((range: DateRange | null) => !range ? placeholder : sameDay(range.start, range.end) ? formatters.label.format(range.start) : formatters.label.formatRange(range.start, range.end), [formatters, placeholder]);

  const count = compact ? 1 : 2;
  const visible = useMemo(() => Array.from({ length: count }, (_, index) => addMonths(view, index)), [count, view]);
  const shown = anchor ? ordered(anchor, hover ?? anchor) : draft;
  const activePreset = today && !anchor ? rail.findIndex(preset => sameRange(preset.range(today), draft)) : -1;
  const days = shown ? dayDiff(shown.end, shown.start) + 1 : 0;
  const committedDirection = useDirection(committed);
  const shownDirection = useDirection(shown);

  const rootRef = useRef<HTMLDivElement>(null);
  const floating = useFloating(rootRef, lifted, { placement: "over" });
  const inFloating = floating.contains;
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const railRef = useRef<HTMLDivElement>(null);
  const pendingFocus = useRef<"trigger" | "day" | null>(null);

  /* ---------- One surface: the trigger's box springs to the panel's box and back ---------- */
  // Before the first measurement the surface simply fills the root and the root wraps the trigger, so server markup already looks right.
  const width = useMotionValue<number | string>("100%"), height = useMotionValue<number | string>("100%"), radius = useMotionValue(TRIGGER_RADIUS), x = useMotionValue(0), y = useMotionValue(0), rootWidth = useMotionValue<number | string>("auto");
  const sizes = useRef<{ trigger: Size; panel: Size | null }>({ trigger: { w: 0, h: 0 }, panel: null });
  const live = useRef({ open: false, reduced: false, ready: false });
  const queued = useRef(false);
  const boundaryRef = useRef(boundary);
  useLayoutEffect(() => { boundaryRef.current = boundary; }, [boundary]);

  /** The panel floats above every clipping ancestor, so only the viewport (or an explicit boundary) limits it. */
  const bounds = useCallback(() => {
    const element = boundaryRef.current?.() ?? null;
    const viewport = { left: 0, right: document.documentElement.clientWidth };
    if (!element) return viewport;
    const rect = element.getBoundingClientRect();
    return { left: Math.max(viewport.left, rect.left), right: Math.min(viewport.right, rect.right) };
  }, []);

  const update = useCallback(() => {
    queued.current = false;
    const { trigger, panel } = sizes.current, state = live.current;
    if (!trigger.w) return;
    const openPanel = state.open && panel ? panel : null;
    const target = openPanel ?? trigger;
    let offset = 0, lift = 0;
    // The panel grows from the trigger's corner and slides just enough to stay EDGE inside the viewport on both axes.
    if (openPanel && rootRef.current) {
      const rect = rootRef.current.getBoundingClientRect(), viewport = document.documentElement;
      offset = Math.max(Math.min(0, viewport.clientWidth - EDGE - (rect.left + openPanel.w)), EDGE - rect.left);
      lift = Math.max(Math.min(0, viewport.clientHeight - EDGE - (rect.top + openPanel.h)), EDGE - rect.top);
    }
    const r = openPanel ? PANEL_RADIUS : TRIGGER_RADIUS;
    if (!state.ready || state.reduced) {
      width.jump(target.w); height.jump(target.h); radius.jump(r); x.jump(offset); y.jump(lift); rootWidth.jump(trigger.w);
      state.ready = true;
      if (!state.open) setLifted(false);
      return;
    }
    const spring = openPanel ? GROW : state.open ? GROW : SHRINK;
    const settled = Promise.all([animate(width, target.w, spring), animate(height, target.h, spring), animate(x, offset, spring), animate(y, lift, spring)]);
    animate(radius, r, spring);
    animate(rootWidth, trigger.w, motionTokens.spring.morph);
    // Back on the trigger, the surface returns from the floating layer into the field.
    if (!state.open) void settled.then(() => { if (!live.current.open) setLifted(false); });
  }, [height, radius, rootWidth, width, x, y]);

  const schedule = useCallback(() => {
    if (queued.current) return;
    queued.current = true;
    queueMicrotask(update);
  }, [update]);

  useLayoutEffect(() => { live.current.open = open; live.current.reduced = reduced; if (!open) sizes.current.panel = null; schedule(); }, [open, reduced, schedule]);

  useLayoutEffect(() => {
    const node = triggerRef.current;
    if (!node) return;
    const read = () => { sizes.current.trigger = { w: node.offsetWidth, h: node.offsetHeight }; schedule(); };
    read();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(read);
    observer.observe(node);
    return () => observer.disconnect();
  }, [schedule]);

  useLayoutEffect(() => {
    const node = panelRef.current;
    if (!open || !node) return;
    const read = () => { sizes.current.panel = { w: node.offsetWidth, h: node.offsetHeight }; schedule(); };
    read();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(read);
    observer.observe(node);
    return () => observer.disconnect();
  }, [open, schedule]);

  /* ---------- Layout: two months when there is room, one compact month otherwise ---------- */
  const measureLayout = useCallback(() => {
    const box = bounds();
    const available = Math.max(0, box.right - box.left - EDGE * 2);
    const single = months === 1 || (months === "auto" && available < WIDE_MIN);
    return { single, width: Math.min(352, available) };
  }, [bounds, months]);

  useEffect(() => {
    if (!open) return;
    const onResize = () => { const next = measureLayout(); setCompact(next.single); setPanelWidth(next.width); schedule(); };
    // The floating layer follows the trigger on scroll; the panel's own offsets then re-fit it to the viewport.
    window.addEventListener("resize", onResize);
    window.addEventListener("scroll", schedule, { capture: true, passive: true });
    return () => { window.removeEventListener("resize", onResize); window.removeEventListener("scroll", schedule, { capture: true }); };
  }, [measureLayout, open, schedule]);

  const viewFor = (range: DateRange | null, single: boolean, fallback: Date) => {
    const end = monthStart(range?.end ?? fallback);
    return single ? end : addMonths(end, -1);
  };

  const openPanel = () => {
    if (!today) return;
    const layout = measureLayout();
    setCompact(layout.single);
    setPanelWidth(layout.width);
    setDraft(committed);
    setAnchor(null);
    setHover(null);
    setView(viewFor(committed, layout.single, today));
    setDirection(0);
    setFocusKey(keyOf(committed?.start ?? today));
    pendingFocus.current = "day";
    setOpen(true);
  };

  const close = useCallback((focus: boolean) => {
    if (focus) pendingFocus.current = "trigger";
    setDirection(0);
    setOpen(false);
    setAnchor(null);
    setHover(null);
  }, []);

  const apply = () => {
    const next = anchor ? { start: anchor, end: anchor } : draft;
    if (!next) return;
    if (value === undefined) setInner(next);
    onChange?.(next);
    close(true);
  };

  const clear = () => {
    if (value === undefined) setInner(null);
    onChange?.(null);
    triggerRef.current?.focus({ preventScroll: true });
  };

  /* ---------- Focus management ---------- */
  useLayoutEffect(() => {
    const target = pendingFocus.current;
    if (!target) return;
    if (target === "trigger" && !open) { pendingFocus.current = null; triggerRef.current?.focus({ preventScroll: true }); return; }
    if (target === "day" && open) {
      const node = panelRef.current?.querySelector<HTMLButtonElement>(`[data-current] [data-date="${focusKey}"]`);
      if (node) { pendingFocus.current = null; node.focus({ preventScroll: true }); }
    }
  }, [focusKey, open, view, compact]);

  useEffect(() => {
    if (!open) return;
    const down = (event: PointerEvent) => { const target = event.target as Node; if (!rootRef.current?.contains(target) && !inFloating(target)) close(false); };
    document.addEventListener("pointerdown", down);
    return () => document.removeEventListener("pointerdown", down);
  }, [close, inFloating, open]);

  const onRootKey = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape" && open) { event.preventDefault(); event.stopPropagation(); close(true); }
  };
  const onRootBlur = (event: ReactFocusEvent<HTMLDivElement>) => {
    const next = event.relatedTarget as Node | null;
    if (open && next && !event.currentTarget.contains(next) && !inFloating(next)) close(false);
  };

  /* ---------- Month navigation and picking ---------- */
  const goTo = (next: Date) => {
    const delta = monthDiff(next, view);
    if (!delta) return;
    setDirection(Math.sign(delta));
    setView(next);
  };

  /** Keeps a range on screen: nothing moves when it already fits, otherwise its last month lands on the right. */
  const reveal = (range: DateRange) => {
    const first = view, last = addMonths(view, count - 1);
    if (monthDiff(range.start, first) >= 0 && monthDiff(range.end, last) <= 0) return;
    goTo(viewFor(range, compact, range.end));
  };

  const pick = (date: Date) => {
    setFocusKey(keyOf(date));
    if (!anchor) { setAnchor(date); setHover(date); return; }
    setDraft(ordered(anchor, date));
    setAnchor(null);
    setHover(null);
  };

  /** Footer Today: brings today's month into view and puts focus on today, without changing the draft. */
  const showToday = () => {
    if (!today) return;
    const target = clampDate(today, minDate, maxDate);
    setFocusKey(keyOf(target));
    pendingFocus.current = "day";
    if (!visible.some(month => monthDiff(target, month) === 0)) goTo(viewFor({ start: target, end: target }, compact, target));
  };

  const choosePreset = (preset: DateRangePreset) => {
    if (!today) return;
    const range = preset.range(today);
    setDraft(range);
    setAnchor(null);
    setHover(null);
    setFocusKey(keyOf(range.start));
    reveal(range);
  };

  const onDayKey = (event: ReactKeyboardEvent<HTMLButtonElement>, date: Date) => {
    const weekday = (date.getDay() - weekStartsOn + 7) % 7;
    const moves: Record<string, () => Date> = {
      ArrowLeft: () => addDays(date, -1), ArrowRight: () => addDays(date, 1), ArrowUp: () => addDays(date, -7), ArrowDown: () => addDays(date, 7),
      Home: () => addDays(date, -weekday), End: () => addDays(date, 6 - weekday),
      PageUp: () => shiftMonths(date, event.shiftKey ? -12 : -1), PageDown: () => shiftMonths(date, event.shiftKey ? 12 : 1),
    };
    const move = moves[event.key];
    if (!move) return;
    event.preventDefault();
    const next = clampDate(move(), minDate, maxDate);
    setFocusKey(keyOf(next));
    if (anchor) setHover(next);
    pendingFocus.current = "day";
    const first = view, last = addMonths(view, count - 1);
    if (monthDiff(next, first) < 0) goTo(monthStart(next));
    else if (monthDiff(next, last) > 0) goTo(addMonths(monthStart(next), -(count - 1)));
  };

  const onRailKey = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const keys = compact ? ["ArrowLeft", "ArrowRight"] : ["ArrowUp", "ArrowDown"];
    if (!keys.includes(event.key)) return;
    const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("[data-preset]"));
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (at < 0) return;
    event.preventDefault();
    buttons[(at + (event.key === keys[1] ? 1 : -1) + buttons.length) % buttons.length]?.focus();
  };

  /** The day that owns the tab stop: the focused day when it is on screen, else the first visible day of the range, else the 1st. */
  const tabbable = useMemo(() => {
    const onScreen = (date: Date) => visible.some(month => monthDiff(date, month) === 0);
    if (focusKey && onScreen(fromKey(focusKey))) return focusKey;
    if (shown && onScreen(shown.start)) return keyOf(shown.start);
    return keyOf(visible[0]!);
  }, [focusKey, shown, visible]);

  /* ---------- Preset highlight glides between presets ---------- */
  const gx = useMotionValue(0), gy = useMotionValue(0), gw = useMotionValue(0), gh = useMotionValue(0), go = useMotionValue(0);
  useLayoutEffect(() => {
    const rail = railRef.current;
    const node = activePreset < 0 ? null : rail?.querySelector<HTMLElement>(`[data-preset="${activePreset}"]`);
    if (!rail || !node) { animate(go, 0, { duration: reduced ? 0 : .14 }); return; }
    const place = [node.offsetLeft, node.offsetTop, node.offsetWidth, node.offsetHeight] as const;
    if (go.get() < .05 || reduced) { gx.jump(place[0]); gy.jump(place[1]); gw.jump(place[2]); gh.jump(place[3]); }
    else { animate(gx, place[0], GLIDE); animate(gy, place[1], GLIDE); animate(gw, place[2], GLIDE); animate(gh, place[3], GLIDE); }
    animate(go, 1, { duration: reduced ? 0 : .16 });
  }, [activePreset, compact, go, gh, gw, gx, gy, open, reduced]);

  /* ---------- Copy ---------- */
  const committedText = format(committed);
  const shownText = shown ? format(shown) : t.noDates;
  const countText = shown ? t.days(days) : "";
  const status = !open ? "" : anchor ? t.chooseEnd(formatters.label.format(anchor)) : shown ? `${shownText}, ${countText}` : "";
  const layoutId = reduced ? undefined : `${uid}-value`;

  const cellSize = compact && panelWidth ? Math.max(28, Math.min(42, Math.floor((panelWidth - 24) / 7))) : WIDE_CELL;

  const quiet = open ? (reduced ? { opacity: 0 } : { opacity: 0, filter: `blur(${blur.subtle}px)` }) : { opacity: 1, filter: "blur(0px)" };
  const quietTransition = open ? { duration: .12, ease: standardEase } : { duration: .22, ease: enterEase, delay: reduced ? 0 : .1 };

  const errorId = error ? `${uid}-error` : undefined;
  const surface = (
    <motion.div key="surface" className={styles.surface} data-lifted={lifted || undefined} data-invalid={error ? true : undefined} style={{ width, height, borderRadius: radius, x, y }}>
      <AnimatePresence>
        {open && today && <motion.div key="panel" ref={panelRef} id={`${uid}-panel`} className={styles.panel} role="dialog" aria-label={label} data-compact={compact || undefined}
          style={{ "--cell": `${cellSize}px`, width: compact && panelWidth ? panelWidth : undefined } as CSSProperties} exit={{ opacity: 1, transition: { duration: .14 } }}>
          <motion.div className={styles.body} variants={reduced ? faceFade : faceIn} initial="hidden" animate="shown" exit="gone">
            <div ref={railRef} className={styles.rail} role="group" aria-label={t.presets} onKeyDown={onRailKey}>
              <motion.span className={styles.railHighlight} style={{ x: gx, y: gy, width: gw, height: gh, opacity: go }} aria-hidden="true" />
              {rail.map((preset, index) => <button key={preset.label} type="button" className={styles.preset} data-preset={index} aria-pressed={index === activePreset}
                onClick={() => choosePreset(preset)}>{preset.label}</button>)}
            </div>

            <div className={styles.calendars} onPointerLeave={() => { if (anchor) setHover(null); }}>
              <button type="button" className={styles.navButton} data-side="prev" aria-label={t.previousMonth} onClick={() => goTo(addMonths(view, -1))}
                disabled={Boolean(minDate && monthDiff(view, minDate) <= 0)}><ChevronLeft size={16} strokeWidth={1.75} aria-hidden="true" /></button>
              <button type="button" className={styles.navButton} data-side="next" aria-label={t.nextMonth} onClick={() => goTo(addMonths(view, 1))}
                disabled={Boolean(maxDate && monthDiff(addMonths(view, count - 1), maxDate) >= 0)}><ChevronRight size={16} strokeWidth={1.75} aria-hidden="true" /></button>
              <div className={styles.viewport}>
                <AnimatePresence initial={false} mode="popLayout" custom={direction}>
                  <Months key={`${keyOf(view)}-${count}`} direction={direction} reduced={reduced}>
                    {visible.map(month => <Month key={keyOf(month)} month={month} range={shown} tabbable={tabbable} today={today} minDate={minDate} maxDate={maxDate}
                      weekStartsOn={weekStartsOn} reduced={reduced} formatters={formatters} idBase={uid}
                      onPick={pick} onHover={date => { if (anchor) setHover(date); }} onKey={onDayKey} onFocusDay={date => setFocusKey(keyOf(date))} />)}
                  </Months>
                </AnimatePresence>
              </div>
            </div>
          </motion.div>

          <div className={styles.footer}>
            <div className={styles.summary}>
              <motion.span layoutId={layoutId} layout={layoutId ? "position" : undefined} className={styles.summaryValue} transition={GROW}
                variants={layoutId ? undefined : faceFade} initial={layoutId ? undefined : "hidden"} animate={layoutId ? undefined : "shown"} exit={layoutId ? undefined : "gone"}>
                <Rolling text={shownText} direction={shownDirection} reduced={reduced} />
              </motion.span>
              <motion.span className={styles.summaryCount} variants={reduced ? faceFade : faceIn} initial="hidden" animate="shown" exit="gone">
                {countText && <Rolling text={anchor && sameDay(anchor, hover) ? "Pick an end date" : countText} direction={shownDirection} reduced={reduced} />}
              </motion.span>
            </div>
            <motion.div className={styles.actions} variants={reduced ? faceFade : faceIn} initial="hidden" animate="shown" exit="gone">
              <button type="button" className={styles.ghost} data-today="" onClick={showToday} aria-label={today ? t.todayOn(formatters.day.format(today)) : t.today}>{t.today}</button>
              <button type="button" className={styles.ghost} onClick={() => close(true)}>{t.cancel}</button>
              <button type="button" className={styles.primary} onClick={apply} disabled={!shown}>{t.apply}</button>
            </motion.div>
          </div>
          <p className={styles.srOnly} aria-live="polite">{status}</p>
        </motion.div>}
      </AnimatePresence>
    </motion.div>
  );
  return <div className={[styles.field, className].filter(Boolean).join(" ")} data-block={block || undefined} data-invalid={error ? true : undefined}>
    {!hideLabel && <span className={styles.label}>{label}</span>}
    <motion.div ref={rootRef} className={styles.root} style={{ width: block ? "100%" : rootWidth }} data-open={open || undefined} data-size={size} data-block={block || undefined} data-clearable={clearable && committed ? true : undefined} onKeyDown={onRootKey} onBlur={onRootBlur}>
    {/* The trigger stays in place under the panel. Its icons fade, while the value itself flies to the panel footer and back. */}
    <button ref={triggerRef} type="button" className={styles.trigger} aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? `${uid}-panel` : undefined}
      aria-label={`${label}: ${committedText}`} aria-invalid={error ? true : undefined} aria-describedby={errorId} disabled={!today} inert={open || undefined} onClick={openPanel}>
      <motion.span className={styles.triggerIcon} initial={false} animate={quiet} transition={quietTransition}><CalendarDays size={16} strokeWidth={1.75} aria-hidden="true" /></motion.span>
      {open || !layoutId
        ? <motion.span key="resting" className={styles.value} data-empty={!committed || undefined} initial={false} animate={open ? { opacity: 0 } : { opacity: 1 }} transition={quietTransition}>
          <Rolling text={committedText} direction={committedDirection} reduced={reduced} />
        </motion.span>
        : <motion.span key="shared" layoutId={layoutId} layout="position" className={styles.value} data-empty={!committed || undefined} transition={SHRINK}>
          <Rolling text={committedText} direction={committedDirection} reduced={reduced} />
        </motion.span>}
      <motion.span className={styles.chevron} initial={false} animate={quiet} transition={quietTransition}><ChevronDown size={16} strokeWidth={1.75} aria-hidden="true" /></motion.span>
    </button>
    {clearable && committed && !open && <button type="button" className={styles.clear} aria-label={t.clear(label)} onClick={clear}><X size={14} strokeWidth={1.75} aria-hidden="true" /></button>}

    {!lifted && surface}
    <FloatingLayer floating={floating}>{lifted && surface}</FloatingLayer>
    </motion.div>
    <FieldMessage id={errorId} text={error} className={styles.error} alert />
  </div>;
}

export default DateRangePicker;
