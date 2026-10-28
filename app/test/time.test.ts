import assert from "node:assert/strict";
import test from "node:test";
import { TimeBus } from "../src/lib/time.ts";

test("a placed cursor notifies its listeners once per move, until they stop", () => {
  const bus = new TimeBus();
  assert.equal(bus.placed, null);
  let calls = 0;
  const stop = bus.onPlace(() => calls++);
  bus.place(5);
  bus.place(5);
  bus.place(6);
  assert.deepEqual([bus.placed, calls], [6, 2]);
  stop();
  bus.place(7);
  assert.deepEqual([bus.placed, calls], [7, 2]);
});

test("the placed cursor is apart from the hovered time", () => {
  const bus = new TimeBus();
  const hovered: (number | null)[] = [];
  bus.subscribe((t) => hovered.push(t));
  bus.place(5);
  bus.emit(3);
  bus.emit(null);
  assert.deepEqual([bus.placed, hovered], [5, [3, null]]);
});
