"use client";

import { useCallback, useEffect, useRef, useState } from "react";

/**
 * A polite screen-reader voice for a component. Render `notice` in a visually hidden `aria-live="polite"` element that
 * is always mounted, and call `say()` with full sentences. A repeated message gets a trailing non-breaking space, so
 * the second "Moved to Review" is announced too. An empty message clears the region. With `delay` (ms) only the last message in a burst is spoken, so a
 * shift-click range or a held arrow key does not flood the reader.
 */
export function useAnnouncer(delay = 0): { notice: string; say(message: string): void } {
  const [notice, setNotice] = useState("");
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const say = useCallback((message: string) => {
    const speak = () => setNotice(previous => previous === message && message ? `${message} ` : message);
    clearTimeout(timer.current);
    if (delay > 0) timer.current = setTimeout(speak, delay);
    else speak();
  }, [delay]);
  return { notice, say };
}
