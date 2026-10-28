import { useMemo } from "react";
import { numberFormat } from "@ui/lib/number-format";
import type { Tone } from "@ui/lib/tone";
import { useToday } from "@ui/lib/use-today";

/**
 * Pure date helpers for feeds, histories and queues. Every function takes the reference time (`now`) and a time zone
 * explicitly instead of reading the clock or the viewer's zone, so server and client markup agree; the zone defaults to
 * UTC for that reason. Times are epoch milliseconds, ISO strings or Dates.
 */
export type DateInput = number | string | Date;

const MINUTE = 60_000, HOUR = 60 * MINUTE, DAY = 24 * HOUR, WEEK = 7 * DAY;
const toTime = (value: DateInput) => typeof value === "number" ? value : new Date(value).getTime();
const steps = [[WEEK, "week"], [DAY, "day"], [HOUR, "hour"], [MINUTE, "minute"]] as const;

/** The calendar day of a moment in a time zone, as "YYYY-MM-DD", for grouping and comparing days. */
export function dayKey(time: DateInput, timeZone = "UTC") {
  const parts = new Intl.DateTimeFormat("en-CA", { timeZone, year: "numeric", month: "2-digit", day: "2-digit" }).formatToParts(new Date(toTime(time)));
  const get = (type: string) => parts.find(part => part.type === type)?.value ?? "";
  return `${get("year")}-${get("month")}-${get("day")}`;
}

export interface DayLabelOptions {
  locale?: string;
  timeZone?: string;
  /** Localised words for the two relative days. */
  labels?: { today?: string; yesterday?: string };
}

/** A day heading (`options` may be just the locale): "Today", "Yesterday", "Sep 8", or "Sep 8, 2025" when the year differs from `now`'s. */
export function dayLabel(time: DateInput, now: DateInput, options: DayLabelOptions | string = {}) {
  const { locale = "en-US", timeZone = "UTC", labels } = typeof options === "string" ? { locale: options } : options;
  const day = dayKey(time, timeZone), today = dayKey(now, timeZone);
  if (day === today) return labels?.today ?? "Today";
  if (day === dayKey(toTime(now) - DAY, timeZone)) return labels?.yesterday ?? "Yesterday";
  const year = day.slice(0, 4) === today.slice(0, 4) ? undefined : "numeric";
  return new Intl.DateTimeFormat(locale, { month: "short", day: "numeric", year, timeZone }).format(new Date(toTime(time)));
}

export interface RelativeTimeOptions {
  locale?: string;
  /** Words for "less than a minute ago": `short` ("Now") and `long` ("just now"). */
  labels?: { short?: string; long?: string };
}

/** How long ago (`options` may be just the locale), short and long: { short: "3d", long: "3 days ago" } in en, { short: "3g", long: "3 gün önce" } in tr. Minutes, hours, days, then weeks; the future reads as now. */
export function relativeTime(time: DateInput, now: DateInput, options: RelativeTimeOptions | string = {}) {
  const { locale = "en-US", labels } = typeof options === "string" ? { locale: options } : options;
  const elapsed = Math.max(0, toTime(now) - toTime(time));
  if (elapsed < MINUTE) return { short: labels?.short ?? "Now", long: labels?.long ?? "just now" };
  const [size, unit] = steps.find(([step]) => elapsed >= step) ?? steps[3];
  const count = Math.floor(elapsed / size);
  return { short: numberFormat(locale, { style: "unit", unit, unitDisplay: "narrow" }).format(count), long: new Intl.RelativeTimeFormat(locale, { numeric: "always" }).format(-count, unit) };
}

/** 0 is Sunday ... 6 is Saturday: the first day of the week in a locale (Monday in tr, Sunday in en-US), from `Intl.Locale` where the runtime knows it. */
export type WeekStart = 0 | 1 | 2 | 3 | 4 | 5 | 6;
const SUNDAY_FIRST = new Set(["US", "CA", "MX", "BR", "JP", "IL", "IN", "KR", "TW", "HK", "PH", "ZA", "SA"]);
export function weekStartOf(locale: string): WeekStart {
  const intl = new Intl.Locale(locale) as Intl.Locale & { getWeekInfo?: () => { firstDay: number }; weekInfo?: { firstDay: number } };
  const firstDay = (intl.getWeekInfo?.() ?? intl.weekInfo)?.firstDay;
  // Intl numbers days 1 (Monday) to 7 (Sunday). Without week data, fall back on the region.
  return (firstDay !== undefined ? firstDay % 7 : SUNDAY_FIRST.has(intl.maximize().region ?? "") ? 0 : 1) as WeekStart;
}

/** True when the locale writes the clock on 24 hours (tr, de), false for 12 (en-US). */
export const uses24Hour = (locale: string) => /^h2[34]$/.test(new Intl.DateTimeFormat(locale, { hour: "numeric" }).resolvedOptions().hourCycle ?? "");

/** A weekday header: English keeps the two-letter "Mo", other languages keep the short name ("Pzt"), since two letters can be ambiguous. */
export const weekdayHeader = (short: string, locale: string) => new Intl.Locale(locale).language === "en" ? short.slice(0, 2) : short;

export interface DateTimeOptions extends Intl.DateTimeFormatOptions {
  locale?: string;
}

/** A date and time in a fixed zone: "Sep 8, 2026, 2:20 PM" by default. Pass Intl fields to choose others, such as
 *  `{ month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hourCycle: "h23" }` for "Sep 8, 14:20". */
export function formatDateTime(time: DateInput, { locale = "en-US", timeZone = "UTC", ...fields }: DateTimeOptions = {}) {
  const format: Intl.DateTimeFormatOptions = Object.keys(fields).length ? fields : { month: "short", day: "numeric", year: "numeric", hour: "numeric", minute: "2-digit" };
  return new Intl.DateTimeFormat(locale, { ...format, timeZone }).format(new Date(toTime(time)));
}

/** Calendar days from `a` to `b` in a time zone: 0 on the same day, positive when `b` is later. */
export function daysBetween(a: DateInput, b: DateInput, timeZone = "UTC") {
  return Math.round((Date.parse(dayKey(b, timeZone)) - Date.parse(dayKey(a, timeZone))) / DAY);
}

/** The tone of something waiting: neutral while fresh, warning from `warn` days, danger from `danger` days. */
export function ageTone(days: number, { warn = 7, danger = 14 }: { warn?: number; danger?: number } = {}): Tone {
  return days >= danger ? "danger" : days >= warn ? "warning" : "neutral";
}

/** Milliseconds to add to an instant to read the zone's wall clock. */
function zoneOffset(ms: number, timeZone: string) {
  const parts = new Intl.DateTimeFormat("en-US", { timeZone, hourCycle: "h23", year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric", second: "numeric" }).formatToParts(ms);
  const get = (type: string) => Number(parts.find(part => part.type === type)?.value);
  return Date.UTC(get("year"), get("month") - 1, get("day"), get("hour"), get("minute"), get("second")) - Math.floor(ms / 1000) * 1000;
}

/** The wall clock of an instant in a time zone: month is 0-based, as in `Date`. */
export function wallParts(time: DateInput, timeZone = "UTC") {
  const ms = toTime(time), wall = new Date(ms + zoneOffset(ms, timeZone));
  return { year: wall.getUTCFullYear(), month: wall.getUTCMonth(), day: wall.getUTCDate(), hour: wall.getUTCHours(), minute: wall.getUTCMinutes() };
}

/** The instant whose wall clock in `timeZone` reads `wall`, given as `Date.UTC(year, month, day, hour, minute)`. Two passes settle zones whose offset differs between the guess and the real instant. */
export function fromWall(wall: number, timeZone = "UTC") {
  const guess = wall - zoneOffset(wall, timeZone);
  return wall - zoneOffset(guess, timeZone);
}

/** The calendar day (a local-midnight `Date`) an instant falls on in a time zone; the viewer's own day without a zone. */
export function dayOf(time: DateInput = Date.now(), timeZone?: string) {
  if (!timeZone) { const date = new Date(toTime(time)); return new Date(date.getFullYear(), date.getMonth(), date.getDate()); }
  const wall = wallParts(time, timeZone);
  return new Date(wall.year, wall.month, wall.day);
}

/** The instant a calendar day starts in a time zone. Local midnight without a zone. */
export const dayStart = (day: Date, timeZone?: string) => timeZone ? fromWall(Date.UTC(day.getFullYear(), day.getMonth(), day.getDate()), timeZone) : new Date(day.getFullYear(), day.getMonth(), day.getDate()).getTime();

/** Today for pickers: undefined until hydration (like `useToday`), then the calendar day in `timeZone`, or the viewer's own day without one. */
export function useZoneToday(timeZone?: string) {
  const viewer = useToday();
  const time = viewer && timeZone ? dayOf(Date.now(), timeZone).getTime() : undefined;
  return useMemo(() => time === undefined ? viewer : new Date(time), [time, viewer]);
}
