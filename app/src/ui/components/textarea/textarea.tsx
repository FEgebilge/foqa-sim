"use client";
import { forwardRef, useId, useState } from "react";
import type { CSSProperties, TextareaHTMLAttributes } from "react";
import { useArcLocale, useLabels } from "@ui/lib/arc-provider";
import { FieldMessage, joinDescribedBy } from "@ui/lib/field-message";
import styles from "./textarea.module.css";
export interface TextareaLabels {
  /** The counter text; both numbers arrive already formatted for the locale. */
  count: (count: string, max: string) => string;
}
const DEFAULT_LABELS: TextareaLabels = { count: (count, max) => `${count} / ${max}` };
declare module "@ui/lib/arc-provider" { interface ArcLabels { textarea: TextareaLabels } }
export interface TextareaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  label: string; description?: string; error?: string;
  /** Keeps the label for assistive tech and drops it visually (dense forms). */
  hideLabel?: boolean;
  /** With `maxLength`, shows "1,240 / 2,000" and warns from `warnAt`. */
  showCount?: boolean;
  /** The share of `maxLength`, 0 to 1, from which the counter warns. */
  warnAt?: number;
  labels?: Partial<TextareaLabels>;
  /** BCP 47 locale for the counter's numbers; defaults to the nearest `ArcProvider`'s. */
  locale?: string;
}
export const Textarea = forwardRef<HTMLTextAreaElement, TextareaProps>(function Textarea({ label, description, error, hideLabel, showCount, warnAt = 0.95, labels, locale: localeProp, id, className, style, onChange, ...props }, ref) {
  const t = useLabels("textarea", DEFAULT_LABELS, labels);
  const { locale: arcLocale } = useArcLocale();
  const format = (value: number) => value.toLocaleString(localeProp ?? arcLocale);
  const generatedId = useId(); const controlId = id ?? generatedId;
  const hintId = description ? `${controlId}-description` : undefined; const errorId = error ? `${controlId}-error` : undefined;
  const max = showCount ? props.maxLength : undefined;
  const countId = max ? `${controlId}-count` : undefined;
  const [typed, setTyped] = useState(String(props.defaultValue ?? "").length);
  const count = props.value !== undefined ? String(props.value).length : typed;
  const hint = <FieldMessage id={hintId} text={description} className={styles.hint} />;
  return <div className={styles.field}>
    <label htmlFor={controlId} className={hideLabel ? styles.srOnly : undefined}>{label}</label>
    <textarea {...props} id={controlId} ref={ref} className={[styles.control, className].filter(Boolean).join(" ")} style={props.rows ? { ...style, "--_rows": props.rows } as CSSProperties : style} onChange={(event) => { setTyped(event.target.value.length); onChange?.(event); }} aria-invalid={error ? true : props["aria-invalid"]} aria-describedby={joinDescribedBy(props["aria-describedby"], hintId, countId, errorId)}/>
    {max ? <div className={styles.footer}><div className={styles.footerHint}>{hint}</div><span id={countId} className={styles.count} data-state={count >= max * warnAt ? "warn" : undefined}>{t.count(format(count), format(max))}</span></div> : hint}
    <FieldMessage id={errorId} text={error} className={styles.error} alert />
  </div>;
});

Textarea.displayName = "Textarea";
