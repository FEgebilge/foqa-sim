"use client";

import { forwardRef, useId } from "react";
import type { InputHTMLAttributes } from "react";
import { FieldMessage, joinDescribedBy } from "@ui/lib/field-message";
import styles from "./input.module.css";

export interface InputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "size"> {
  label: string;
  description?: string;
  error?: string;
  /** Keeps the label for assistive tech and drops it visually (dense forms). */
  hideLabel?: boolean;
  size?: "sm" | "md";
}

export const Input = forwardRef<HTMLInputElement, InputProps>(function Input(
  { label, description, error, hideLabel, size = "md", id, className, ...props }, ref,
) {
  const generatedId = useId();
  const controlId = id ?? generatedId;
  const hintId = description ? `${controlId}-description` : undefined;
  const errorId = error ? `${controlId}-error` : undefined;
  const describedBy = joinDescribedBy(props["aria-describedby"], hintId, errorId);
  return <div className={styles.field}>
    <label className={[styles.label, hideLabel && styles.srOnly].filter(Boolean).join(" ")} htmlFor={controlId}>{label}</label>
    <input {...props} id={controlId} ref={ref} className={[styles.input, className].filter(Boolean).join(" ")} data-size={size} aria-invalid={error ? true : props["aria-invalid"]} aria-describedby={describedBy}/>
    <FieldMessage id={hintId} text={description} className={styles.description} />
    <FieldMessage id={errorId} text={error} className={styles.error} alert />
  </div>;
});

Input.displayName = "Input";
