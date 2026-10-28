import assert from "node:assert/strict";
import test from "node:test";
import { autoCloses, Countdown, NOTICE_MS, type Timers } from "../src/lib/notice-timing.ts";

/** A clock the test moves by hand. */
function fake() {
  let now = 0;
  const pending = new Map<number, { at: number; run: () => void }>();
  let next = 1;
  const timers: Timers = {
    set: (run, ms) => { pending.set(next, { at: now + ms, run }); return next++; },
    clear: (handle) => { pending.delete(handle as number); },
    now: () => now,
  };
  const advance = (ms: number) => {
    now += ms;
    for (const [id, t] of [...pending]) if (t.at <= now) { pending.delete(id); t.run(); }
  };
  return { timers, advance };
}

test("only a plain notice closes by itself", () => {
  assert.equal(autoCloses({}), true);
  assert.equal(autoCloses({ tone: "info" }), true);
  assert.equal(autoCloses({ tone: "error" }), false);
  assert.equal(autoCloses({ action: { label: "Open debrief" } }), false);
});

test("a countdown fires once its time is up", () => {
  const { timers, advance } = fake();
  let fired = 0;
  new Countdown(() => fired++, NOTICE_MS, timers);
  advance(NOTICE_MS - 1);
  assert.equal(fired, 0);
  advance(1);
  assert.equal(fired, 1);
});

test("pausing keeps the time already spent, and resuming runs only the rest", () => {
  const { timers, advance } = fake();
  let fired = 0;
  const c = new Countdown(() => fired++, 10_000, timers);
  advance(4_000);
  c.pause();
  assert.equal(c.remaining, 6_000);
  advance(60_000);
  assert.equal(fired, 0);
  c.resume();
  advance(5_999);
  assert.equal(fired, 0);
  advance(1);
  assert.equal(fired, 1);
});
