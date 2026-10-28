"use client";

import { useEffect, useRef, useState } from "react";
import { AnimatePresence, animate, motion, useMotionValue, useReducedMotion, type Variants } from "motion/react";
import { useArcLocale } from "@ui/lib/arc-provider";
import { motionTokens } from "@ui/lib/motion-tokens";
import type { Tone } from "@ui/lib/tone";
import { AnimatedCounter } from "../animated-counter/animated-counter";
import styles from "./metric-card.module.css";
export interface MetricCardProps {
  label: string;
  value: number;
  prefix?: string;
  suffix?: string;
  decimals?: number;
  /** Defaults to the ArcProvider locale. */
  locale?: string;
  /** A line of context under the number, such as the period. Optional: a bare number reads fine. */
  context?: string;
  change?: string;
  /** Tints a top edge. Words in `context` or `change` must carry the meaning; the tone is only a cue. */
  tone?: Tone;
  /** When the number rolls up from zero: as the card scrolls into view (default), on mount, or never. Reduced motion always shows the value at once. */
  animate?: "view" | "mount" | false;
}

/** Copy that holds a number enters from the side it moved toward: a larger value rises from below, a smaller one drops from above. */
const rise: Variants = { hidden: (direction: number) => ({ opacity: 0, y: `${.3 * direction}em`, filter: `blur(${motionTokens.blur.soft}px)` }), shown: { opacity: 1, y: 0, filter: "blur(0px)", transition: { duration: motionTokens.duration.standard, ease: [...motionTokens.ease.enter] } }, gone: (direction: number) => ({ opacity: 0, y: `${-.3 * direction}em`, filter: `blur(${motionTokens.blur.subtle}px)`, transition: { duration: motionTokens.duration.fast, ease: [...motionTokens.ease.standard] } }) };
const fade: Variants = { hidden: { opacity: 0, y: 0, filter: "blur(0px)" }, shown: { opacity: 1, y: 0, filter: "blur(0px)", transition: { duration: motionTokens.duration.instant } }, gone: { opacity: 0, y: 0, filter: "blur(0px)", transition: { duration: motionTokens.duration.instant } } };

/** New copy rises in while the old copy leaves; `morph` springs the wrapper to the new text's width instead of letting it snap. */
function Swap({ text, value, morph = false, block = false }: { text: string; value: number; morph?: boolean; block?: boolean }) {
  const reduceMotion = !!useReducedMotion();
  const sizer = useRef<HTMLSpanElement>(null);
  const width = useMotionValue<number | "auto">("auto");
  // The direction follows the card's number, not a parse of the text: "1.234,5" is not 1.2345.
  const [shown, setShown] = useState({ text, value, direction: 1 });
  if (shown.text !== text || shown.value !== value) setShown({ text, value, direction: value < shown.value ? -1 : 1 });
  useEffect(() => {
    const node = sizer.current;
    if (!node || typeof ResizeObserver === "undefined") return;
    let measured: string | null = null;
    // Layout size, not the transformed rect, so a scaling parent never leaves the text clipped. Only a new text springs; font loads jump.
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const next = entry.borderBoxSize?.[0]?.inlineSize ?? node.offsetWidth;
      if (next && measured !== null && measured !== node.textContent && !reduceMotion) animate(width, next, motionTokens.spring.morph);
      else width.jump(next || "auto");
      measured = next ? node.textContent : null;
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [morph, reduceMotion, width]);
  return <motion.span className={block ? styles.swapBlock : styles.swap} style={morph ? { width } : undefined}>
    {morph && <span ref={sizer} className={styles.sizer} aria-hidden="true">{text}</span>}
    <AnimatePresence mode="popLayout" initial={false} custom={shown.direction}><motion.span key={text} className={styles.text} custom={shown.direction} variants={reduceMotion ? fade : rise} initial="hidden" animate="shown" exit="gone">{text}</motion.span></AnimatePresence>
  </motion.span>;
}

export function MetricCard({ label, value, prefix, suffix, decimals, locale: localeProp, context, change, tone, animate = "view" }: MetricCardProps) {
  const reduceMotion = !!useReducedMotion();
  const { locale: arcLocale } = useArcLocale();
  const locale = localeProp ?? arcLocale;
  return <article className={styles.card} data-tone={tone}>
    <div className={styles.top}><span><Swap text={label} value={value} block /></span><AnimatePresence initial={false}>{change && <motion.small key="change" data-trend={/^[+]/.test(change) ? "up" : /^[-−]/.test(change) ? "down" : undefined} initial={{ opacity: 0, scale: reduceMotion ? 1 : .96 }} animate={{ opacity: 1, scale: 1 }} exit={{ opacity: 0, scale: reduceMotion ? 1 : .96, transition: { duration: motionTokens.duration.fast, ease: [...motionTokens.ease.standard] } }} transition={reduceMotion ? { duration: 0 } : motionTokens.spring.snappy}><Swap text={change} value={value} morph /></motion.small>}</AnimatePresence></div>
    <AnimatedCounter value={value} prefix={prefix} suffix={suffix} decimals={decimals} locale={locale} animateOnView={!reduceMotion && animate !== false && (animate === "mount" ? "mount" : true)} />
    {context && <p><Swap text={context} value={value} block /></p>}
  </article>;
}
