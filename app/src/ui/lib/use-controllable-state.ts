"use client";

import { useCallback, useState } from "react";

/**
 * Value state that is either owned by the parent (`value` defined) or kept inside (`value` undefined, seeded from
 * `defaultValue`). While controlled it never writes the internal copy, so handing control back later starts from the
 * default rather than from a stale echo. `null` counts as a controlled empty value; only `undefined` means uncontrolled.
 * The setter always reports the change through `onChange`, so the parent decides whether it sticks.
 */
export function useControllableState<T>(value: T | undefined, defaultValue: T, onChange?: (next: T) => void): [T, (next: T) => void] {
  const [internal, setInternal] = useState(defaultValue);
  const controlled = value !== undefined;
  const set = useCallback((next: T) => {
    if (!controlled) setInternal(next);
    onChange?.(next);
  }, [controlled, onChange]);
  return [controlled ? value : internal, set];
}
