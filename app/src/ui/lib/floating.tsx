"use client";

import { useCallback, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode, RefObject } from "react";
import { createPortal } from "react-dom";
import { AnimatePresence } from "motion/react";

/**
 * One floating layer for every inline popup (calendars, listboxes, time menus). The popup is portaled out of the
 * field, so no `overflow` ancestor (a sidebar, a scrolling sheet body) can clip it, and placed with `position: fixed`
 * against its anchor: flipped above when it fits better there, shifted to stay 8px inside the viewport.
 *
 * Why not Radix Popover: its Content brings its own open state, DismissableLayer and FocusScope, which would fight
 * the controls' existing models (a combobox keeps focus in its input and drives `aria-activedescendant`, the range
 * picker morphs its trigger into the panel). Radix Popper is not a direct dependency. This layer only portals and
 * positions; each control keeps its own open, focus and dismiss logic and asks `contains()` for outside presses.
 */

export type FloatingSide = "top" | "bottom";

export interface FloatingOptions {
  /** "below" hangs the popup under the anchor (flips above); "over" pins its top-left to the anchor's, for surfaces that grow out of the trigger and shift themselves. */
  placement?: "below" | "over";
  /** Expected popup height, used to pick the side in the opening render before the popup can be measured. */
  preferred?: number;
  /** Gap between anchor and popup. */
  gap?: number;
  /** "exact" makes the popup as wide as the anchor, "min" at least as wide. */
  width?: "exact" | "min";
}

export interface Floating {
  open: boolean;
  side: FloatingSide;
  /** True for nodes inside the portaled popup, so outside-press and blur checks can treat it as part of the field. */
  contains: (node: Node | null | undefined) => boolean;
  /** Internal: attaches the positioned wrapper; the node it portals into. */
  attach: (node: HTMLDivElement | null) => void;
  container: HTMLElement | null;
}

const EDGE = 8;
/** Radix dialogs and drawers render their content with a role and a data-state; portaling there keeps their focus trap, scroll lock and pointer events working. */
const DIALOG = "[role=dialog][data-state=open], [role=alertdialog][data-state=open]";

const containerFor = (anchor: HTMLElement | null) => anchor?.closest<HTMLElement>(DIALOG) ?? (typeof document === "undefined" ? null : document.body);

/** A transformed (or filtered, contained) ancestor becomes the containing block of `position: fixed`; its origin is subtracted so viewport coordinates still land right. */
function originOf(container: HTMLElement) {
  for (let node: HTMLElement | null = container; node && node !== document.body && node !== document.documentElement; node = node.parentElement) {
    const style = getComputedStyle(node);
    if (style.transform !== "none" || style.filter !== "none" || style.perspective !== "none" || /paint|layout|strict|content/.test(style.contain) || /transform|filter|perspective/.test(style.willChange)) {
      const rect = node.getBoundingClientRect();
      return { x: rect.left + node.clientLeft - node.scrollLeft, y: rect.top + node.clientTop - node.scrollTop };
    }
  }
  return { x: 0, y: 0 };
}

/** The side for a popup of `height` under `box`: below unless it does not fit there and there is more room above. */
function sideFor(box: DOMRect, height: number, gap: number): { side: FloatingSide; room: number } {
  const viewport = document.documentElement.clientHeight;
  const below = viewport - box.bottom - gap - EDGE;
  const above = box.top - gap - EDGE;
  const side: FloatingSide = height <= below || below >= above ? "bottom" : "top";
  return { side, room: Math.max(0, Math.floor(side === "top" ? above : below)) };
}

export function useFloating(anchor: RefObject<HTMLElement | null>, open: boolean, { placement = "below", preferred = 280, gap = 8, width }: FloatingOptions = {}): Floating {
  const layer = useRef<HTMLDivElement>(null);
  const [side, setSide] = useState<FloatingSide>("bottom");
  const [container, setContainer] = useState<HTMLElement | null>(null);
  const [wasOpen, setWasOpen] = useState(false);
  // Reset-on-change idiom: side and container are settled in the opening render, so the entrance travels the right way.
  if (wasOpen !== open) {
    setWasOpen(open);
    if (open && anchor.current) {
      setContainer(containerFor(anchor.current));
      if (placement === "below") setSide(sideFor(anchor.current.getBoundingClientRect(), preferred, gap).side);
    }
  }

  // Placed before paint on open, then kept in place on scroll (any scroller, via capture), resize and either box resizing.
  useLayoutEffect(() => {
    if (!open || !container) return;
    const place = () => {
      const node = layer.current, target = anchor.current;
      if (!node || !target) return;
      const box = target.getBoundingClientRect();
      const viewportWidth = document.documentElement.clientWidth;
      const origin = node.parentElement === document.body ? { x: 0, y: 0 } : originOf(node.parentElement!);
      node.style.width = width === "exact" ? `${box.width}px` : "";
      node.style.minWidth = width === "min" ? `${box.width}px` : "";
      if (placement === "over") {
        node.style.left = `${box.left - origin.x}px`;
        node.style.top = `${box.top - origin.y}px`;
        return;
      }
      // A popup already squeezed to its room asks for its preferred height again, so it can flip back once there is space.
      const room = parseFloat(node.style.getPropertyValue("--_room")) || 0;
      const natural = node.offsetHeight;
      const next = sideFor(box, room && natural >= room - 1 ? Math.max(natural, preferred) : natural, gap);
      node.style.setProperty("--_room", `${next.room}px`);
      node.dataset.side = next.side;
      setSide(next.side);
      const height = node.offsetHeight, popupWidth = node.offsetWidth;
      const left = Math.max(EDGE, Math.min(box.left, viewportWidth - EDGE - popupWidth));
      const top = next.side === "top" ? box.top - gap - height : box.bottom + gap;
      node.style.left = `${left - origin.x}px`;
      node.style.top = `${top - origin.y}px`;
    };
    place();
    const frame = { id: 0 };
    const update = () => { cancelAnimationFrame(frame.id); frame.id = requestAnimationFrame(place); };
    // Escape belongs to the open popup first: marking it handled (window capture runs before Radix's document listener)
    // keeps a surrounding dialog or drawer open, while the control's own Escape handler still closes the popup.
    const claimEscape = (event: KeyboardEvent) => { if (event.key === "Escape") event.preventDefault(); };
    window.addEventListener("keydown", claimEscape, { capture: true });
    window.addEventListener("scroll", update, { capture: true, passive: true });
    window.addEventListener("resize", update);
    const observer = typeof ResizeObserver === "undefined" ? undefined : new ResizeObserver(update);
    if (anchor.current) observer?.observe(anchor.current);
    if (layer.current) observer?.observe(layer.current);
    return () => {
      cancelAnimationFrame(frame.id);
      observer?.disconnect();
      window.removeEventListener("keydown", claimEscape, { capture: true });
      window.removeEventListener("scroll", update, { capture: true });
      window.removeEventListener("resize", update);
    };
  }, [anchor, container, gap, open, placement, preferred, width]);

  const contains = useCallback((node: Node | null | undefined) => Boolean(node && layer.current?.contains(node)), []);
  const attach = useCallback((node: HTMLDivElement | null) => { layer.current = node; }, []);
  return { open, side, contains, attach, container };
}

/**
 * Renders `children` (usually `{open && <motion.div …/>}`) into the floating layer. The layer stays mounted until the
 * child's exit animation finishes, so the existing entrance and exit presets (and their reduced-motion forms) still play.
 */
export function FloatingLayer({ floating: { open, side, attach, container }, children, className }: { floating: Floating; children: ReactNode; className?: string }) {
  const [present, setPresent] = useState(open);
  if (open && !present) setPresent(true);
  if (!present || !container) return null;
  return createPortal(
    <div ref={attach} className={className} data-floating="" data-side={side} style={{ position: "fixed", zIndex: 70, top: 0, left: 0, pointerEvents: "auto" }}>
      <AnimatePresence onExitComplete={() => { if (!open) setPresent(false); }}>{children}</AnimatePresence>
    </div>,
    container,
  );
}
