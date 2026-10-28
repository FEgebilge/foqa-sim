import assert from "node:assert/strict";
import test from "node:test";
import { ellColor, ellWords, levelColor } from "../src/lib/assessment.ts";
import { levelTone } from "../src/lib/trends.ts";

// docs/scoring.md, "continuous level": 0 is ideal, up to 0.5 within tolerance, below 1 outside checkride
// tolerance, then floor(ℓ) is the FDM level, at most 3.
test("ℓ reads as the documented words", () => {
  assert.deepEqual([0, 0.5, 0.51, 0.99, 1, 1.9, 2, 3, 4.2].map(ellWords), [
    "ideal", "within tolerance", "outside checkride tolerance", "outside checkride tolerance",
    "level 1", "level 1", "level 2", "level 3", "level 3",
  ]);
});

test("ℓ is coloured with the level it reads as", () => {
  assert.equal(ellColor(0.5), "var(--success)");
  assert.equal(ellColor(1.4), levelColor(1));
  assert.equal(ellColor(3.7), levelColor(3));
});

// The trends' mean level 0-3 as a tone: a UI choice with no backend source, pinned here so changing it is a decision.
test("a mean level is on target below 0.75, drifting below 1.5, else costing points", () => {
  assert.deepEqual([0, 0.74, 0.75, 1.49, 1.5, 3].map(levelTone), ["good", "good", "fair", "fair", "poor", "poor"]);
});
