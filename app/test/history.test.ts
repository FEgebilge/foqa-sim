import assert from "node:assert/strict";
import { test } from "node:test";
import { adjacentFlights, median, typeMedian } from "../src/lib/history.ts";
import type { FlightSummary, TrendLanding } from "../src/types.ts";

const flight = (id: string, started_utc?: string) => ({ id, started_utc }) as FlightSummary;

test("adjacentFlights steps by start time, whatever order the list is in", () => {
  const list = [flight("c", "2026-10-03T08:00:00Z"), flight("a", "2026-10-01T08:00:00Z"), flight("b", "2026-10-02T08:00:00Z")];
  assert.deepEqual(adjacentFlights(list, "b"), { older: list[1], newer: list[0] });
  assert.deepEqual(adjacentFlights(list, "a"), { older: undefined, newer: list[2] });
  assert.deepEqual(adjacentFlights(list, "c"), { older: list[2], newer: undefined });
  assert.deepEqual(adjacentFlights(list, "gone"), {});
});

test("adjacentFlights puts undated flights first and breaks ties by id", () => {
  const list = [flight("y", "2026-10-01T08:00:00Z"), flight("x", "2026-10-01T08:00:00Z"), flight("undated")];
  assert.equal(adjacentFlights(list, "x").older?.id, "undated");
  assert.equal(adjacentFlights(list, "x").newer?.id, "y");
});

test("median of odd, even and empty lists", () => {
  assert.equal(median([3, 1, 2]), 2);
  assert.equal(median([4, 1, 3, 2]), 2.5);
  assert.equal(median([]), null);
});

const landing = (flight_id: string, aircraft: string, sink_fpm: number, peak_g: number) => ({ flight_id, aircraft, sink_fpm, peak_g }) as TrendLanding;

test("typeMedian leaves out this flight and other types, and needs three other flights", () => {
  const rows = [landing("this", "C172", 900, 2), landing("f1", "C172", 100, 1.1), landing("f2", "C172", 300, 1.3), landing("f2", "C172", 200, 1.2), landing("b", "B738", 50, 1)];
  assert.equal(typeMedian(rows, "C172", "this"), null, "two other flights");
  const median3 = typeMedian([...rows, landing("f3", "C172", 400, 1.5)], "C172", "this");
  assert.deepEqual(median3, { sink_fpm: 250, peak_g: 1.25, landings: 4, flights: 3 });
});
