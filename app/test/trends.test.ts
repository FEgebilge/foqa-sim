import assert from "node:assert/strict";
import { test } from "node:test";
import { chartHeight, delta, direction, levelTone, rollingMean, trendNeeds } from "../src/lib/trends.ts";

test("rollingMean averages the trailing window", () => {
  assert.deepEqual(rollingMean([10, 20, 30, 40], 2), [10, 15, 25, 35]);
  assert.deepEqual(rollingMean([], 3), []);
});

test("delta is null without both halves", () => {
  assert.equal(delta(80, 70), 10);
  assert.equal(delta(null, 70), null);
});

test("direction reads event rates the other way round", () => {
  assert.equal(direction(5, true), "better");
  assert.equal(direction(5, false), "worse");
  assert.equal(direction(0.01, true), "same");
});

test("levelTone thresholds", () => {
  assert.equal(levelTone(0.2), "good");
  assert.equal(levelTone(1), "fair");
  assert.equal(levelTone(2), "poor");
});

test("trendNeeds says how many more flights a trend needs", () => {
  assert.equal(trendNeeds(10, "flights"), null);
  assert.equal(trendNeeds(0, "flights"), "Arrows need 10 flights, 5 in each half; none yet.");
  assert.equal(trendNeeds(7, "scored landings"), "Arrows need 10 scored landings, 5 in each half; 7 so far.");
});

test("chartHeight grows with the width and stays between its limits", () => {
  assert.equal(chartHeight(600, 220, 440), 220);
  assert.equal(chartHeight(1200, 220, 440), 360);
  assert.equal(chartHeight(2400, 220, 440), 440);
});
