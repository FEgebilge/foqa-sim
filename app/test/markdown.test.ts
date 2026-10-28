import assert from "node:assert/strict";
import test from "node:test";
import { blocks, spans } from "../src/lib/markdown-blocks.ts";

test("spans reads bold, code and links in order", () => {
  assert.deepEqual(spans("a **b** `c` [d](https://e.f) g"), [
    { kind: "text", text: "a " }, { kind: "bold", text: "b" }, { kind: "text", text: " " }, { kind: "code", text: "c" },
    { kind: "text", text: " " }, { kind: "link", text: "d", href: "https://e.f" }, { kind: "text", text: " g" },
  ]);
  assert.deepEqual(spans("[x](javascript:alert(1))"), [{ kind: "text", text: "[x](javascript:alert(1))" }]);
});

test("blocks nests one level and keeps a list together across a blank line", () => {
  const out = blocks("Intro line\ncontinued\n\n## Heading\n\n- one\n  - nested **bold**\n- two\n\n- three\n\nAfter");
  assert.deepEqual(out, [
    { kind: "para", text: "Intro line continued" },
    { kind: "heading", text: "Heading" },
    { kind: "list", items: [{ text: "one", children: ["nested **bold**"] }, { text: "two", children: [] }, { text: "three", children: [] }] },
    { kind: "para", text: "After" },
  ]);
});

test("blocks handles CRLF and leaves nothing for an empty source", () => {
  assert.deepEqual(blocks("- a\r\n- b"), [{ kind: "list", items: [{ text: "a", children: [] }, { text: "b", children: [] }] }]);
  assert.deepEqual(blocks(""), []);
});
