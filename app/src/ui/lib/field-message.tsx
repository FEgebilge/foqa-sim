"use client";

import { useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { motionTokens } from "@ui/lib/motion-tokens";
import { RollingNumber } from "@ui/lib/motion-text";

/**
 * Helper and error copy shared by the form controls. Lib ships without CSS, so the stacking and clipping the motion needs
 * is inline; callers pass `className` for colour, size and spacing.
 */

/** Joins the ids a control points `aria-describedby` at, dropping empty ones; undefined when none are left. */
export const joinDescribedBy = (...ids: (string | false | null | undefined)[]) => ids.filter(Boolean).join(" ") || undefined;

const slotStyle: CSSProperties = { display: "block", overflow: "hidden" };
const copyStyle: CSSProperties = { display: "block" };
const wordsStyle: CSSProperties = { position: "relative", display: "block" };
const wordStyle: CSSProperties = { display: "inline-block", whiteSpace: "pre" };
const srOnlyStyle: CSSProperties = { position: "absolute", width: 1, height: 1, overflow: "hidden", clipPath: "inset(50%)", whiteSpace: "nowrap" };
const isNumber = (word: string) => /^\d[\d.,/:]*%?$/.test(word);

/** Changed words rise in and unblur while unchanged words hold still, and numbers roll. Assistive tech reads the plain copy. */
function MotionText({ text, reduced }: { text: string; reduced: boolean | null }) {
  const words = text.split(" ");
  return <><span style={srOnlyStyle}>{text}</span><span style={wordsStyle} aria-hidden="true"><AnimatePresence initial={false} mode="popLayout">{words.map((word, index) => <motion.span key={`${index}:${isNumber(word) ? "#" : word}`} style={wordStyle}
    initial={reduced ? { opacity: 0 } : { opacity: 0, y: "0.35em", filter: `blur(${motionTokens.blur.soft}px)` }} animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
    exit={reduced ? { opacity: 0, transition: { duration: 0 } } : { opacity: 0, y: "-0.35em", filter: `blur(${motionTokens.blur.subtle}px)`, transition: { duration: motionTokens.duration.exit, ease: [...motionTokens.ease.standard] } }}
    transition={reduced ? { duration: motionTokens.duration.instant } : { duration: motionTokens.duration.standard, ease: [...motionTokens.ease.enter] }}>{isNumber(word) ? <RollingNumber value={parseFloat(word.replace(/,/g, ""))} display={word} reduce={reduced} /> : word}{index < words.length - 1 ? " " : null}</motion.span>)}</AnimatePresence></span></>;
}

/** The row tracks the measured copy, so a longer message that wraps opens its next line instead of snapping. */
function MessageRow({ id, text, className, alert }: { id?: string; text: string; className?: string; alert?: boolean }) {
  const reduced = useReducedMotion();
  const copyRef = useRef<HTMLSpanElement>(null);
  const [height, setHeight] = useState<number | "auto">("auto");
  useEffect(() => {
    const node = copyRef.current;
    if (!node || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(([entry]) => { if (entry) setHeight(entry.borderBoxSize?.[0]?.blockSize ?? node.offsetHeight); });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  // Reduced motion mounts the row at full height: a zero-duration open would still paint one collapsed frame. The presence starts
  // with initial={false}, so rows present at hydration render the same on server and client either way.
  return <motion.span style={slotStyle} initial={reduced ? false : { height: 0, opacity: 0 }} animate={{ height, opacity: 1 }} exit={{ height: 0, opacity: 0, transition: reduced ? { duration: 0 } : { height: motionTokens.spring.smooth, opacity: { duration: motionTokens.duration.instant } } }} transition={reduced ? { duration: 0 } : { height: motionTokens.spring.smooth, opacity: { duration: motionTokens.duration.fast } }}>
    <motion.span ref={copyRef} id={id} className={className} style={copyStyle} role={alert ? "alert" : undefined} initial={reduced ? false : { y: "0.35em", filter: `blur(${motionTokens.blur.soft}px)` }} animate={{ y: 0, filter: "blur(0px)" }} transition={{ duration: reduced ? 0 : motionTokens.duration.standard, ease: [...motionTokens.ease.enter] }}><MotionText text={text} reduced={reduced} /></motion.span>
  </motion.span>;
}

export interface FieldMessageProps {
  /** Goes on the copy, so `aria-describedby` reads it. */
  id?: string;
  /** Nothing renders while empty; a change in text animates in place and an empty-to-text change opens the row. */
  text?: string;
  /** Colour, size and spacing of the copy. */
  className?: string;
  /** Marks the row `role="alert"`, for errors. */
  alert?: boolean;
}

/** Helper or error copy under a control: the row opens its height on a spring, then the words settle in. */
export function FieldMessage({ id, text, className, alert }: FieldMessageProps) {
  return <AnimatePresence initial={false}>{text ? <MessageRow key="message" id={id} text={text} className={className} alert={alert} /> : null}</AnimatePresence>;
}
