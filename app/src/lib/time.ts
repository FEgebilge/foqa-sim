import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";

/**
 * The hovered time, shared by charts, timeline and map without re-rendering React on every mouse move.
 * `null` means nothing is hovered. It also holds the placed cursor (a click, a step, playback), so a tick of
 * playback redraws only what shows the cursor, not the whole debrief.
 */
export class TimeBus {
  private listeners = new Set<(t: number | null) => void>();
  private last: number | null = null;
  private placedListeners = new Set<() => void>();
  private cursor: number | null = null;
  /** The placed cursor's time; `null` before one is placed. */
  get placed() { return this.cursor; }
  place(t: number) {
    if (t === this.cursor) return;
    this.cursor = t;
    this.placedListeners.forEach((listener) => listener());
  }
  /** Calls `listener` when the cursor is placed; returns how to stop. An arrow, so React can subscribe with it. */
  onPlace = (listener: () => void) => {
    this.placedListeners.add(listener);
    return () => { this.placedListeners.delete(listener); };
  };
  emit(t: number | null) {
    if (t === this.last) return;
    this.last = t;
    this.listeners.forEach((listener) => listener(t));
  }
  subscribe(listener: (t: number | null) => void) {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  }
}

export type TimeRange = [start: number, end: number];

/** "Nice" tick step in seconds for a span, aiming at about `count` ticks. */
export function tickStep(span: number, count: number) {
  const steps = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200];
  return steps.find((step) => span / step <= count) ?? 14400;
}

export function useElementWidth<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const node = ref.current;
    if (!node) return;
    setWidth(node.clientWidth);
    const observer = new ResizeObserver(() => setWidth(node.clientWidth));
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  return [ref, width] as const;
}

/** The bus's placed cursor, re-rendering the caller when it moves. */
export const usePlaced = (bus: TimeBus) => useSyncExternalStore(bus.onPlace, () => bus.placed);

/** Runs `listener` for each hovered time on the bus. */
export function useHover(bus: TimeBus, listener: (t: number | null) => void) {
  useEffect(() => bus.subscribe(listener), [bus, listener]);
}
