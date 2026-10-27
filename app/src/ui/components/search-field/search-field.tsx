"use client";
import { forwardRef, useId, useRef, useState } from "react";
import type { InputHTMLAttributes } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { Search, X as Xmark } from "lucide-react";
import { motionTokens } from "@ui/lib/motion-tokens";
import { useLabels } from "@ui/lib/arc-provider";
import styles from "./search-field.module.css";
export interface SearchFieldLabels { clear: string }
const DEFAULT_LABELS: SearchFieldLabels = { clear: "Clear search" };
declare module "@ui/lib/arc-provider" { interface ArcLabels { "search-field": SearchFieldLabels } }
export interface SearchFieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "size" | "value" | "defaultValue"> {
  label: string;
  /** Controlled value. Leave it out, with `defaultValue`, for an uncontrolled field. */
  value?: string;
  defaultValue?: string;
  onValueChange?: (value: string) => void;
  /** Keeps the label for assistive tech and hides it visually. */
  hideLabel?: boolean;
  size?: "sm" | "md";
  labels?: Partial<SearchFieldLabels>;
}
export const SearchField = forwardRef<HTMLInputElement, SearchFieldProps>(function SearchField({ label, value: controlled, defaultValue = "", onValueChange, hideLabel, size = "md", id, className, labels, ...props }, ref) {
  const t = useLabels("search-field", DEFAULT_LABELS, labels);
  const generated = useId(); const controlId = id ?? generated; const reduced = useReducedMotion(); const inputRef = useRef<HTMLInputElement | null>(null);
  const [internal, setInternal] = useState(defaultValue);
  const value = controlled ?? internal;
  const change = (next: string) => { if (controlled === undefined) setInternal(next); onValueChange?.(next); };
  const setRefs = (node: HTMLInputElement | null) => { inputRef.current = node; if (typeof ref === "function") ref(node); else if (ref) ref.current = node; };
  // Clearing returns focus to the field, since the clear button unmounts under the pointer.
  function clear() { change(""); inputRef.current?.focus(); }
  return <div className={styles.field}><label htmlFor={controlId} className={hideLabel ? styles.srOnly : undefined}>{label}</label><div className={styles.shell} data-size={size} data-filled={value ? "true" : undefined}>
    <Search width={18} height={18} aria-hidden="true"/><input {...props} ref={setRefs} id={controlId} type="search" value={value} onChange={event => change(event.target.value)} className={[styles.input, className].filter(Boolean).join(" ")} />
    {/* The clear button has a reserved slot, so the field never changes width when it appears or leaves. */}
    <span className={styles.clearSlot}><AnimatePresence initial={false}>{value ? <motion.button key="clear" type="button" tabIndex={0} onClick={clear} aria-label={t.clear} initial={reduced ? { opacity: 0 } : { opacity: 0, scale: .8, filter: `blur(${motionTokens.blur.subtle}px)` }} animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }} exit={reduced ? { opacity: 0, transition: { duration: 0 } } : { opacity: 0, scale: .8, filter: `blur(${motionTokens.blur.subtle}px)`, transition: { duration: motionTokens.duration.instant, ease: [...motionTokens.ease.standard] } }} whileTap={reduced ? undefined : { scale: .96, transition: { duration: motionTokens.duration.instant, ease: [...motionTokens.ease.standard] } }} transition={reduced ? { duration: motionTokens.duration.instant } : { ...motionTokens.spring.snappy, opacity: { duration: motionTokens.duration.fast }, filter: { duration: motionTokens.duration.fast } }}><Xmark width={16} height={16} aria-hidden="true" /></motion.button> : null}</AnimatePresence></span>
  </div></div>;
});
