"use client";

import * as DialogPrimitive from "@radix-ui/react-dialog";
import { createContext, useCallback, useContext, useLayoutEffect, useRef, useState } from "react";
import type { ComponentPropsWithoutRef, ReactNode } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import type { Transition } from "motion/react";
import { X } from "lucide-react";
import { useLabels } from "@ui/lib/arc-provider";
import { motionTokens } from "@ui/lib/motion-tokens";
import styles from "./dialog.module.css";

export interface DialogLabels { close: string }
const DEFAULT_LABELS: DialogLabels = { close: "Close dialog" };
declare module "@ui/lib/arc-provider" { interface ArcLabels { dialog: DialogLabels } }

/** Mirrors the open state so the content can stay mounted while it animates out, and retarget mid-flight if it is reopened or closed early. */
const OpenContext = createContext<boolean | null>(null);

export function Dialog({ open: openProp, defaultOpen = false, onOpenChange, ...props }: ComponentPropsWithoutRef<typeof DialogPrimitive.Root>) {
  const [uncontrolled, setUncontrolled] = useState(defaultOpen);
  const open = openProp ?? uncontrolled;
  const setOpen = useCallback((next: boolean) => { if (openProp === undefined) setUncontrolled(next); onOpenChange?.(next); }, [openProp, onOpenChange]);
  return <OpenContext.Provider value={open}><DialogPrimitive.Root {...props} open={open} onOpenChange={setOpen}/></OpenContext.Provider>;
}

export const DialogTrigger = DialogPrimitive.Trigger;
export const DialogClose = DialogPrimitive.Close;

export interface DialogContentProps extends ComponentPropsWithoutRef<typeof DialogPrimitive.Content> {
  title: string;
  description?: string;
  /** The scrolling body. Leave it out for a question that only needs a footer. */
  children?: ReactNode;
  /** Width: 440 / 560 / 720px (never wider than the screen). Unset: 440px, or whatever --dialog-width the className sets. */
  size?: "sm" | "md" | "lg";
  /** A row under the scrolling body that stays in view with the header: the dialog's actions. */
  footer?: ReactNode;
  /** Accessible name of the Close button. Wins over `labels.close`. */
  closeLabel?: string;
  /** Drops the Close button, for a dialog that must be answered or that is busy. Escape and outside presses still dismiss it unless the caller prevents them. */
  hideClose?: boolean;
  labels?: Partial<DialogLabels>;
}

const fade: Transition = { duration: motionTokens.duration.instant };
const leave: Transition = { duration: motionTokens.duration.fast, ease: [...motionTokens.ease.standard] };

/** When the title or description changes while open, the new copy rises in and the old copy leaves upward. */
function SwapText({ text }: { text: string }) {
  const reduced = useReducedMotion();
  return <AnimatePresence mode="popLayout" initial={false}>
    <motion.span key={text} className={styles.swap} initial={reduced ? false : { opacity: 0, y: "0.3em", filter: `blur(${motionTokens.blur.soft}px)` }} animate={{ opacity: 1, y: 0, filter: "blur(0px)" }} exit={reduced ? { opacity: 0, transition: { duration: 0 } } : { opacity: 0, y: "-0.3em", filter: `blur(${motionTokens.blur.subtle}px)`, transition: { duration: motionTokens.duration.fast, ease: [...motionTokens.ease.standard] } }} transition={{ duration: motionTokens.duration.standard, ease: [...motionTokens.ease.enter] }}>{text}</motion.span>
  </AnimatePresence>;
}

/** Where focus was when a dialog opened. From a menu item that is the item, which is gone by close, so it resolves to the menu's trigger. */
function opener(): HTMLElement | null {
  const active = document.activeElement;
  if (!(active instanceof HTMLElement) || active === document.body) return null;
  const menu = active.closest('[role="menu"]')?.getAttribute("aria-labelledby");
  return (menu ? document.getElementById(menu) : null) ?? active;
}

export function DialogContent({ title, description, children, className, size, footer, closeLabel, hideClose, labels, onPointerDownOutside, onOpenAutoFocus, onCloseAutoFocus, ...props }: DialogContentProps) {
  const t = useLabels("dialog", DEFAULT_LABELS, labels);
  const open = useContext(OpenContext);
  const reduced = useReducedMotion();
  // When the open state last changed. Radix waits for the click before treating a press as outside, and a press on the trigger
  // while the dialog leaves reopens it first, so that press must not close it again.
  const change = useRef({ open, at: 0 });
  useLayoutEffect(() => { change.current = { open, at: performance.now() }; }, [open]);
  const pressOutside: DialogContentProps["onPointerDownOutside"] = event => {
    onPointerDownOutside?.(event);
    if (open !== null && (!change.current.open || event.detail.originalEvent.timeStamp < change.current.at)) event.preventDefault();
  };
  // Radix only refocuses a DialogTrigger, so a dialog opened from code (a menu item, a shortcut) would drop focus to the page.
  // A caller that handled it (preventDefault) keeps priority, and a trigger that is not the opener is left to Radix when focus was nowhere.
  const from = useRef<HTMLElement | null>(null);
  const shared: Pick<DialogContentProps, "onOpenAutoFocus" | "onCloseAutoFocus" | "onPointerDownOutside"> = {
    onOpenAutoFocus: event => { from.current = opener(); onOpenAutoFocus?.(event); },
    onCloseAutoFocus: event => {
      onCloseAutoFocus?.(event);
      if (event.defaultPrevented || !from.current?.isConnected) return;
      event.preventDefault();
      from.current.focus();
    },
    onPointerDownOutside: pressOutside,
  };
  const classes = [styles.content, className].filter(Boolean).join(" ");
  const inner = <>
    <div className={styles.header}><div><DialogPrimitive.Title className={styles.title}><SwapText text={title}/></DialogPrimitive.Title>{description ? <DialogPrimitive.Description className={styles.description}><SwapText text={description}/></DialogPrimitive.Description> : null}</div>{hideClose ? null : <DialogPrimitive.Close className={styles.close} aria-label={closeLabel ?? t.close}><X size={16} strokeWidth={1.75} aria-hidden="true"/></DialogPrimitive.Close>}</div>
    <div className={styles.body}>{children}</div>
    {footer ? <div className={styles.footer}>{footer}</div> : null}
  </>;
  // Under a bare Radix root the open state is unknown here, so CSS keyframes keyed off data-state animate the layers instead.
  if (open === null) return <DialogPrimitive.Portal>
    <DialogPrimitive.Overlay className={`${styles.overlay} ${styles.keyframes}`}/>
    <DialogPrimitive.Content {...props} {...shared} data-size={size} className={`${classes} ${styles.keyframes}`}>{inner}</DialogPrimitive.Content>
  </DialogPrimitive.Portal>;
  // The overlay fades while the dialog rises 8px and scales up on a spring. Closing is shorter and quieter, and starts from wherever the entrance is.
  return <AnimatePresence>
    {open && <DialogPrimitive.Portal key="dialog" forceMount>
      <DialogPrimitive.Overlay asChild forceMount><motion.div className={styles.overlay} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0, transition: reduced ? fade : leave }} transition={reduced ? fade : { duration: motionTokens.duration.standard, ease: [...motionTokens.ease.enter] }}/></DialogPrimitive.Overlay>
      <DialogPrimitive.Content {...props} {...shared} data-size={size} asChild forceMount>
        <motion.div className={classes} initial={reduced ? { opacity: 0 } : { opacity: 0, y: 8, scale: .96 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={reduced ? { opacity: 0, transition: fade } : { opacity: 0, y: 4, scale: .98, transition: leave }} transition={reduced ? fade : { default: motionTokens.spring.smooth, opacity: { duration: motionTokens.duration.fast, ease: [...motionTokens.ease.enter] } }}>{inner}</motion.div>
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>}
  </AnimatePresence>;
}
