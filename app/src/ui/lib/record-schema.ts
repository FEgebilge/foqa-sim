import type { ReactNode } from "react";
import type { Tone } from "@ui/lib/tone";

export type FieldKey<T> = Extract<keyof T, string>;
export type FieldErrors<T> = Partial<Record<FieldKey<T>, string>>;
/** Value per kind: text/email/phone/textarea string|null · number number|null · date/datetime epoch ms|null (date =
 *  UTC midnight) · choice/search/rating string|null · multi/tags string[] · toggle boolean|null · list unknown[] (view
 *  only) · custom anything. */
export type FieldKind = "text" | "email" | "phone" | "number" | "textarea" | "date" | "datetime" | "choice" | "search"
  | "multi" | "tags" | "toggle" | "rating" | "list" | "custom";
export interface ChoiceOption {
  value: string; label: string; tone?: Tone; description?: string; keywords?: string[];
  /** Picking it requires a reason (status change, bulk change, interview outcome). Replaces name/id sniffing. */
  requiresReason?: boolean;
  /** false = retired: never offered, but a saved value still shows, marked "inactive". */
  active?: boolean;
}
export interface LevelOption { value: string; label: string; description?: string; tone?: Tone }
export interface ListItemView { title: string; meta?: string; detail?: string }
/** Grows a lookup list from inside a field. Pass only what the viewer may do. */
export interface LookupSource<T> {
  noun: string;                                                          // "position"
  /** Quick add; the result gets selected. Resolve `null` when the user backed out of the host's own dialog (no error shown). */
  onCreate?: (label: string, draft: T) => Promise<ChoiceOption | null>;
  /** Hides quick add while the record cannot take it yet (a position needs its site first). Default: always offered. */
  canCreate?: (draft: T) => boolean;
  onRename?: (option: ChoiceOption, label: string) => Promise<ChoiceOption>;
  onRetire?: (option: ChoiceOption, active: boolean) => Promise<ChoiceOption>;
}
export interface FieldContext<T, K extends FieldKey<T>> {
  field: FieldDefFor<T, K>; value: T[K]; record: T; id: string; error?: string;
  onChange: (value: T[K]) => void; onBlur: () => void;
}
export interface FieldDefFor<T, K extends FieldKey<T>> {
  key: K; label: string; kind: FieldKind;
  description?: string; placeholder?: string; empty?: string;          // empty: view text, default "—"
  span?: 1 | 2;                                                         // in a two-column section
  required?: boolean | ((record: T) => boolean);
  visible?: (record: T) => boolean;                                     // conditional field (host site)
  readOnly?: boolean | ((record: T) => boolean);                        // system value: a fact even in edit mode
  options?: ChoiceOption[] | ((record: T) => ChoiceOption[]);           // a function makes cascades
  dependsOn?: FieldKey<T>[];                                            // cleared when its value leaves `options`
  validate?: (value: T[K], record: T) => string | undefined;
  maxLength?: number; min?: number; max?: number;
  levels?: LevelOption[];                                               // kind "rating"
  item?: (item: unknown) => ListItemView;                               // kind "list"
  format?: (value: T[K], record: T) => ReactNode;                       // view override
  copyable?: boolean;
  lookup?: LookupSource<T>;
  render?: { view?: (ctx: FieldContext<T, K>) => ReactNode; edit?: (ctx: FieldContext<T, K>) => ReactNode };
}
/** Distributes over keys so `value` is typed per field. */
export type FieldDef<T> = { [K in FieldKey<T>]: FieldDefFor<T, K> }[FieldKey<T>];
export interface SectionDef<T> {
  id: string; title: string; description?: string; fields: FieldDef<T>[];
  columns?: 1 | 2;              // default 2; always 1 below a 520px container
  editable?: boolean;           // default true; false = data the subject entered ("Entered by the applicant")
  collapsible?: boolean; defaultCollapsed?: boolean;
}
/** A field that list and record can change for many rows at once (status, outcome). */
export interface ChoiceField<T> { key: FieldKey<T>; label: string; options: ChoiceOption[]; reasonKey?: FieldKey<T> }

/**
 * Reject `onSave` with this to put a server's field messages on the fields: the form keeps the draft, shows each message
 * under its field (until that field is edited), focuses the first, and puts `message` in the save bar.
 */
export class FieldErrorsError<T> extends Error {
  errors: FieldErrors<T>;
  constructor(errors: FieldErrors<T>, message = "Some fields need attention") {
    super(message);
    this.name = "FieldErrorsError";
    this.errors = errors;
  }
}

export const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

/** The one "nothing here" test: null, undefined, blank text and empty lists all count, so "" and null never read as a change. */
export function isEmptyValue(value: unknown) {
  return value == null || (typeof value === "string" && !value.trim()) || (Array.isArray(value) && !value.length);
}

/** Equality for dirty checks: empties are equal, arrays and plain objects compare by value (objects ignore key order). */
export function sameValue(a: unknown, b: unknown): boolean {
  if (isEmptyValue(a) || isEmptyValue(b)) return isEmptyValue(a) && isEmptyValue(b);
  if (Object.is(a, b)) return true;
  if (a instanceof Date && b instanceof Date) return a.getTime() === b.getTime();
  if (Array.isArray(a) || Array.isArray(b)) return Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every((item, i) => sameValue(item, b[i]));
  if (typeof a !== "object" || typeof b !== "object" || !a || !b) return false;
  const left = a as Record<string, unknown>, right = b as Record<string, unknown>;
  return [...new Set([...Object.keys(left), ...Object.keys(right)])].every(key => sameValue(left[key], right[key]));
}

/** An empty record for create mode: every field key is present (lists as `[]`, the rest `null`), then `defaults` laid over it. */
export function blankRecord<T extends object>(fields: FieldDef<T>[], defaults?: Partial<T>): T {
  const blank = Object.fromEntries(fields.map(field => [field.key, field.kind === "multi" || field.kind === "tags" || field.kind === "list" ? [] : null]));
  return { ...blank, ...defaults } as T;
}

/** A rule that is a constant or a function of the record (required, readOnly, options). */
export function resolveRule<T, V>(rule: V | ((record: T) => V) | undefined, record: T, fallback: V): V {
  if (rule === undefined) return fallback;
  return typeof rule === "function" ? (rule as (record: T) => V)(record) : rule;
}

export function optionsOf<T>(field: FieldDef<T>, record: T): ChoiceOption[] {
  return resolveRule<T, ChoiceOption[]>(field.options, record, []);
}

export function findOption(options: ChoiceOption[], value: string | null | undefined) {
  return value == null ? undefined : options.find(option => option.value === value);
}

export interface ValidationLabels {
  required: (label: string) => string; email: string; tooLong: (max: number) => string;
  tooSmall?: (min: number) => string; tooLarge?: (max: number) => string;
}
const DEFAULT_LABELS: Required<ValidationLabels> = {
  required: label => `${label} is required`, email: "Enter a valid email address", tooLong: max => `Use at most ${max} characters`,
  tooSmall: min => `Must be at least ${min}`, tooLarge: max => `Must be at most ${max}`,
};

/** Visible, non-read-only fields only. Order: required → email → maxLength/min/max → field.validate. One message per field. */
export function validateFields<T>(fields: FieldDef<T>[], record: T, labels?: Partial<ValidationLabels>): FieldErrors<T> {
  const text: Required<ValidationLabels> = { ...DEFAULT_LABELS, ...labels };
  const errors: FieldErrors<T> = {};
  for (const def of fields) {
    const field = def as FieldDefFor<T, FieldKey<T>>;
    if (field.visible && !field.visible(record)) continue;
    if (resolveRule(field.readOnly, record, false)) continue;
    const value = record[field.key];
    const required = resolveRule(field.required, record, false);
    // A required switch means "must be on" (consent), so only `true` fills it; false is a real answer elsewhere.
    const filled = field.kind === "toggle" && required ? value === true : !isEmptyValue(value);
    const message =
      (!filled && required ? text.required(field.label) : undefined)
      ?? (filled && field.kind === "email" && !EMAIL_PATTERN.test(String(value).trim()) ? text.email : undefined)
      ?? (typeof value === "string" && field.maxLength !== undefined && value.length > field.maxLength ? text.tooLong(field.maxLength) : undefined)
      ?? (typeof value === "number" && field.min !== undefined && value < field.min ? text.tooSmall(field.min) : undefined)
      ?? (typeof value === "number" && field.max !== undefined && value > field.max ? text.tooLarge(field.max) : undefined)
      ?? field.validate?.(value, record);
    if (message) errors[field.key] = message;
  }
  return errors;
}

/**
 * When a field a dependent field `dependsOn` changed, drops the dependent's value (or the list entries) that its options
 * no longer offer. Options are checked as listed, so a retired value that is still offered stays. Fields are walked in
 * order, so a chain (site → position → rate) clears through. Returns `next` itself when nothing was cleared.
 */
export function pruneDependents<T>(fields: FieldDef<T>[], previous: T, next: T): T {
  let result = next;
  for (const def of fields) {
    const field = def as FieldDefFor<T, FieldKey<T>>;
    const value = result[field.key];
    if (!field.dependsOn?.length || !field.options || isEmptyValue(value)) continue;
    if (field.dependsOn.every(key => sameValue(previous[key], result[key]))) continue;
    const offered = new Set(optionsOf(def, result).map(option => option.value));
    const kept = Array.isArray(value) ? value.filter(item => offered.has(item)) : offered.has(value as string) ? value : null;
    if (!sameValue(kept, value)) result = { ...result, [field.key]: kept };
  }
  return result;
}
