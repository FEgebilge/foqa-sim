"use client";

import type { HTMLAttributes } from "react";
import { useLabels } from "@ui/lib/arc-provider";
import styles from "./spinner.module.css";

export type SpinnerSize = "xs" | "sm" | "md" | "lg";
export type SpinnerTone = "current" | "muted" | "accent";

export interface SpinnerLabels { loading: string }
const DEFAULT_LABELS: SpinnerLabels = { loading: "Loading" };
declare module "@ui/lib/arc-provider" { interface ArcLabels { spinner: SpinnerLabels } }

export interface SpinnerProps extends Omit<HTMLAttributes<HTMLSpanElement>, "children"> {
  size?: SpinnerSize;
  /** `current` takes the surrounding text color, so a spinner inside a button or a muted line matches it. */
  tone?: SpinnerTone;
  /** What assistive technology announces. Overrides `labels.loading`. */
  label?: string;
  /** For a spinner whose parent already carries the busy state (a loading button): drops the status role and the label. */
  decorative?: boolean;
  labels?: Partial<SpinnerLabels>;
}

/** A small indeterminate ring. The label is visually hidden text inside a status region, so it is read where a bare aria-label on an empty element is often skipped. */
export function Spinner({ size = "md", tone = "current", label, decorative = false, labels, className, ...props }: SpinnerProps) {
  const t = useLabels("spinner", DEFAULT_LABELS, labels);
  return <span {...props} className={[styles.spinner, styles[size], styles[tone], className].filter(Boolean).join(" ")} role={decorative ? undefined : "status"} aria-hidden={decorative || undefined}>
    <span className={styles.ring} aria-hidden="true" />
    {!decorative && <span className={styles.label}>{label ?? t.loading}</span>}
  </span>;
}

export default Spinner;
