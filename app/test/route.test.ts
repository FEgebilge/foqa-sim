import assert from "node:assert/strict";
import test from "node:test";
import { flightHref, parse } from "../src/lib/route.ts";

test("routes parse to their screens", () => {
  assert.deepEqual(parse("#/setup"), { screen: "setup" });
  assert.deepEqual(parse("#/challenges"), { screen: "challenges", key: undefined });
  assert.deepEqual(parse("#/challenges/ltfj%2006"), { screen: "challenges", key: "ltfj 06" });
  assert.deepEqual(parse(flightHref("a b/c")), { screen: "debrief", id: "a b/c" });
  assert.deepEqual(parse(""), { screen: "flights" });
});

test("a malformed escape opens the flights list instead of throwing", () => {
  for (const hash of ["#/flight/%", "#/flight/%E0%A4%A", "#/challenges/%zz"]) assert.deepEqual(parse(hash), { screen: "flights" });
});
