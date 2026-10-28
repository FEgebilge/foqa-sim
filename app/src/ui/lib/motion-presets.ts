import type { Transition, Variants } from "motion/react";
import { motionTokens } from "@ui/lib/motion-tokens";

/**
 * Shared variant sets built from motion tokens, so recurring recipes (floating menus, small icon buttons, width slots)
 * live in one place. Every set uses the same three states: "hidden" (before entering), "shown" and "gone" (leaving),
 * so a set works both with AnimatePresence (initial="hidden" animate="shown" exit="gone") and as a two-state toggle
 * (animate={on ? "shown" : "gone"}). Reduced motion keeps short crossfades and drops scale, blur and travel.
 */
const enter = motionTokens.ease.enter;
const standard = motionTokens.ease.standard;
const instant = { duration: motionTokens.duration.instant };
const leave = { duration: motionTokens.duration.instant, ease: standard };

/** Floating menus drop in from the anchor's side with a snappy settle and leave faster than they came. */
function popover(reduce: boolean | null, side: "top" | "bottom" = "bottom"): Variants {
  if (reduce) return { hidden: { opacity: 0 }, shown: { opacity: 1, transition: instant }, gone: { opacity: 0, transition: instant } };
  const away = side === "top" ? 1 : -1;
  return {
    hidden: { opacity: 0, y: 6 * away, scale: 0.97 },
    shown: { opacity: 1, y: 0, scale: 1, transition: { ...motionTokens.spring.snappy, opacity: { duration: motionTokens.duration.fast, ease: enter } } },
    gone: { opacity: 0, y: 4 * away, scale: 0.98, transition: leave },
  };
}

/** Small icon buttons (clear, remove) pop in from .6 with a subtle blur and shrink back out on an instant curve. */
function pop(reduce: boolean | null): Variants {
  if (reduce) return { hidden: { opacity: 0 }, shown: { opacity: 1, transition: instant }, gone: { opacity: 0, transition: leave } };
  const blur = `blur(${motionTokens.blur.subtle}px)`;
  return {
    hidden: { opacity: 0, scale: 0.6, filter: blur },
    // Scale rides the spring; opacity and blur tween so the blur never overshoots below zero, then the filter is dropped.
    shown: {
      opacity: 1, scale: 1, filter: "blur(0px)", transitionEnd: { filter: "none" },
      transition: { ...motionTokens.spring.snappy, opacity: { duration: motionTokens.duration.fast }, filter: { duration: motionTokens.duration.fast, ease: enter } },
    },
    gone: { opacity: 0, scale: 0.6, filter: blur, transition: leave },
  };
}

/** A clipped slot whose width springs open and closed, so whatever sits beside it travels with it instead of jumping. */
function slot(reduce: boolean | null): Variants {
  if (reduce) return { hidden: { width: 0, opacity: 0 }, shown: { width: "auto", opacity: 1, transition: { duration: 0, opacity: instant } }, gone: { width: 0, opacity: 0, transition: { duration: 0, opacity: instant } } };
  return {
    hidden: { width: 0, opacity: 0 },
    shown: { width: "auto", opacity: 1, transition: { width: motionTokens.spring.smooth, opacity: { duration: motionTokens.duration.fast, ease: enter } } },
    gone: { width: 0, opacity: 0, transition: { width: motionTokens.spring.smooth, opacity: leave } },
  };
}

/** Press feedback for small icon buttons: a quick dip that never scales an anchor. */
function press(reduce: boolean | null) {
  return { scale: reduce ? 1 : 0.96, transition: leave };
}

const blurred = (radius: number) => `blur(${radius}px)`;
const fast = { duration: motionTokens.duration.fast, ease: enter };

/** Height opens from 0 to auto on the smooth spring while the content fades in, and closes the same way with a faster
 *  fade, so the rows below glide instead of jumping. The element needs `overflow: hidden` (or clip) in its own CSS.
 *  Reduced motion snaps the height and keeps an instant fade; on the way out the fade plays before the height closes. */
function collapse(reduce: boolean | null): Variants {
  if (reduce) return {
    hidden: { height: 0, opacity: 0 },
    shown: { height: "auto", opacity: 1, transition: { height: { duration: 0 }, opacity: instant } },
    gone: { height: 0, opacity: 0, transition: { height: { duration: 0, delay: motionTokens.duration.instant }, opacity: instant } },
  };
  return {
    hidden: { height: 0, opacity: 0 },
    shown: { height: "auto", opacity: 1, transition: { height: motionTokens.spring.smooth, opacity: { duration: motionTokens.duration.standard, ease: enter } } },
    gone: { height: 0, opacity: 0, transition: { height: motionTokens.spring.smooth, opacity: { duration: motionTokens.duration.fast, ease: standard } } },
  };
}

/** Content that arrives in place (a swapped pane, a new log entry) settles up from 6px out of a soft blur; it leaves on an
 *  instant curve, lifting slightly, so the next content never waits on it. */
function rise(reduce: boolean | null): Variants {
  if (reduce) return { hidden: { opacity: 0 }, shown: { opacity: 1, transition: instant }, gone: { opacity: 0, transition: instant } };
  return {
    hidden: { opacity: 0, y: 6, filter: blurred(motionTokens.blur.soft) },
    shown: { opacity: 1, y: 0, filter: blurred(0), transitionEnd: { filter: "none" }, transition: { duration: motionTokens.duration.standard, ease: enter } },
    gone: { opacity: 0, y: -4, filter: blurred(motionTokens.blur.subtle), transition: leave },
  };
}

/** Bars that dock to the bottom edge rise 20px on the smooth spring with a quick fade; they sink back a shorter way. */
function dock(reduce: boolean | null): Variants {
  if (reduce) return { hidden: { opacity: 0 }, shown: { opacity: 1, transition: instant }, gone: { opacity: 0, transition: instant } };
  return {
    hidden: { opacity: 0, y: 20, scale: 0.98, filter: blurred(motionTokens.blur.subtle) },
    shown: { opacity: 1, y: 0, scale: 1, filter: blurred(0), transitionEnd: { filter: "none" }, transition: { ...motionTokens.spring.smooth, opacity: fast, filter: fast } },
    gone: { opacity: 0, y: 14, scale: 0.98, transition: { duration: motionTokens.duration.fast, ease: standard } },
  };
}

/** A short, decaying side-to-side shake: a refusal reads as "no" without moving anything else. It is pure travel, so
 *  callers skip it when motion is reduced: `if (!reduce) animate(node, shake.keyframes, shake.transition)`. */
const shake: { keyframes: { x: number[] }; transition: Transition } = { keyframes: { x: [0, -7, 6, -4, 3, -1.5, 0] }, transition: { duration: 0.42, ease: "easeOut" } };

/** Delay for the nth item of a bulk change, capped so the whole sweep stays under a quarter second. */
function cascade(index: number, cap = 0.2) {
  return Math.min(index * motionTokens.stagger.item, cap);
}

export const motionPresets = { popover, pop, slot, press, collapse, rise, dock, shake, cascade };
