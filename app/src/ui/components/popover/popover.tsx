"use client";

import * as PopoverPrimitive from "@radix-ui/react-popover";
import { forwardRef } from "react";
import type { ComponentPropsWithoutRef } from "react";
import styles from "./popover.module.css";

export const Popover = PopoverPrimitive.Root;
export const PopoverClose = PopoverPrimitive.Close;

/** The trigger anchors the panel, so it opts out of press-scale: a scaled rect measured on open would shift the panel as the trigger springs back. */
export const PopoverTrigger = forwardRef<
  HTMLButtonElement,
  ComponentPropsWithoutRef<typeof PopoverPrimitive.Trigger>
>(function PopoverTrigger({ className, ...props }, ref) {
  return <PopoverPrimitive.Trigger {...props} ref={ref} className={[styles.anchor, className].filter(Boolean).join(" ")}/>;
});

PopoverTrigger.displayName = "PopoverTrigger";

export interface PopoverContentProps extends ComponentPropsWithoutRef<typeof PopoverPrimitive.Content> {
  /** Portals the panel into this element instead of the page body. Inside a modal dialog pass an element within it, so the panel sits inside the dialog's focus trap and scroll lock. */
  container?: HTMLElement | null;
}

export const PopoverContent = forwardRef<HTMLDivElement, PopoverContentProps>(function PopoverContent({ className, align = "start", sideOffset = 6, collisionPadding = 10, container, ...props }, ref) {
  return <PopoverPrimitive.Portal container={container}>
    <PopoverPrimitive.Content
      {...props}
      ref={ref}
      align={align}
      sideOffset={sideOffset}
      collisionPadding={collisionPadding}
      className={[styles.content, className].filter(Boolean).join(" ")}
    />
  </PopoverPrimitive.Portal>;
});

PopoverContent.displayName = "PopoverContent";
