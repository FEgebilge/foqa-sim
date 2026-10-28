import assert from "node:assert/strict";
import test from "node:test";
import { flowStep, methodOf, partLabel, scoringFact } from "../src/lib/challenge-flow.ts";
import type { AttemptView, PreflightLine } from "../src/types.ts";

const line = (state: PreflightLine["state"]): PreflightLine => ({ name: "sim", state, text: "" });
const attempt = (state: AttemptView["state"], graded = false) => ({ state, result: graded ? { valid: true } : null }) as AttemptView;

test("flowStep stays at pre-flight until nothing fails", () => {
  assert.equal(flowStep(null, []), 2);
  assert.equal(flowStep([line("ok"), line("fail")], []), 2);
  assert.equal(flowStep([line("ok"), line("warn")], []), 3);
  assert.equal(flowStep([line("fail")], [attempt("abandoned")]), 2);
});

test("flowStep follows the newest attempt", () => {
  assert.equal(flowStep([line("ok")], [attempt("flying")]), 3);
  assert.equal(flowStep([line("ok")], [attempt("rollout")]), 4);
  assert.equal(flowStep([line("ok")], [attempt("done", true), attempt("done", true)]), 5);
  // Graded, but the next attempt is blocked: back to Pre-flight.
  assert.equal(flowStep([line("fail")], [attempt("done", true)]), 2);
});

test("methodOf treats retired landing-2 as landing-3", () => {
  assert.equal(methodOf("landing-2"), "landing-3");
  assert.equal(methodOf("landing-1"), "landing-1");
  assert.equal(methodOf("landing-9"), null);
});

test("partLabel is per method: stability means two things", () => {
  assert.equal(partLabel("landing-1", "stability"), "stability from the gate");
  assert.equal(partLabel("landing-3", "stability"), "approach stability");
  assert.equal(partLabel("landing-3", "something_new"), "something new");
});

test("scoringFact names the challenge's own parts and weights", () => {
  assert.match(scoringFact("landing-1").note, /touchdown point 25, sink rate 25, centreline 15/);
  const three = scoringFact("landing-3");
  assert.match(three.note, /approach stability 25, threshold and flare energy 10, touchdown point 20/);
  assert.doesNotMatch(three.note, /sink rate/);
  assert.match(scoringFact("landing-2").note, /landing-2 is retired/);
  assert.equal(scoringFact("landing-2").value, "landing-3");
});

test("scoringFact says when an attempt shared a domain's weight out", () => {
  const score = { version: "landing-3", total: 80, parts: [{ name: "stability", weight: 27.8, score: 1, value: null, unit: "" }, { name: "point", weight: 22.2, score: 1, value: null, unit: "" }] };
  assert.match(scoringFact("landing-3", score).note, /Latest attempt: approach stability 27.8, touchdown point 22.2/);
  assert.doesNotMatch(scoringFact("landing-3").note, /Latest attempt/);
});
