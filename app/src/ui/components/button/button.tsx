"use client";

import { forwardRef, isValidElement, useCallback, useEffect, useLayoutEffect, useRef } from "react";
import type { AnchorHTMLAttributes, ButtonHTMLAttributes, ElementType, MouseEvent, ReactNode, Ref, RefObject } from "react";
import { AnimatePresence, animate, motion, useIsPresent, useMotionValue, useReducedMotion } from "motion/react";
import type { TargetAndTransition, Variants } from "motion/react";
import { Spinner } from "@ui/components/spinner/spinner";
import { ArcLink } from "@ui/lib/arc-provider";
import { motionTokens } from "@ui/lib/motion-tokens";
import styles from "./button.module.css";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
export type ButtonSize = "xs" | "sm" | "md" | "lg";

/**
 * The primary fill is `--button-primary-bg` with the label in `--button-primary-fg` (ink on the page color by default), so an app sets
 * its brand once on :root. With `href` or `asChild` the node is an anchor, not a button, though the ref is typed as the button's.
 */
export interface ButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "onDrag" | "onDragEnd" | "onDragStart" | "onAnimationStart">, Pick<AnchorHTMLAttributes<HTMLAnchorElement>, "target" | "rel" | "download"> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  /** A square button for a lone icon. Give it a `label`. */
  iconOnly?: boolean;
  /** The accessible name (aria-label). There is no built-in tooltip, which would make every Button depend on Tooltip: wrap an icon-only Button in `Tooltip` where one is wanted. */
  label?: string;
  /** Renders a link through the provider's link component (so router links work) that looks and presses like the button. */
  href?: string;
  /** Renders the single child element (a router NavLink, an `<a>`) as the button, keeping its own props and children. */
  asChild?: boolean;
}

/** Icon buttons press a little deeper, wide buttons a little less, so every size reads as the same push. */
const pressVariants: Variants = {
  pressed: (button: RefObject<HTMLElement | null>) => {
    const width = button.current?.offsetWidth ?? 0;
    return { scale: width > 220 ? .985 : width && width <= 48 ? .96 : .97, transition: { duration: motionTokens.duration.instant, ease: [...motionTokens.ease.standard] } };
  },
};

const rest: TargetAndTransition = { opacity: 1, y: 0, scale: 1, filter: "blur(0px)" };
/** Text rises about .3em out of a soft blur; the outgoing label lifts away a little faster. */
const textIn: TargetAndTransition = { opacity: 0, y: 4, filter: `blur(${motionTokens.blur.soft}px)` };
const textOut: TargetAndTransition = { opacity: 0, y: -3, filter: `blur(${motionTokens.blur.soft}px)`, transition: { duration: motionTokens.duration.fast, ease: [...motionTokens.ease.standard] } };
const iconIn: TargetAndTransition = { opacity: 0, scale: .6, filter: `blur(${motionTokens.blur.subtle}px)` };
const iconOut: TargetAndTransition = { opacity: 0, scale: .6, filter: `blur(${motionTokens.blur.subtle}px)`, transition: { duration: motionTokens.duration.fast, ease: [...motionTokens.ease.standard] } };
const fadeIn: TargetAndTransition = { ...rest, opacity: 0 };
const fadeOut: TargetAndTransition = { opacity: 0, transition: { duration: motionTokens.duration.instant } };
/** Scale rides the spring; opacity and blur tween so blur never overshoots below zero. */
const iconEnter = { ...motionTokens.spring.snappy, opacity: { duration: motionTokens.duration.fast, ease: [...motionTokens.ease.enter] }, filter: { duration: motionTokens.duration.fast, ease: [...motionTokens.ease.enter] } } as const;

/** One motion element per root type, created once: a type made on every render would remount the button each time. */
const roots = new Map<ElementType, typeof motion.button>();
function rootFor(type: ElementType) {
  let root = roots.get(type);
  if (!root) roots.set(type, root = motion.create(type) as typeof motion.button);
  return root;
}

/** A key for the label content: text plus element names, so a new label or icon crossfades while prop-only updates stay in place. */
function labelKey(node: ReactNode): string {
  if (node == null || typeof node === "boolean") return "";
  if (typeof node === "string" || typeof node === "number" || typeof node === "bigint") return String(node);
  if (Array.isArray(node)) return node.map(labelKey).join("");
  if (!isValidElement(node)) return "";
  const type = node.type as string | { displayName?: string; name?: string };
  return `<${typeof type === "string" ? type : type?.displayName ?? type?.name ?? ""}>${labelKey((node.props as { children?: ReactNode }).children)}`;
}

/** Springs the slot to the natural width of the incoming label when it changes, so a new label never snaps the button's size.
 *  The outgoing label is popped out of flow at once, so it never holds the old width. Other resizes (a late web font, a parent reflow) jump straight to the new width, so nothing wobbles on first paint. */
function useMorphWidth(content: RefObject<HTMLElement | null>, key: string, reduced: boolean) {
  const width = useMotionValue<number | "auto">("auto");
  const lastKey = useRef(key), armedUntil = useRef(0);
  useLayoutEffect(() => {
    if (lastKey.current === key) return;
    lastKey.current = key;
    armedUntil.current = performance.now() + 700;
  }, [key]);
  useEffect(() => {
    const node = content.current, slot = node?.parentElement;
    if (!node || !slot || typeof ResizeObserver === "undefined") return;
    let measured = false;
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const next = entry.contentRect.width;
      if (!next || !measured || reduced || performance.now() > armedUntil.current) { measured = next > 0; width.jump(next || "auto"); delete slot.dataset.morphing; return; }
      slot.dataset.morphing = "";
      animate(width, next, { ...motionTokens.spring.morph, onComplete: () => { delete slot.dataset.morphing; } });
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [content, reduced, width]);
  return width;
}

function LabelPhase({ children, icon, reduced, ref }: { children: ReactNode; icon: boolean; reduced: boolean; ref?: Ref<HTMLSpanElement> }) {
  const present = useIsPresent();
  return <motion.span ref={ref} className={styles.labelPhase} aria-hidden={present ? undefined : true} initial={reduced ? fadeIn : icon ? iconIn : textIn} animate={rest} exit={reduced ? fadeOut : icon ? iconOut : textOut} transition={reduced ? { duration: motionTokens.duration.instant } : icon ? iconEnter : { duration: motionTokens.duration.standard, ease: [...motionTokens.ease.enter] }}>{children}</motion.span>;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { className, variant = "primary", size = "md", loading = false, iconOnly = false, label, href, asChild = false, disabled, children, onClick, ...props },
  ref,
) {
  const reduceMotion = useReducedMotion() ?? false;
  const child = asChild && isValidElement<{ children?: ReactNode; className?: string; onClick?: (event: MouseEvent<HTMLElement>) => void; ref?: Ref<HTMLButtonElement> }>(children) ? children : null;
  const content = child ? child.props.children : children;
  const type: ElementType = child ? child.type as ElementType : href !== undefined ? ArcLink : "button";
  const native = type === "button";
  const Root = rootFor(type);
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  const contentRef = useRef<HTMLSpanElement>(null);
  const key = labelKey(content);
  const width = useMorphWidth(contentRef, key, reduceMotion);
  const childRef = child?.props.ref;
  const setRefs = useCallback((node: HTMLButtonElement | null) => {
    buttonRef.current = node;
    for (const target of [ref, childRef]) if (typeof target === "function") target(node); else if (target) target.current = node;
  }, [ref, childRef]);
  // A trigger that anchors a menu, popover, or dialog keeps its rect still while pressed, so the layer never measures a scaled anchor.
  // Radix triggers (asChild) pass aria-haspopup or data-state through; the stylesheet repeats the guard.
  const popup = props["aria-haspopup"];
  const anchorsLayer = (popup !== undefined && popup !== false && popup !== "false") || props.role === "combobox" || (props as Record<string, unknown>)["data-state"] !== undefined;
  const inert = disabled || loading || props["aria-disabled"] === true || props["aria-disabled"] === "true";
  // A link cannot be disabled natively: it stays in the DOM, is marked aria-disabled, and its press is swallowed like a loading one.
  const blocked = loading || (!native && !!disabled);
  const classes = [styles.button, styles[variant], styles[size], iconOnly && styles.iconOnly, className, child?.props.className].filter(Boolean).join(" ");

  return (
    // eslint-disable-next-line react/static-components -- rootFor returns the same component for the same type, so it is stable across renders.
    <Root
      aria-label={label}
      aria-busy={loading || undefined}
      data-disabled={disabled || undefined}
      custom={buttonRef}
      variants={pressVariants}
      whileTap={reduceMotion || anchorsLayer || inert ? undefined : "pressed"}
      transition={motionTokens.spring.snappy}
      {...(native ? { disabled, tabIndex: 0 } : { href: disabled ? undefined : href, tabIndex: disabled ? -1 : undefined })}
      {...props}
      {...child?.props}
      ref={setRefs}
      className={classes}
      // Loading keeps the button focusable (a disabled button would drop keyboard focus mid-action) and swallows presses instead.
      aria-disabled={blocked || props["aria-disabled"] || undefined}
      onClick={(event: MouseEvent<HTMLButtonElement>) => { if (blocked) event.preventDefault(); else { child?.props.onClick?.(event); onClick?.(event); } }}
    >
      <AnimatePresence initial={false}>
        {loading ? <motion.span key="loader" className={styles.loader} aria-hidden="true" initial={reduceMotion ? { opacity: 0 } : { opacity: 0, scale: .6 }} animate={{ opacity: 1, scale: 1 }} exit={reduceMotion ? fadeOut : { ...iconOut, scale: .8 }} transition={reduceMotion ? { duration: motionTokens.duration.instant } : iconEnter}><Spinner size="sm" decorative /></motion.span> : null}
      </AnimatePresence>
      <motion.span className={[styles.labelSlot, loading ? styles.loadingLabel : ""].filter(Boolean).join(" ")} style={{ width }}>
        <span ref={contentRef} className={styles.labelContent}>
          <AnimatePresence mode="popLayout" initial={false}>
            <LabelPhase key={key} icon={!/\S/.test(key.replace(/<[^>]*>/g, ""))} reduced={reduceMotion}>{content}</LabelPhase>
          </AnimatePresence>
        </span>
      </motion.span>
    </Root>
  );
});

Button.displayName = "Button";

export default Button;
