import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { GLOSSARY, isTermId } from "../src/lib/glossary.ts";

const SRC = join(import.meta.dirname, "../src");
const files = (dir: string): string[] => readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
  e.isDirectory() ? (e.name === "ui" ? [] : files(join(dir, e.name))) : e.name.endsWith(".tsx") ? [join(dir, e.name)] : []);

test("every <Term id> in the app is in the glossary", () => {
  const used = files(SRC).flatMap((file) => [...readFileSync(file, "utf8").matchAll(/<Term id="([^"]+)"/g)].map((m) => m[1]!));
  assert.ok(used.length > 10, "the scan finds the terms");
  assert.deepEqual(used.filter((id) => !isTermId(id)), []);
});

test("every entry has a term and a definition that ends a sentence", () => {
  for (const [id, entry] of Object.entries(GLOSSARY)) {
    assert.ok(entry.term && entry.text.length > 20, id);
    assert.match(entry.text, /[.)]$/, id);
  }
});
