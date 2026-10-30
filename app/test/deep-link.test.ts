// node --experimental-strip-types --test test/ (npm test): the app's pure logic, without a browser.
import assert from "node:assert/strict";
import test from "node:test";
import { linkNotice, linkPrompt, linkWhat } from "../src/lib/deep-link.ts";
import type { DeepLink } from "../src/lib/sites.ts";

const link = (over: Partial<DeepLink>): DeepLink =>
  ({ state: "open", seq: 4, kind: "challenge", origin: "https://a.example", host: "a.example", id: "ltfj-06", v: 2, signed_in: true, ...over });

test("a signed-in site's challenge link flies at once; any other link asks first", () => {
  assert.deepEqual(linkPrompt(link({ state: "ask", signed_in: false })), { title: "Connect to a.example?", confirm: "Connect", action: "connect" });
  assert.deepEqual(linkPrompt(link({ signed_in: false })), { title: "Sign in to a.example?", confirm: "Sign in and fly", action: "sign_in" });
  // The click on the pilot's own signed-in site was the consent: no question, it flies.
  assert.deepEqual(linkPrompt(link({})), { title: "Fly ltfj-06 from a.example", confirm: "Fly", action: "open" });
});

test("a connect link to a signed-in site has nothing to ask", () => {
  assert.equal(linkPrompt(link({ kind: "connect", id: undefined, v: undefined })), null);
  assert.equal(linkPrompt(link({ kind: "connect", id: undefined, signed_in: false }))?.confirm, "Sign in");
  assert.equal(linkPrompt(link({ state: "refused", text: "x" })), null);
});

test("the link's challenge and notices in words", () => {
  assert.equal(linkWhat(link({})), "the challenge ltfj-06, version 2");
  assert.equal(linkWhat(link({ v: undefined })), "the challenge ltfj-06");
  assert.equal(linkWhat(link({ kind: "connect", id: undefined })), "its challenges");
  assert.equal(linkNotice(link({ state: "refused" })), "This link couldn't be opened");
  assert.equal(linkNotice(link({ state: "disabled" })), "Sites are disabled in this build");
  assert.equal(linkNotice(link({})), null);
});
