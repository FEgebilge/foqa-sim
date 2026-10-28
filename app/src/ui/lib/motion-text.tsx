"use client";

import { useState } from "react";
import type { CSSProperties, Ref } from "react";
import { AnimatePresence, motion, useIsPresent } from "motion/react";
import type { HTMLMotionProps, Transition, Variants } from "motion/react";
import { motionTokens } from "@ui/lib/motion-tokens";

/**
 * Text that changes in place. The new copy rises out of a soft blur while the old one lifts away a little faster, in the
 * same slot, and numbers roll like an odometer. Lib ships without CSS, so the stacking is inline: the slot is a one-cell
 * grid and every copy sits in that cell, and `popLayout` pops the leaving copy out of flow so the slot takes the new
 * size at once. Pass `reduce` from `useReducedMotion()`: reduced motion keeps a short fade and drops travel and blur.
 */
const exitFast: Transition = { duration: motionTokens.duration.fast, ease: [...motionTokens.ease.standard] };
const enter: Transition = { duration: motionTokens.duration.standard, ease: [...motionTokens.ease.enter] };
const instant: Transition = { duration: 0 };
const fadeOut = { opacity: 0, transition: { duration: motionTokens.duration.instant } };
const rest = { opacity: 1, y: "0em", filter: "blur(0px)", transitionEnd: { filter: "none" } };
const slotStyle: CSSProperties = { position: "relative", display: "inline-grid", maxWidth: "100%", verticalAlign: "top" };
const lineStyle: CSSProperties = { gridArea: "1 / 1", display: "block" };

/** Outgoing copies are hidden from assistive tech while they leave, so live text reads only the current value. */
export function Swap({ ref, ...props }: HTMLMotionProps<"span"> & { ref?: Ref<HTMLSpanElement> }) {
  const present = useIsPresent();
  return <motion.span ref={ref} {...props} aria-hidden={present ? props["aria-hidden"] : true} />;
}

const riseVariants: Variants = {
  enter: (direction: number) => ({ opacity: 0, y: `${.3 * direction}em`, filter: `blur(${motionTokens.blur.soft}px)` }),
  center: rest,
  exit: (direction: number) => ({ opacity: 0, y: `${-.3 * direction}em`, filter: `blur(${motionTokens.blur.subtle}px)`, transition: exitFast }),
};
const fadeVariants: Variants = { enter: { opacity: 0 }, center: { opacity: 1 }, exit: fadeOut };

export interface SwapTextProps {
  children: string;
  reduce: boolean | null;
  /** 1 rises from below (the default), -1 drops from above, for values that move back. Leaving copy follows the latest direction. */
  direction?: 1 | -1;
  /** Applies to the slot that holds both copies. */
  className?: string;
}

/** A label, status or rubric line that swaps in place: the new copy rises .3em out of a soft blur and the old one lifts away. */
export function SwapText({ children, reduce, direction = 1, className }: SwapTextProps) {
  return <span className={className} style={slotStyle}>
    <AnimatePresence mode="popLayout" initial={false} custom={direction}>
      <Swap key={children} style={lineStyle} custom={direction} variants={reduce ? fadeVariants : riseVariants} initial="enter" animate="center" exit="exit" transition={reduce ? instant : enter}>{children}</Swap>
    </AnimatePresence>
  </span>;
}

const rollVariants: Variants = {
  enter: (direction: number) => ({ opacity: 0, y: direction >= 0 ? "0.7em" : "-0.7em", filter: `blur(${motionTokens.blur.subtle}px)` }),
  center: rest,
  exit: (direction: number) => ({ opacity: 0, y: direction >= 0 ? "-0.7em" : "0.7em", filter: `blur(${motionTokens.blur.subtle}px)`, transition: exitFast }),
};

export interface RollingNumberProps {
  value: number;
  /** What is shown, such as "9+" or "1,204"; defaults to the plain value. A new display rolls even when the value holds. */
  display?: string;
  reduce: boolean | null;
  className?: string;
}

/** Counts roll like an odometer: a higher number rises from below, a lower one drops from above. The y travel rides the
 *  snappy spring while opacity and blur tween fast, so the blur never overshoots. */
export function RollingNumber({ value, display = String(value), reduce, className }: RollingNumberProps) {
  // Direction is derived during render from the previous value, so the first frame of a change already knows its way.
  const [previous, setPrevious] = useState(value);
  const [direction, setDirection] = useState(0);
  if (value !== previous) { setDirection(value > previous ? 1 : -1); setPrevious(value); }
  return <span className={className} style={slotStyle}>
    <AnimatePresence mode="popLayout" initial={false} custom={direction}>
      <Swap key={display} style={lineStyle} custom={direction} variants={reduce ? fadeVariants : rollVariants} initial="enter" animate="center" exit="exit" transition={reduce ? instant : { y: motionTokens.spring.snappy, opacity: exitFast, filter: exitFast }}>{display}</Swap>
    </AnimatePresence>
  </span>;
}
