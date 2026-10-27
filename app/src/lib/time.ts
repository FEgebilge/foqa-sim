import { useEffect, useLayoutEffect, useRef, useState } from "react";

/**
 * The hovered time, shared by charts, timeline and map without re-rendering React on every mouse move.
 * `null` means nothing is hovered.
 */
export class TimeBus {
  private listeners = new Set<(t: number | null) => void>();
  private last: number | null = null;
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

/** Runs `listener` for each hovered time on the bus. */
export function useHover(bus: TimeBus, listener: (t: number | null) => void) {
  useEffect(() => bus.subscribe(listener), [bus, listener]);
}
