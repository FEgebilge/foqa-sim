"use client";

import { useEffect, useMemo } from "react";
import { AnimatePresence, animate, motion, useIsPresent, useMotionValue, useReducedMotion, useTransform, type HTMLMotionProps, type TargetAndTransition, type Transition } from "motion/react";
import { Check } from "lucide-react";
import { motionTokens } from "@ui/lib/motion-tokens";
import { useArcLocale, useLabels } from "@ui/lib/arc-provider";
import type { Tone } from "@ui/lib/tone";
import styles from "./progress.module.css";

export interface ProgressLabels { progress: string }
const DEFAULT_LABELS: ProgressLabels = { progress: "Progress" };
declare module "@ui/lib/arc-provider" { interface ArcLabels { progress: ProgressLabels } }

export interface ProgressProps extends Omit<React.HTMLAttributes<HTMLDivElement>, "children"> {
  value?: number;
  max?: number;
  label?: string;
  showValue?: boolean;
  /** Colours the fill with a status tone (overrides the accent and the complete green). */
  tone?: Tone;
  labels?: Partial<ProgressLabels>;
  /** BCP 47 locale of the percentage ("%50" in tr-TR); defaults to the provider's. */
  locale?: string;
}

const exitFast: Transition = { duration: motionTokens.duration.fast, ease: [...motionTokens.ease.standard] };
const textIn: TargetAndTransition = { opacity: 0, y: "0.3em", filter: `blur(${motionTokens.blur.soft}px)` };
const textOut: TargetAndTransition = { opacity: 0, y: "-0.3em", filter: `blur(${motionTokens.blur.subtle}px)`, transition: exitFast };
const iconIn: TargetAndTransition = { opacity: 0, scale: .6, filter: `blur(${motionTokens.blur.subtle}px)` };
const shown: TargetAndTransition = { opacity: 1, y: "0em", scale: 1, filter: "blur(0px)" };
const fadeOut: TargetAndTransition = { opacity: 0, transition: { duration: motionTokens.duration.instant } };

/** Outgoing copies are hidden from assistive tech while they fade. */
function Swap(props: HTMLMotionProps<"span">) {
  const present = useIsPresent();
  return <motion.span {...props} aria-hidden={present ? props["aria-hidden"] : true} />;
}

export function Progress({ value = 0, max = 100, label, showValue = false, tone, className, labels, locale: localeProp, ...props }: ProgressProps) {
  const t = useLabels("progress", DEFAULT_LABELS, labels);
  const { locale: arcLocale } = useArcLocale();
  const locale = localeProp ?? arcLocale;
  const percent = useMemo(() => { const format = new Intl.NumberFormat(locale, { style: "percent" }); return (value: number) => format.format(Math.round(value) / 100); }, [locale]);
  const reduce = useReducedMotion();
  const safeMax = max > 0 ? max : 100;
  const safeValue = Math.min(Math.max(value, 0), safeMax);
  const percentage = Math.round((safeValue / safeMax) * 100);
  const complete = percentage >= 100;
  // One spring drives both the fill and the counted label, so the number always matches the bar.
  const progress = useMotionValue(percentage);
  // The fill slides in from the left instead of scaling, so its rounded end keeps its shape at every value.
  const x = useTransform(progress, latest => `${Math.min(Math.max(latest, 0), 100) - 100}%`);
  const counted = useTransform(progress, latest => percent(Math.min(Math.max(latest, 0), 100)));
  useEffect(() => {
    if (reduce) { progress.jump(percentage); return; }
    const controls = animate(progress, percentage, motionTokens.spring.smooth);
    return () => controls.stop();
  }, [percentage, progress, reduce]);
  const enter: Transition = reduce ? { duration: motionTokens.duration.instant } : { duration: motionTokens.duration.standard, ease: [...motionTokens.ease.enter] };
  const classes = [styles.progress, className].filter(Boolean).join(" ");
  return <div {...props} className={classes} data-complete={complete ? "" : undefined} data-tone={tone} role="progressbar" aria-label={label ?? t.progress} aria-valuemin={0} aria-valuemax={safeMax} aria-valuenow={safeValue} aria-valuetext={percent(percentage)}>
    {(label || showValue) ? <div className={styles.meta}>
      {label ? <span className={styles.label}><AnimatePresence mode="popLayout" initial={false}><Swap key={label} className={styles.line} initial={reduce ? { opacity: 0 } : textIn} animate={shown} exit={reduce ? fadeOut : textOut} transition={enter}>{label}</Swap></AnimatePresence></span> : <span />}
      {showValue ? <span className={styles.value}>
        {/* Completion lands as the fill arrives: a check settles in beside the final count. */}
        <AnimatePresence initial={false}>{complete && <motion.span key="done" className={styles.done} initial={reduce ? { opacity: 0 } : iconIn} animate={shown} exit={reduce ? fadeOut : { ...iconIn, transition: exitFast }} transition={reduce ? enter : { ...motionTokens.spring.snappy, delay: .24 }}><Check size={14} strokeWidth={1.75} aria-hidden="true" /></motion.span>}</AnimatePresence>
        <motion.span className={styles.count}>{counted}</motion.span>
      </span> : null}
    </div> : null}
    <div className={styles.track}><motion.span className={styles.fill} style={{ x }} /></div>
  </div>;
}

export default Progress;
