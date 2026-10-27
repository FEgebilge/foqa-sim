import assert from "node:assert/strict";
import test from "node:test";
import { assessmentText } from "../src/lib/format.ts";
import type { Assessment } from "../src/types.ts";

const scored = { grade: "fair", points: 4, findings: 2, score: 77.4 } as Assessment;

test("the assessment line has single spaces and no trailing one", () => {
  assert.equal(assessmentText(scored), "Fair · 77.4 of 100");
  assert.equal(assessmentText({ ...scored, confidence: "partial" }), "Fair · 77.4 of 100 (partial data)");
  assert.equal(assessmentText({ ...scored, score: undefined, points: 1, findings: 1 }), "Fair · 1 pt from 1 finding (no scored landing)");
});
