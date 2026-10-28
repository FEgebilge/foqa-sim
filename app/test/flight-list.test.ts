import assert from "node:assert/strict";
import test from "node:test";
import { severity, toRow } from "../src/components/flights/flight-row.ts";
import { bytes, cleared, filtered, INITIAL, isFiltered, options, saveState, savedState, visibleRows, type ListState } from "../src/components/flights/list-state.ts";
import type { FlightSummary } from "../src/types.ts";

const NONE = { advisory: 0, low: 0, medium: 0, high: 0 };
const flight = (id: string, over: Partial<FlightSummary> = {}): FlightSummary => ({
  id, started_utc: "2026-10-26T18:05:32Z", duration_s: 600, landings: 1, event_counts: NONE,
  aircraft: { icao: "C172", title: "C172SP G1000" }, departure: "LTBL", arrival: "LTBL",
  assessment: { grade: "good", points: 0, findings: 0, method: "landing-3", score: 90, parts: [], bands: [] }, kind: "flight", ...over,
});
const rows = [
  flight("a", { meta: { title: "Gusty circuits", notes: "Crosswind 28L", tags: ["practice", "crosswind"] } }),
  flight("b", { aircraft: { icao: "A20N", title: "A320neo" }, departure: "LTFJ", arrival: "LTAI", started_utc: "2026-10-01T10:00:00Z", assessment: { grade: "poor", points: 6, findings: 2, method: "landing-3", parts: [], bands: [] } }),
  flight("c", { kind: "ground_only", meta: { tags: ["Practice"] } }),
].map(toRow);
const view = (change: Partial<ListState> = {}) => ({ ...INITIAL, ...change });
const ids = (state: ListState) => visibleRows(filtered(rows, state), state).map((row) => row.id);

test("recordings that aren't flights are hidden until asked for", () => {
  assert.deepEqual(ids(view()), ["a", "b"]);
  assert.deepEqual(ids(view({ showAll: true })), ["a", "b", "c"]);
});

test("search covers the title, notes, tags and date", () => {
  assert.deepEqual(ids(view({ query: "gusty" })), ["a"]);
  assert.deepEqual(ids(view({ query: "28l crosswind" })), ["a"]);
  assert.deepEqual(ids(view({ query: "01 oct" })), ["b"]);
  assert.deepEqual(ids(view({ query: "2026-10" })), ["a", "b"]);
  assert.deepEqual(ids(view({ query: "a20n ltai" })), ["b"]);
});

test("filters compose and clear together, keeping the sort", () => {
  assert.deepEqual(ids(view({ aircraft: "C172", showAll: true })), ["a", "c"]);
  assert.deepEqual(ids(view({ airport: "LTAI" })), ["b"]);
  assert.deepEqual(ids(view({ tag: "practice", showAll: true })), ["a", "c"], "tags match ignoring case");
  assert.deepEqual(ids(view({ from: "2026-10-20", to: "2026-10-31" })), ["a"]);
  assert.deepEqual(ids(view({ grade: "poor", airport: "LTFJ" })), ["b"]);
  assert.deepEqual(ids(view({ grade: "poor", aircraft: "C172" })), []);
  const busy = view({ query: "x", grade: "fair", tag: "t", from: "2026-01-01", to: "2026-02-01", showAll: true, sort: { key: "events", direction: "asc" } });
  assert.ok(isFiltered(busy));
  assert.deepEqual(cleared(busy), { ...INITIAL, sort: busy.sort });
  assert.ok(!isFiltered(cleared(busy)));
});

test("the list state outlives the screen", () => {
  const state = view({ query: "a320", grade: "poor", scrollY: 840 });
  saveState(state);
  assert.equal(savedState(), state);
  saveState(INITIAL);
});

test("the filter menus offer each value with its count, most used first", () => {
  const { aircraft, airports, tags } = options(rows);
  assert.deepEqual(aircraft, [{ value: "C172", hint: 2 }, { value: "A20N", hint: 1 }]);
  assert.deepEqual(airports.map((o) => o.value), ["LTBL", "LTAI", "LTFJ"]);
  assert.deepEqual(airports[0], { value: "LTBL", hint: 2 }, "a local flight counts its airport once");
  assert.deepEqual(tags.map((o) => o.value), ["crosswind", "practice", "Practice"]);
});

test("events sort by severity before count, and unscored flights have no score to mix in", () => {
  assert.ok(severity({ ...NONE, high: 1 }) > severity({ ...NONE, advisory: 5, low: 5, medium: 5 }));
  assert.ok(severity({ ...NONE, medium: 2 }) > severity({ ...NONE, medium: 1, low: 9 }));
  assert.equal(rows[0]!.assessment, 90);
  assert.equal(rows[1]!.assessment, undefined, "a flight without a scored landing sorts after the scored ones");
  assert.equal(rows[0]!.date, "2026-10-26T18:05:32Z");
});

test("sizes read as Explorer shows them", () => {
  assert.equal(bytes(512), "512 B");
  assert.equal(bytes(1_800_000), "1.7 MB");
  assert.equal(bytes(950 * 1024 ** 3), "950 GB");
});
