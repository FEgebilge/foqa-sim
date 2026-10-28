import type { ChoiceOption } from "@ui/lib/record-schema";

/**
 * Shared filter contract: one schema drives the filter panel (controls), the list (matching) and the active-filter
 * chips, so a page declares each filter once. Values are plain data so they round-trip to URLs and server queries.
 */
export type FilterKind = "multi" | "single" | "text" | "dateRange" | "dateTimeRange" | "timeRange" | "numberRange" | "triState" | "toggle";

/** Epoch ms for dates, minutes since midnight for times, plain numbers for number ranges. */
export interface RangeValue { from?: number | null; to?: number | null }
export type FilterValue = string[] | string | boolean | RangeValue | null;
export type FilterState = Record<string, FilterValue>;

export interface FilterPreset { id: string; label: string; value: RangeValue | (() => RangeValue) }

export interface FilterField<T = unknown> {
  id: string;
  label: string;
  kind: FilterKind;
  /** Static options, or options that depend on the other filters (a cascade such as site → position). */
  options?: ChoiceOption[] | ((filters: FilterState) => ChoiceOption[]);
  /** Fields whose change clears this one's values that are no longer offered. */
  dependsOn?: string[];
  placeholder?: string;
  /** Helper text under the control, e.g. "Exact match only". */
  description?: string;
  /** Marks personal data (national id, phone, e-mail, name): the panel treats it like any field, but saved views never store it. */
  sensitive?: boolean;
  /**
   * Server-backed options for a long or remote list. `query` is what was typed; `parents` holds the values of `dependsOn`
   * (a city for its districts). Prefer it over `options` whenever the list is not small and static.
   */
  loadOptions?: (query: string, context: LoadContext) => Promise<ChoiceOption[]>;
  /** Labels for values that are selected but not in the loaded options, such as ids restored from a URL or a saved view. */
  resolveOptions?: (values: string[], context: LoadContext) => Promise<ChoiceOption[]>;
  /** Date presets (Today, Last 7 days…) for range kinds; number bounds for numberRange. */
  presets?: FilterPreset[];
  min?: number; max?: number; step?: number; unit?: string;
  /** Labels for triState: [any, yes, no]. */
  triLabels?: [string, string, string];
  /** Client-side matching for demos and small lists; server-backed pages ignore it and send the state. */
  match?: (row: T, value: FilterValue) => boolean;
  /** Text of the active-filter chip; defaults to option labels or a formatted range. */
  format?: (value: FilterValue) => string;
}

/** What an async field's loaders get: the values of the fields it depends on, and a signal that aborts a superseded request. */
export interface LoadContext { parents: FilterState; signal: AbortSignal }

export interface FilterGroup { id: string; title: string; fields: string[]; collapsible?: boolean; defaultCollapsed?: boolean }

export const isEmptyFilter = (value: FilterValue | undefined): boolean =>
  value == null || value === "" || (Array.isArray(value) && value.length === 0)
  || (typeof value === "object" && !Array.isArray(value) && value.from == null && value.to == null);

export const activeFilters = (state: FilterState) => Object.entries(state).filter(([, value]) => !isEmptyFilter(value));
export const countActive = (state: FilterState) => activeFilters(state).length;

export const optionsOf = <T,>(field: FilterField<T>, state: FilterState): ChoiceOption[] =>
  typeof field.options === "function" ? field.options(state) : field.options ?? [];

/** The values of the fields `field` depends on: the context for its async loaders and the key its option cache is stored under. */
export const parentsOf = (field: Pick<FilterField, "dependsOn">, state: FilterState): FilterState =>
  Object.fromEntries((field.dependsOn ?? []).map(id => [id, state[id] ?? null]));

const emptyOf = (value: FilterValue): FilterValue => (Array.isArray(value) ? [] : null);

/**
 * A changed field clears the dependent values its new state no longer offers. An async field cannot know what its parents now
 * offer without a request, so it is cleared outright (a district never outlives its city).
 */
export function pruneFilters<T>(fields: FilterField<T>[], prev: FilterState, next: FilterState): FilterState {
  const changed = fields.filter(field => prev[field.id] !== next[field.id]).map(field => field.id);
  if (!changed.length) return next;
  const out = { ...next };
  for (const field of fields) {
    if (!field.dependsOn?.some(id => changed.includes(id)) || isEmptyFilter(out[field.id])) continue;
    const value = out[field.id]!;
    if (field.loadOptions) { out[field.id] = emptyOf(value); continue; }
    const offered = new Set(optionsOf(field, out).map(option => option.value));
    if (Array.isArray(value)) { const kept = value.filter(item => offered.has(item)); if (kept.length !== value.length) out[field.id] = kept; }
    else if (typeof value === "string" && !offered.has(value)) out[field.id] = null;
  }
  // A cleared value is itself a change, so grandchildren (neighbourhoods under a cleared district) follow.
  return fields.some(field => out[field.id] !== next[field.id]) ? pruneFilters(fields, next, out) : out;
}

/** Default client-side match for a field without its own `match`, given the row's raw value for it. */
export function matchValue(raw: unknown, value: FilterValue, kind: FilterKind): boolean {
  if (isEmptyFilter(value)) return true;
  if (kind === "text") return String(raw ?? "").toLowerCase().includes(String(value).toLowerCase());
  if (Array.isArray(value)) return Array.isArray(raw) ? raw.some(item => value.includes(String(item))) : value.includes(String(raw));
  if (typeof value === "boolean") return Boolean(raw) === value;
  if (typeof value === "object" && value) {
    const n = typeof raw === "number" ? raw : raw instanceof Date ? raw.getTime() : Number(raw);
    return !Number.isNaN(n) && (value.from == null || n >= value.from) && (value.to == null || n <= value.to);
  }
  return String(raw) === value;
}
