/** How long a plain notice stays. */
export const NOTICE_MS = 12_000;

/** Whether a notice closes by itself: only a plain one. An error must be read, and one with an action (open the debrief) must stay available until used. */
export const autoCloses = (notice: { tone?: string; action?: unknown }) => notice.tone !== "error" && !notice.action;

export interface Timers {
  set: (run: () => void, ms: number) => unknown;
  clear: (handle: unknown) => void;
  now: () => number;
}
const BROWSER: Timers = { set: (run, ms) => window.setTimeout(run, ms), clear: (handle) => window.clearTimeout(handle as number), now: () => Date.now() };

/** A countdown that can be paused (pointer over the notice, focus inside it) and resumed without restarting: the time already spent counts. */
export class Countdown {
  private left: number;
  private handle: unknown = null;
  private since = 0;
  private done: () => void;
  private timers: Timers;
  constructor(done: () => void, ms = NOTICE_MS, timers: Timers = BROWSER) {
    this.done = done;
    this.left = ms;
    this.timers = timers;
    this.resume();
  }
  /** Milliseconds still to run. */
  get remaining() { return this.handle === null ? this.left : Math.max(0, this.left - (this.timers.now() - this.since)); }
  pause() {
    if (this.handle === null) return;
    this.left = this.remaining;
    this.timers.clear(this.handle);
    this.handle = null;
  }
  resume() {
    if (this.handle !== null) return;
    this.since = this.timers.now();
    this.handle = this.timers.set(this.done, this.left);
  }
}
