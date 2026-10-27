// node --experimental-strip-types --test test/ (npm test): the app's pure logic, without a browser.
import assert from "node:assert/strict";
import test from "node:test";
import { linkNotice, linkPrompt, linkWhat } from "../src/lib/deep-link.ts";
import type { DeepLink } from "../src/lib/sites.ts";

const link = (over: Partial<DeepLink>): DeepLink =>
  ({ state: "open", seq: 4, kind: "challenge", origin: "https://a.example", host: "a.example", id: "ltfj-06", v: 2, signed_in: true, ...over });

test("every link waits for a click that says what it does", () => {
  assert.deepEqual(linkPrompt(link({ state: "ask", signed_in: false }), null), { title: "Connect to a.example?", confirm: "Connect", action: "connect" });
  assert.deepEqual(linkPrompt(link({ signed_in: false }), null), { title: "Sign in to a.example?", confirm: "Sign in and open", action: "sign_in" });
  // A signed-in site of the user's no longer opens by itself: it asks, with the id only (no title before the click).
  assert.deepEqual(linkPrompt(link({}), null), { title: "Open ltfj-06 from a.example?", confirm: "Open", action: "open" });
  const conflict = { key: "ltfj-06-v2", title: "LTFJ 06 gusty", version: 2 };
  assert.deepEqual(linkPrompt(link({}), conflict), { title: "Replace your copy of “LTFJ 06 gusty”?", confirm: "Replace", action: "replace" });
});

test("a connect link to a signed-in site has nothing to ask", () => {
  assert.equal(linkPrompt(link({ kind: "connect", id: undefined, v: undefined }), null), null);
  assert.equal(linkPrompt(link({ kind: "connect", id: undefined, signed_in: false }), null)?.confirm, "Sign in");
  assert.equal(linkPrompt(link({ state: "refused", text: "x" }), null), null);
});

test("the link's challenge and notices in words", () => {
  assert.equal(linkWhat(link({})), "the challenge ltfj-06, version 2");
  assert.equal(linkWhat(link({ v: undefined })), "the challenge ltfj-06");
  assert.equal(linkWhat(link({ kind: "connect", id: undefined })), "its challenges");
  assert.equal(linkNotice(link({ state: "refused" })), "This link couldn't be opened");
  assert.equal(linkNotice(link({ state: "disabled" })), "Sites are disabled in this build");
  assert.equal(linkNotice(link({})), null);
});
