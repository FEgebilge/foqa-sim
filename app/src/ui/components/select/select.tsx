"use client";

import { forwardRef, useId, useState } from "react";
import type { ComponentPropsWithoutRef, ReactNode } from "react";
import * as SelectPrimitive from "@radix-ui/react-select";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import type { Variants } from "motion/react";
import { Check, ChevronDown, ChevronUp } from "lucide-react";
import { useLabels } from "@ui/lib/arc-provider";
import { FieldMessage, joinDescribedBy } from "@ui/lib/field-message";
import { motionTokens } from "@ui/lib/motion-tokens";
import styles from "./select.module.css";

export interface SelectLabels { placeholder: string }

const DEFAULT_LABELS: SelectLabels = { placeholder: "Select an option" };

declare module "@ui/lib/arc-provider" { interface ArcLabels { select: SelectLabels } }

export interface SelectProps extends Omit<ComponentPropsWithoutRef<typeof SelectPrimitive.Root>, "children"> {
  label: string;
  description?: string;
  error?: string;
  /** Keeps the label for assistive tech and drops it visually (dense forms). */
  hideLabel?: boolean;
  /** `top` stacks the label above (default), `inline` puts it beside the control, `hidden` keeps it for assistive tech only (same as `hideLabel`). */
  labelPosition?: "top" | "inline" | "hidden";
  size?: "sm" | "md";
  placeholder?: string;
  labels?: Partial<SelectLabels>;
  id?: string;
  className?: string;
  "aria-describedby"?: string;
  /** `icon` (a tone dot, say) shows in the item and the trigger; it is decorative, the label carries the meaning. */
  options: { value: string; label: string; disabled?: boolean; icon?: ReactNode }[];
}

/** The shown value rolls in the direction of the list: a later option rises from below, an earlier one drops from above. */
const valueRoll: Variants = {
  enter: (direction: number) => ({ opacity: 0, y: `${direction * 0.35}em`, filter: `blur(${motionTokens.blur.soft}px)` }),
  center: { opacity: 1, y: 0, filter: "blur(0px)", transition: { duration: motionTokens.duration.standard, ease: [...motionTokens.ease.enter] } },
  exit: (direction: number) => ({ opacity: 0, y: `${direction * -0.3}em`, filter: `blur(${motionTokens.blur.subtle}px)`, transition: { duration: motionTokens.duration.fast, ease: [...motionTokens.ease.standard] } }),
};
/** Reduced motion keeps a short crossfade; the resting state matches valueRoll so server and client markup agree. */
const valueFade: Variants = { enter: { opacity: 0 }, center: { opacity: 1, y: 0, filter: "blur(0px)", transition: { duration: motionTokens.duration.instant } }, exit: { opacity: 0, transition: { duration: motionTokens.duration.instant } } };

export const Select = forwardRef<HTMLButtonElement, SelectProps>(function Select(
  { label, description, error, hideLabel, labelPosition = hideLabel ? "hidden" : "top", size = "md", placeholder: placeholderProp, labels, options, id, className, disabled, onValueChange, "aria-describedby": describedByProp, ...rootProps },
  ref,
) {
  const t = useLabels("select", DEFAULT_LABELS, labels);
  const placeholder = placeholderProp ?? t.placeholder;
  const generatedId = useId();
  const controlId = id ?? generatedId;
  const hintId = description ? `${controlId}-description` : undefined;
  const errorId = error ? `${controlId}-error` : undefined;
  const describedBy = joinDescribedBy(describedByProp, hintId, errorId);
  const reduceMotion = useReducedMotion();
  const [uncontrolledValue, setUncontrolledValue] = useState(rootProps.defaultValue ?? "");
  const currentValue = rootProps.value ?? uncontrolledValue;
  const index = options.findIndex((option) => option.value === currentValue);
  const shown = currentValue ? options[index]?.label ?? "" : placeholder;
  const shownIcon = currentValue ? options[index]?.icon : undefined;
  const [previousIndex, setPreviousIndex] = useState(index);
  const [direction, setDirection] = useState(1);
  if (previousIndex !== index) { setPreviousIndex(index); setDirection(index > previousIndex ? 1 : -1); }

  return (
    <div className={styles.field} data-label={labelPosition}>
      <label htmlFor={controlId} className={labelPosition === "hidden" ? styles.srOnly : undefined}>{label}</label>
      <SelectPrimitive.Root {...rootProps} disabled={disabled} onValueChange={(next) => { setUncontrolledValue(next); onValueChange?.(next); }}>
        <SelectPrimitive.Trigger
          ref={ref}
          id={controlId}
          data-size={size}
          aria-invalid={error ? true : undefined}
          aria-describedby={describedBy}
          className={[styles.trigger, className].filter(Boolean).join(" ")}
        >
          {/* Radix keeps the real value for assistive tech; the visible copy below animates between values. */}
          <span className={styles.srOnly}><SelectPrimitive.Value placeholder={placeholder} /></span>
          <span className={styles.valueText} aria-hidden="true">
            <AnimatePresence initial={false} custom={direction}>
              <motion.span key={currentValue ? `value-${currentValue}` : "placeholder"} data-placeholder={currentValue ? undefined : ""} custom={direction} variants={reduceMotion ? valueFade : valueRoll} initial="enter" animate="center" exit="exit">{shownIcon && <span className={styles.icon} aria-hidden="true">{shownIcon}</span>}<span className={styles.valueLabel}>{shown}</span></motion.span>
            </AnimatePresence>
          </span>
          <SelectPrimitive.Icon className={styles.chevron}>
            <ChevronDown size={16} strokeWidth={1.75} aria-hidden="true" />
          </SelectPrimitive.Icon>
        </SelectPrimitive.Trigger>
        <SelectPrimitive.Portal>
          <SelectPrimitive.Content className={styles.content} position="popper" sideOffset={4} collisionPadding={12}>
            <SelectPrimitive.ScrollUpButton className={styles.scrollButton}>
              <ChevronUp size={16} strokeWidth={1.75} aria-hidden="true" />
            </SelectPrimitive.ScrollUpButton>
            <SelectPrimitive.Viewport className={styles.viewport}>
              {options.map((option) => (
                <SelectPrimitive.Item key={option.value} value={option.value} disabled={option.disabled} className={styles.item}>
                  {option.icon && <span className={styles.icon} aria-hidden="true">{option.icon}</span>}
                  <SelectPrimitive.ItemText>{option.label}</SelectPrimitive.ItemText>
                  <SelectPrimitive.ItemIndicator className={styles.indicator}>
                    <Check size={16} strokeWidth={1.75} aria-hidden="true" />
                  </SelectPrimitive.ItemIndicator>
                </SelectPrimitive.Item>
              ))}
            </SelectPrimitive.Viewport>
            <SelectPrimitive.ScrollDownButton className={styles.scrollButton}>
              <ChevronDown size={16} strokeWidth={1.75} aria-hidden="true" />
            </SelectPrimitive.ScrollDownButton>
          </SelectPrimitive.Content>
        </SelectPrimitive.Portal>
      </SelectPrimitive.Root>
      <FieldMessage id={hintId} text={description} className={styles.hint} />
      <FieldMessage id={errorId} text={error} className={styles.error} alert />
    </div>
  );
});
