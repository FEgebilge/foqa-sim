"use client";

import * as TabsPrimitive from "@radix-ui/react-tabs";
import { createContext, useCallback, useContext, useId, useLayoutEffect, useRef, useState } from "react";
import type { ComponentPropsWithoutRef, RefObject } from "react";
import { ChevronLeft as NavArrowLeft, ChevronRight as NavArrowRight } from "lucide-react";
import { AnimatePresence, LayoutGroup, animate, motion, useReducedMotion } from "motion/react";
import type { AnimationPlaybackControls, Variants } from "motion/react";
import { motionPresets } from "@ui/lib/motion-presets";
import { motionTokens } from "@ui/lib/motion-tokens";
import { ArcLink, useLabels } from "@ui/lib/arc-provider";
import styles from "./tabs.module.css";

export interface TabsLabels { scrollLeft: string; scrollRight: string }
const DEFAULT_LABELS: TabsLabels = { scrollLeft: "Scroll tabs left", scrollRight: "Scroll tabs right" };
declare module "@ui/lib/arc-provider" { interface ArcLabels { tabs: TabsLabels } }

/** "underline" (default): page or section tabs, a hairline row with an accent bar under the active tab. "pill" is set by
 *  `PillTabs`: a raised highlight on a tinted track, for compact toggles such as Preview / Code. */
export type TabsVariant = "underline" | "pill";
type RootProps = ComponentPropsWithoutRef<typeof TabsPrimitive.Root> & { variant?: TabsVariant };
/** `direction` is +1 when the new tab sits after the old one; `panelHeightRef` holds the visible panel height so the next panel can morph from it; `leavingRectRef` pins the outgoing panel where it was on screen. */
const TabsContext = createContext<{ active: string; layoutId: string; direction: number; variant: TabsVariant; panelHeightRef: RefObject<number | null>; leavingRectRef: RefObject<DOMRect | null> }>({ active: "", layoutId: "tabs", direction: 1, variant: "underline", panelHeightRef: { current: null }, leavingRectRef: { current: null } });

export function Tabs({ value, defaultValue, onValueChange, variant = "underline", className, ...props }: RootProps) {
  const [internal, setInternal] = useState(defaultValue ?? "");
  const [direction, setDirection] = useState(1);
  const active = value ?? internal;
  const layoutId = useId();
  const root = useRef<HTMLDivElement>(null);
  const panelHeightRef = useRef<number | null>(null);
  const leavingRectRef = useRef<DOMRect | null>(null);
  function handleChange(next: string) {
    const frame = root.current;
    leavingRectRef.current = frame?.querySelector(':scope > [role="tabpanel"][data-state="active"]')?.getBoundingClientRect() ?? null;
    const order = frame ? Array.from(frame.querySelectorAll<HTMLElement>('[role="tab"][data-value]')).filter(tab => tab.closest(`.${styles.root}`) === frame).map(tab => tab.dataset.value) : [];
    const from = order.indexOf(active), to = order.indexOf(next);
    if (from >= 0 && to >= 0 && from !== to) setDirection(to > from ? 1 : -1);
    if (value === undefined) setInternal(next); onValueChange?.(next);
  }
  return <TabsContext.Provider value={{ active, layoutId, direction, variant, panelHeightRef, leavingRectRef }}><LayoutGroup id={layoutId}><TabsPrimitive.Root {...props} ref={root} className={[styles.root, className].filter(Boolean).join(" ")} value={active} onValueChange={handleChange}/></LayoutGroup></TabsContext.Provider>;
}

export function TabsList({ className, labels, ...props }: ComponentPropsWithoutRef<typeof TabsPrimitive.List> & { labels?: Partial<TabsLabels> }) {
  const t = useLabels("tabs", DEFAULT_LABELS, labels);
  const { active, variant } = useContext(TabsContext);
  const reduced = useReducedMotion();
  const shell = useRef<HTMLDivElement>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const [edges, setEdges] = useState({ overflow: false, left: false, right: false });
  const update = useCallback(() => {
    const frame = shell.current;
    const scroll = viewport.current;
    if (!frame || !scroll) return;
    const max = Math.max(0, scroll.scrollWidth - scroll.clientWidth);
    const next = { overflow: scroll.scrollWidth > frame.clientWidth + 1, left: scroll.scrollLeft > 1, right: scroll.scrollLeft < max - 1 };
    setEdges(previous => previous.overflow === next.overflow && previous.left === next.left && previous.right === next.right ? previous : next);
  }, []);
  const reveal = useCallback((tab: HTMLElement | null) => {
    const scroll = viewport.current;
    if (!scroll || !tab) return;
    const frame = scroll.getBoundingClientRect();
    const item = tab.getBoundingClientRect();
    const max = Math.max(0, scroll.scrollWidth - scroll.clientWidth);
    const left = frame.left + (scroll.scrollLeft > 1 ? 34 : 0);
    const right = frame.right - (scroll.scrollLeft < max - 1 ? 34 : 0);
    const delta = item.left < left ? item.left - left : item.right > right ? item.right - right : 0;
    if (delta) scroll.scrollBy({ left: delta, behavior: reduced ? "instant" : "smooth" });
  }, [reduced]);
  useLayoutEffect(() => {
    const frame = shell.current;
    const scroll = viewport.current;
    const content = list.current;
    if (!frame || !scroll || !content) return;
    const observer = new ResizeObserver(update);
    observer.observe(frame);
    observer.observe(scroll);
    observer.observe(content);
    scroll.addEventListener("scroll", update, { passive: true });
    update();
    return () => { observer.disconnect(); scroll.removeEventListener("scroll", update); };
  }, [update]);
  useLayoutEffect(() => { reveal(list.current?.querySelector<HTMLElement>('[role="tab"][data-state="active"]') ?? null); }, [active, reveal]);
  const scrollTabs = (direction: number) => viewport.current?.scrollBy({ left: direction * (viewport.current?.clientWidth ?? 0) * .75, behavior: reduced ? "instant" : "smooth" });
  return <div ref={shell} className={styles.listShell} data-variant={variant} data-overflow={edges.overflow} data-left={edges.left} data-right={edges.right}>
    {edges.overflow && <button type="button" className={`${styles.scrollButton} ${styles.scrollLeft}`} aria-label={t.scrollLeft} disabled={!edges.left} onClick={() => scrollTabs(-1)}><NavArrowLeft width={17} height={17} aria-hidden="true"/></button>}
    <motion.div ref={viewport} layoutScroll className={styles.viewport} onFocusCapture={event => { if (event.target instanceof HTMLElement && event.target.getAttribute("role") === "tab") reveal(event.target); }}>
      <TabsPrimitive.List {...props} ref={list} className={[styles.list, className].filter(Boolean).join(" ")}/>
    </motion.div>
    {edges.overflow && <button type="button" className={`${styles.scrollButton} ${styles.scrollRight}`} aria-label={t.scrollRight} disabled={!edges.right} onClick={() => scrollTabs(1)}><NavArrowRight width={17} height={17} aria-hidden="true"/></button>}
  </div>;
}

/** `href` makes the trigger a link (route tabs): it renders an anchor that keeps the tab role and the selection highlight. */
export function TabsTrigger({ className, children, value, href, ...props }: ComponentPropsWithoutRef<typeof TabsPrimitive.Trigger> & { href?: string }) {
  const { active } = useContext(TabsContext);
  const reduced = useReducedMotion();
  // The LayoutGroup in Tabs scopes the highlight to this instance, so it glides between triggers but never flies in from another tab set.
  const content = <>
    {active === value && <motion.span className={styles.selection} layoutId="selection" layoutDependency={active} transition={reduced ? { duration: 0 } : motionTokens.spring.morph} aria-hidden="true"/>}
    <span className={styles.triggerLabel}>{children}</span>
  </>;
  return <TabsPrimitive.Trigger {...props} value={value} data-value={value} asChild={!!href} className={[styles.trigger, className].filter(Boolean).join(" ")}>
    {href ? <ArcLink href={href}>{content}</ArcLink> : content}
  </TabsPrimitive.Trigger>;
}

const panelMotion: Variants = {
  enter: (direction: number) => ({ opacity: 0, x: direction * 8 }),
  center: { opacity: 1, x: 0, transition: { opacity: { duration: motionTokens.duration.standard, ease: [...motionTokens.ease.enter] }, x: motionTokens.spring.smooth } },
  exit: (direction: number) => ({ opacity: 0, x: direction * -6, transition: { duration: motionTokens.duration.instant, ease: [...motionTokens.ease.standard] } }),
};
/** Reduced motion: a short crossfade in place. Keys match panelMotion so server and client render identical styles. */
const panelFade: Variants = {
  enter: { opacity: 0, x: 0 },
  center: { opacity: 1, x: 0, transition: { duration: motionTokens.duration.instant } },
  exit: { opacity: 0, x: 0, transition: { duration: .1 } },
};

export function TabsContent({ className, value, forceMount, children, ...props }: ComponentPropsWithoutRef<typeof TabsPrimitive.Content>) {
  const { active, direction, panelHeightRef, leavingRectRef } = useContext(TabsContext);
  const reduced = useReducedMotion();
  const panel = useRef<HTMLDivElement>(null);
  const selected = active === value;
  const classes = [styles.content, className].filter(Boolean).join(" ");
  // The incoming panel starts at the outgoing panel's height and settles at its own, so content below glides instead of jumping.
  useLayoutEffect(() => {
    const node = panel.current;
    if (!node || forceMount) return;
    // Outgoing: the tab root may have changed size around it, so pin the panel to where it was while it fades.
    if (!selected) { const before = leavingRectRef.current; const now = node.getBoundingClientRect(); if (before) node.style.translate = `${before.left - now.left}px ${before.top - now.top}px`; node.inert = true; return; }
    node.style.translate = ""; node.inert = false;
    const from = panelHeightRef.current;
    const to = node.offsetHeight;
    let controls: AnimationPlaybackControls | undefined;
    const release = () => { node.style.height = ""; node.style.overflow = ""; };
    if (from !== null && Math.abs(from - to) > 1 && !reduced) {
      if (to > from) node.style.overflow = "clip";
      // A plain value animation writing style.height: animating the height through Motion would leave it in the panel's
      // motion state, which React renders back later and pins a panel whose content has grown since.
      controls = animate(from, to, { ...motionTokens.spring.smooth, onUpdate: height => { node.style.height = `${height}px`; }, onComplete: release });
    }
    panelHeightRef.current = controls && from !== null ? from : to;
    // The panel itself resizes on every frame of the morph, so only its children tell real growth apart. Content that
    // changes size mid-morph (an error row, an opened note) would be clipped at the pinned height, so the morph yields to it.
    const known = new WeakSet<Element>();
    const observer = new ResizeObserver(entries => {
      for (const { target } of entries) {
        if (target === node) panelHeightRef.current = node.offsetHeight;
        else if (known.has(target)) { controls?.stop(); release(); } else known.add(target);
      }
    });
    observer.observe(node);
    for (const child of node.children) observer.observe(child);
    return () => { observer.disconnect(); controls?.stop(); release(); };
  }, [selected, reduced, forceMount, panelHeightRef, leavingRectRef]);
  // Mounted panels keep their state (drafts) across tab switches. Radix leaves them focusable, so inactive ones are hidden; the
  // newly active one rises in place, with no height morph because the panel never unmounts.
  if (forceMount) return <TabsPrimitive.Content {...props} value={value} forceMount asChild hidden={!selected}>
    <motion.div className={classes} variants={motionPresets.rise(reduced)} initial={false} animate={selected ? "shown" : "hidden"}>{children}</motion.div>
  </TabsPrimitive.Content>;
  return <AnimatePresence initial={false} mode="popLayout" custom={direction}>
    {selected && <TabsPrimitive.Content {...props} key={value} value={value} forceMount asChild>
      <motion.div ref={panel} className={classes} custom={direction} variants={reduced ? panelFade : panelMotion} initial="enter" animate="center" exit="exit">{children}</motion.div>
    </TabsPrimitive.Content>}
  </AnimatePresence>;
}
