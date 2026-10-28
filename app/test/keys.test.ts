import assert from "node:assert/strict";
import test from "node:test";
import { shortcutAllowed } from "../src/lib/keys.ts";

// The test runs without a DOM: a stand-in Element whose closest() matches the selectors it is told about.
class FakeElement {
  tags: string[];
  constructor(tags: string[]) { this.tags = tags; }
  closest(selector: string) { return this.tags.some((tag) => selector.split(",").some((s) => s.trim() === tag)) ? this : null; }
}
(globalThis as { Element?: unknown }).Element = FakeElement;
const on = (...tags: string[]) => new FakeElement(tags) as unknown as EventTarget;

test("Space and the arrows are left to a focused button, field or menu", () => {
  for (const key of [" ", "ArrowLeft", "ArrowRight"]) {
    assert.equal(shortcutAllowed(on(), key), true);
    assert.equal(shortcutAllowed(on("button"), key), false);
    assert.equal(shortcutAllowed(on("select"), key), false);
    assert.equal(shortcutAllowed(on("input"), key), false);
    assert.equal(shortcutAllowed(on("[role='menu']"), key), false);
  }
});

test("Escape resets the zoom unless a menu, dialog or field is handling it", () => {
  assert.equal(shortcutAllowed(on("button"), "Escape"), true);
  assert.equal(shortcutAllowed(on("input"), "Escape"), false);
  assert.equal(shortcutAllowed(on("[role='menu']"), "Escape"), false);
  assert.equal(shortcutAllowed(on("dialog[open]"), "Escape"), false);
  assert.equal(shortcutAllowed(null, "Escape"), true);
});
