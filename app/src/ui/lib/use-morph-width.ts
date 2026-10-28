"use client";

import { useEffect, useLayoutEffect, useRef } from "react";
import type { RefObject } from "react";
import { animate, useMotionValue } from "motion/react";
import { motionTokens } from "@ui/lib/motion-tokens";

/** Springs a slot to the natural width of its incoming content when `key` changes, so a new label never snaps the
 *  control's size. Put `content` on the element that holds the label and bind the returned value to its parent's
 *  `style.width` (a motion element); the incoming copy should pop the outgoing one out of flow (`popLayout`) so it never
 *  holds the old width. Other resizes (a late web font, a parent reflow) jump straight to the new width, so nothing
 *  wobbles on first paint. While a morph runs the parent carries `data-morphing`, for a clip that lets a blur bleed out. */
export function useMorphWidth(content: RefObject<HTMLElement | null>, key: string, reduced: boolean) {
  const width = useMotionValue<number | "auto">("auto");
  const lastKey = useRef(key), armedUntil = useRef(0);
  useLayoutEffect(() => {
    if (lastKey.current === key) return;
    lastKey.current = key;
    armedUntil.current = performance.now() + 700;
  }, [key]);
  useEffect(() => {
    const node = content.current, slot = node?.parentElement;
    if (!node || !slot || typeof ResizeObserver === "undefined") return;
    let measured = false;
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const next = entry.contentRect.width;
      if (!next || !measured || reduced || performance.now() > armedUntil.current) { measured = next > 0; width.jump(next || "auto"); delete slot.dataset.morphing; return; }
      slot.dataset.morphing = "";
      animate(width, next, { ...motionTokens.spring.morph, onComplete: () => { delete slot.dataset.morphing; } });
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [content, reduced, width]);
  return width;
}
