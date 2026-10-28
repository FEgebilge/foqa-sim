import assert from "node:assert/strict";
import { test } from "node:test";
import { delta, direction, levelTone, rollingMean } from "../src/lib/trends.ts";

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
