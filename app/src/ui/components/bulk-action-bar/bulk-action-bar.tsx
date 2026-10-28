"use client";

import { forwardRef, useEffect, useId, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import type { CSSProperties, KeyboardEvent, ReactElement, ReactNode, Ref, RefObject } from "react";
import { AnimatePresence, motion, useIsPresent, useReducedMotion } from "motion/react";
import { X } from "lucide-react";
import { Button } from "@ui/components/button/button";
import type { ButtonVariant } from "@ui/components/button/button";
import { ConfirmMorph } from "@ui/components/confirm-morph/confirm-morph";
import type { ConfirmMorphState } from "@ui/components/confirm-morph/confirm-morph";
import { DropdownMenu } from "@ui/components/dropdown-menu/dropdown-menu";
import type { DropdownItem } from "@ui/components/dropdown-menu/dropdown-menu";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { useLabels } from "@ui/lib/arc-provider";
import { motionPresets } from "@ui/lib/motion-presets";
import { RollingNumber, SwapText } from "@ui/lib/motion-text";
import { useAnnouncer } from "@ui/lib/use-announcer";
import { useMorphWidth } from "@ui/lib/use-morph-width";
import styles from "./bulk-action-bar.module.css";

export type BulkActionTone = "neutral" | "danger";
export type BulkActionBarPosition = "sticky" | "fixed" | "inline";

export interface BulkActionConfirm {
  /** The question that takes the actions' place, such as "Delete 3 files?". */
  prompt: string;
  /** Defaults to the action's label. */
  confirmLabel?: string;
  /** `press` answers on a press; `hold` asks for a press held until the fill completes. */
  mode?: "press" | "hold";
  /** Beside the spinner while `onSelect` resolves, such as "Deleting". */
  pendingLabel?: string;
  /** Beside the check once `onSelect` resolves, such as "Deleted". */
  doneLabel?: string;
}

export interface BulkAction {
  id: string;
  label: string;
  icon?: ReactNode;
  /** `danger` marks a destructive action; it keeps its icon and label, so the colour is never the only cue. */
  tone?: BulkActionTone;
  /** The filled button, for the one action most selections lead to. */
  primary?: boolean;
  /** true, or a reason such as "Editors only"; the reason shows in a tooltip and is read with the action. */
  disabled?: boolean | string;
  /** Return a promise to show the action as loading; the other actions wait until it settles. */
  onSelect: () => void | Promise<unknown>;
  /** Asks in place before `onSelect` runs: the actions fold away and the question takes their room. */
  confirm?: BulkActionConfirm;
}

/** Every word the bar shows or announces. Words with a count are functions, so an app can apply its own plural rules. */
export interface BulkActionBarLabels {
  /** After the noun, as in "3 files selected". Hidden on narrow bars. */
  selected: string;
  selectAll: (total: number) => string;
  /** Accessible name of the overflow menu. */
  more: string;
  cancel: string;
  /** Spoken shortly after the count settles. */
  announce: (count: number, noun: string) => string;
  /** Spoken when the selection empties. */
  cleared: string;
  /** Spoken when an action's promise rejects. */
  failed: (label: string) => string;
  /** Confirm faces, unless the action names its own. */
  working: string;
  done: string;
  /** Clear, unless `clearLabel` is passed. */
  clear: string;
  /** The toolbar's accessible name, unless `label` is passed. */
  toolbar: string;
  /** The noun, unless `itemName` is passed. */
  items: { one: string; other: string };
}

export interface BulkActionBarProps {
  /** Selected items. The bar docks in above 0 and leaves at 0. */
  count: number;
  /** Items that could be selected, across pages. Enables "Select all {total}". */
  total?: number;
  itemName?: { one: string; other: string };
  actions: BulkAction[];
  /** Actions shown as buttons; the rest fold into an overflow menu. */
  maxVisible?: number;
  /**
   * Called by Clear and by Escape anywhere in the bar. The bar leaves once `count` reaches 0 and takes its focused control
   * with it, so move focus back to where the selection was made (the list, its select-all checkbox) in this callback.
   */
  onClear: () => void;
  /** "Clear selection" by default; "Discard" when the bar holds unsaved changes. */
  clearLabel?: string;
  /** Offered while `count < total` and not `allSelected`. Return a promise (ids fetched from a server) to show it as pending. */
  onSelectAll?: () => void | Promise<unknown>;
  /** Every matching item is selected, including ones not on screen. */
  allSelected?: boolean;
  /** `sticky` docks to the bottom of the nearest scroll container, `fixed` to the viewport, `inline` stays in the flow. */
  position?: BulkActionBarPosition;
  /** Accessible name of the toolbar. */
  label?: string;
  /** A muted second line under the count, such as "Across 2 pages" or the last action's result. */
  status?: ReactNode;
  labels?: Partial<BulkActionBarLabels>;
  className?: string;
  style?: CSSProperties;
}

const DEFAULT_LABELS: BulkActionBarLabels = {
  selected: "selected",
  selectAll: (total) => `Select all ${total}`,
  more: "More actions",
  cancel: "Cancel",
  announce: (count, noun) => `${count} ${noun} selected`,
  cleared: "Selection cleared",
  failed: (label) => `${label} didn’t finish`,
  working: "Working",
  done: "Done",
  clear: "Clear selection",
  toolbar: "Bulk actions",
  items: { one: "item", other: "items" },
};
declare module "@ui/lib/arc-provider" { interface ArcLabels { "bulk-action-bar": BulkActionBarLabels } }

/** Below this container width action labels always collapse to icons with tooltips. */
const NARROW_WIDTH = 480;
/** The count waits this long before it is spoken, so a range selection reads once instead of item by item. */
const ANNOUNCE_DELAY = 150;
/** How long a confirmed action's result stays before the actions return. */
const RESULT_MS = 1800;
const MORE = "\u0000more";

const isPromise = (value: unknown): value is Promise<unknown> => !!value && typeof (value as Promise<unknown>).then === "function";
/** The bar's tab stops: enabled buttons that are not on their way out. */
const controlsIn = (bar: HTMLElement) => Array.from(bar.querySelectorAll<HTMLButtonElement>("button")).filter((node) => !node.disabled && !node.closest("[inert]"));

/** A swapped group of controls. While it leaves it is inert, so focus and the roving tab stop never land on it. */
function Region({ ref, reduce, children }: { ref?: Ref<HTMLDivElement>; reduce: boolean | null; children: ReactNode }) {
  const present = useIsPresent();
  return <motion.div ref={ref} className={styles.region} variants={motionPresets.rise(reduce)} initial="hidden" animate="shown" exit="gone" inert={!present || undefined}>{children}</motion.div>;
}

/** A control that folds in and out of the row: its width springs while it pops, so its neighbours travel instead of jumping. */
function Slot({ reduce, children }: { reduce: boolean | null; children: ReactNode }) {
  const present = useIsPresent();
  return <motion.span className={styles.slot} data-leaving={present ? undefined : ""} inert={!present || undefined} variants={motionPresets.slot(reduce)} initial="hidden" animate="shown" exit="gone">
    <motion.span className={styles.slotInner} variants={motionPresets.pop(reduce)}>{children}</motion.span>
  </motion.span>;
}

/** Icon-only buttons and disabled reasons explain themselves in a tooltip; everything else renders bare. */
function Tip({ content, children }: { content?: string; children: ReactElement }) {
  return content ? <Tooltip content={content}>{children}</Tooltip> : children;
}

interface ActionButtonProps {
  action: BulkAction;
  compact: boolean;
  pending: boolean;
  waiting: boolean;
  onRun: (action: BulkAction) => void;
}

function ActionButton({ action, compact, pending, waiting, onRun }: ActionButtonProps) {
  const reasonId = useId();
  const reason = typeof action.disabled === "string" ? action.disabled : undefined;
  const iconOnly = compact && !!action.icon;
  const variant: ButtonVariant = action.primary ? "primary" : action.tone === "danger" ? "danger" : "secondary";
  return <>
    <Tip content={reason ? (iconOnly ? `${action.label}: ${reason}` : reason) : iconOnly ? action.label : undefined}>
      <Button
        size="sm"
        variant={variant}
        className={[styles.action, iconOnly ? styles.iconOnly : ""].filter(Boolean).join(" ")}
        data-action={action.id}
        data-variant={variant}
        loading={pending}
        disabled={action.disabled === true}
        // A reason or a running sibling keeps the button focusable, so the reason can be read and focus never drops.
        aria-disabled={reason !== undefined || waiting || undefined}
        aria-label={iconOnly ? action.label : undefined}
        aria-describedby={reason ? reasonId : undefined}
        onClick={() => onRun(action)}
      >
        {action.icon && <span className={styles.actionIcon} aria-hidden="true">{action.icon}</span>}
        {!iconOnly && <span>{action.label}</span>}
      </Button>
    </Tip>
    {reason && <span id={reasonId} className={styles.srOnly}>{reason}</span>}
  </>;
}

/**
 * How much the bar has folded to fit its container: 0 shows every label; 1 turns action labels and Clear into icons with
 * tooltips and drops "selected"; 2 moves "Select all" into the overflow menu and drops the status line; 3 leaves the count
 * chip to speak for the noun. Narrow containers start at 1.
 */
type Fold = 0 | 1 | 2 | 3;
const MAX_FOLD = 3;

/** The row's natural width once it settles: its children's layout widths, margins and gaps. Layout widths ignore the scale of
 *  a popping control and the overhang of a leaving label, and slots on their way out are left out, so a swap in flight
 *  (the actions folding away as a question arrives) never reads as overflow. */
function naturalWidth(content: HTMLElement) {
  const children = (Array.from(content.children) as HTMLElement[]).filter((child) => !child.hasAttribute("data-leaving"));
  const gap = parseFloat(getComputedStyle(content).columnGap) || 0;
  return children.reduce((sum, child) => {
    const { marginLeft, marginRight } = getComputedStyle(child);
    return sum + child.offsetWidth + (parseFloat(marginLeft) || 0) + (parseFloat(marginRight) || 0);
  }, gap * Math.max(0, children.length - 1));
}

/** Buttons spring their own width when their label folds, so a level is judged only once that spring has settled. */
const FOLD_SETTLE_MS = 480;

/** Steps the fold to the room the bar has. The natural width that forced each step is remembered, so the bar unfolds only
 *  once that much room returns and never flickers between two levels. The content box resizes whenever its natural width
 *  changes below the clamp or reaches it, and the root whenever the room does, so one observer on each sees every case. */
function useFold(content: RefObject<HTMLDivElement | null>, paused: boolean) {
  const [fold, setFold] = useState<Fold>(0);
  const hold = useRef(paused);
  useLayoutEffect(() => { hold.current = paused; }, [paused]);
  useLayoutEffect(() => {
    const node = content.current;
    const root = node?.closest<HTMLElement>("[data-bulk-action-bar]");
    if (!node || !root || typeof ResizeObserver === "undefined") return;
    let live: Fold = 0;
    let settledAt = 0;
    let timer = 0;
    const needed: number[] = [];
    const fit = () => {
      if (hold.current) return;
      const wait = settledAt - performance.now();
      if (wait > 0) { window.clearTimeout(timer); timer = window.setTimeout(fit, wait); return; }
      const floor = root.clientWidth < NARROW_WIDTH ? 1 : 0;
      const room = parseFloat(getComputedStyle(node).maxWidth);
      const natural = naturalWidth(node);
      let next = Math.max(live, floor) as Fold;
      if (natural > room + .5 && next < MAX_FOLD) { needed[next] = natural; next = (next + 1) as Fold; }
      else if (next > floor && room >= (needed[next - 1] ?? 0)) next = (next - 1) as Fold;
      if (next === live) return;
      live = next;
      settledAt = performance.now() + FOLD_SETTLE_MS;
      // Observer callbacks run between layout and paint; committing now means a row that does not fit is never painted.
      flushSync(() => setFold(next));
    };
    const observer = new ResizeObserver(fit);
    observer.observe(node);
    observer.observe(root);
    return () => { observer.disconnect(); window.clearTimeout(timer); };
  }, [content]);
  return fold;
}

interface BarProps extends Omit<BulkActionBarProps, "position" | "className" | "style" | "labels" | "itemName"> {
  noun: string;
  t: BulkActionBarLabels;
  reduce: boolean | null;
  say: (message: string) => void;
}

/** The docked surface. It mounts with the selection, so its width, pending action and open question start fresh each time. */
function Bar({ t, count, total, noun, actions, maxVisible = 3, onClear, clearLabel = t.clear, onSelectAll, allSelected = false, label = t.toolbar, status, reduce, say }: BarProps) {
  const summaryId = useId();
  // A leaving bar is inert, so focus and presses never land on a selection that is already gone.
  const present = useIsPresent();
  const barRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const confirmRef = useRef<HTMLDivElement>(null);
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [selectingAll, setSelectingAll] = useState(false);
  const [question, setQuestion] = useState<{ id: string; phase: ConfirmMorphState } | null>(null);
  // Where focus goes when the question closes: the action that asked it, or the overflow trigger it came from.
  const returnTo = useRef<string | null>(null);
  const claimFocus = useRef(false);
  const asking = question ? actions.find((action) => action.id === question.id) : undefined;
  // An action that disappears while asking (the host swapped the list) closes its question with it.
  const confirm = asking?.confirm;

  // The fold holds still while a question is open: the question has its own narrow layout (see the stylesheet), and the
  // actions come back exactly as they left.
  const fold = useFold(contentRef, !!confirm);
  const icons = fold >= 1;
  const tucked = fold >= 2;
  // The bar springs to its new width when the count, the noun, the fold or the open question changes; other resizes follow at once.
  const width = useMorphWidth(contentRef, `${count}|${noun}|${fold}|${confirm ? asking!.id : ""}`, !!reduce);

  const canSelectAll = !!onSelectAll && total !== undefined && count < total && !allSelected;
  const inline = actions.slice(0, maxVisible);
  const folded = actions.slice(maxVisible);
  const busy = pendingId !== null;
  async function selectAll() {
    const result = onSelectAll?.();
    if (!isPromise(result)) return;
    setSelectingAll(true);
    try { await result; } catch { say(t.failed(t.selectAll(total!))); } finally { setSelectingAll(false); }
  }
  const menu: DropdownItem[] = [
    ...(tucked && canSelectAll ? [{ label: t.selectAll(total), disabled: selectingAll, onSelect: () => void selectAll() }] : []),
    ...folded.map((action, index) => ({
      label: action.label,
      icon: action.icon,
      destructive: action.tone === "danger",
      disabled: !!action.disabled || busy,
      separatorBefore: index === 0 && tucked && canSelectAll,
      onSelect: () => run(action, MORE),
    })),
  ];

  async function run(action: BulkAction, from: string) {
    if (busy || action.disabled) return;
    if (action.confirm) {
      const active = document.activeElement;
      // A choice from the overflow menu, or a press while focus is in the bar, hands focus to the question.
      claimFocus.current = from === MORE || !active || active === document.body || !!barRef.current?.contains(active);
      returnTo.current = from;
      setQuestion({ id: action.id, phase: "confirming" });
      return;
    }
    let result: void | Promise<unknown>;
    try { result = action.onSelect(); } catch { say(t.failed(action.label)); return; }
    if (!isPromise(result)) return;
    setPendingId(action.id);
    try { await result; } catch { say(t.failed(action.label)); }
    finally { setPendingId((current) => current === action.id ? null : current); }
  }

  // The question mounts already asking, so the bar moves focus onto Confirm, as confirm-morph does between its own faces. A frame
  // later, once the leaving actions are inert, so a closing overflow menu cannot hand focus back to its trigger.
  useEffect(() => {
    if (!question?.id || !claimFocus.current) return;
    claimFocus.current = false;
    const frame = requestAnimationFrame(() => confirmRef.current?.querySelector<HTMLElement>("[data-autofocus]")?.focus({ preventScroll: true }));
    return () => cancelAnimationFrame(frame);
  }, [question?.id]);

  function closeQuestion() {
    const active = document.activeElement;
    const hadFocus = !active || active === document.body || !!barRef.current?.contains(active);
    setQuestion(null);
    if (!hadFocus) returnTo.current = null;
  }

  // Once the actions are back, focus returns to the one that asked (or the overflow trigger), never stranded in the leaving question.
  useLayoutEffect(() => {
    if (question || returnTo.current === null) return;
    const bar = barRef.current;
    const target = returnTo.current === MORE
      ? bar?.querySelector<HTMLElement>('[aria-haspopup="menu"]')
      : bar?.querySelector<HTMLElement>(`[data-action="${CSS.escape(returnTo.current)}"]`);
    returnTo.current = null;
    target?.focus({ preventScroll: true });
  }, [question]);

  // A toolbar is one tab stop: arrows move between its controls, and Tab returns to the one used last. The tab stop is kept in the
  // DOM because the controls belong to composed components (button, menu trigger, confirm faces) that come and go on their own.
  const lastFocused = useRef<HTMLElement | null>(null);
  useLayoutEffect(() => {
    const bar = barRef.current;
    if (!bar) return;
    const sync = () => {
      const list = controlsIn(bar);
      const current = lastFocused.current && list.includes(lastFocused.current as HTMLButtonElement) ? lastFocused.current : list[0];
      for (const node of list) node.tabIndex = node === current ? 0 : -1;
    };
    const onFocusIn = (event: FocusEvent) => {
      if (event.target instanceof HTMLButtonElement) { lastFocused.current = event.target; sync(); }
    };
    // A focused control that folds away ("Select all" once everything is selected, an action swapped for a question) drops
    // focus to the page; it moves to the control that now stands in its place instead.
    let frame = 0;
    const onFocusOut = (event: FocusEvent) => {
      const lost = event.target as HTMLElement;
      if (event.relatedTarget) return;
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if ((document.activeElement && document.activeElement !== document.body) || (lost.isConnected && !lost.closest("[inert]"))) return;
        const list = controlsIn(bar);
        (list.find((node) => lost.compareDocumentPosition(node) & Node.DOCUMENT_POSITION_FOLLOWING) ?? list.at(-1))?.focus({ preventScroll: true });
      });
    };
    sync();
    const observer = new MutationObserver(sync);
    observer.observe(bar, { childList: true, subtree: true, attributes: true, attributeFilter: ["disabled", "inert"] });
    bar.addEventListener("focusin", onFocusIn);
    bar.addEventListener("focusout", onFocusOut);
    return () => { observer.disconnect(); cancelAnimationFrame(frame); bar.removeEventListener("focusin", onFocusIn); bar.removeEventListener("focusout", onFocusOut); };
  }, []);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const bar = barRef.current;
    // Menus and tooltips render in portals; their keys bubble here through React but belong to them.
    if (!bar || event.defaultPrevented || !bar.contains(event.target as Node)) return;
    // Escape clears, except while a confirmed action is running: that answer is already given.
    if (event.key === "Escape") { if (question?.phase !== "pending") { event.preventDefault(); onClear(); } return; }
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    const list = controlsIn(bar);
    if (!list.length) return;
    const at = list.indexOf(event.target as HTMLButtonElement);
    const last = list.length - 1;
    const next = event.key === "Home" ? 0 : event.key === "End" ? last
      : event.key === "ArrowRight" ? (at >= last ? 0 : at + 1) : (at <= 0 ? last : at - 1);
    event.preventDefault();
    list[next]!.tabIndex = 0;
    list[next]!.focus({ preventScroll: true });
  }

  const tone = asking?.tone === "danger" ? "danger" : "neutral";

  return (
    <motion.div
      ref={barRef}
      role="toolbar"
      aria-label={label}
      aria-describedby={summaryId}
      aria-orientation="horizontal"
      className={styles.bar}
      data-confirming={confirm ? tone : undefined}
      data-fold={fold}
      inert={!present || undefined}
      variants={motionPresets.dock(reduce)}
      initial="hidden"
      animate="shown"
      exit="gone"
      onKeyDown={onKeyDown}
    >
      <motion.div className={styles.frame} style={{ width }}>
        <div ref={contentRef} className={styles.content}>
          <div className={styles.summary}>
            <span className={styles.count} aria-hidden="true"><RollingNumber value={count} reduce={reduce} /></span>
            <span className={styles.text}>
              <span className={styles.noun} aria-hidden="true"><SwapText reduce={reduce}>{noun}</SwapText><span className={styles.selectedWord}> {t.selected}</span></span>
              {/* Always mounted, so a status that appears after an action is announced like one that changes. */}
              <span className={styles.status} role="status">{typeof status === "string" && status ? <SwapText reduce={reduce}>{status}</SwapText> : status}</span>
            </span>
            <span id={summaryId} className={styles.srOnly}>{t.announce(count, noun)}</span>
          </div>
          <AnimatePresence initial={false}>
            {canSelectAll && !tucked && !confirm && <Slot key="select-all" reduce={reduce}>
              <Button size="sm" variant="ghost" className={styles.ghostText} loading={selectingAll} onClick={() => void selectAll()}>{t.selectAll(total)}</Button>
            </Slot>}
          </AnimatePresence>
          <span className={styles.divider} aria-hidden="true" />
          <div className={styles.stage}>
            <AnimatePresence initial={false} mode="popLayout">
              {confirm && asking ? (
                <Region key={`confirm-${asking.id}`} reduce={reduce}>
                  <ConfirmMorph
                    ref={confirmRef}
                    className={styles.confirm}
                    label={asking.label}
                    icon={asking.icon}
                    prompt={confirm.prompt}
                    confirmLabel={confirm.confirmLabel ?? asking.label}
                    cancelLabel={t.cancel}
                    pendingLabel={confirm.pendingLabel ?? t.working}
                    doneLabel={confirm.doneLabel ?? t.done}
                    confirmMode={confirm.mode ?? "press"}
                    tone={tone}
                    state={question!.phase}
                    resultTimeout={RESULT_MS}
                    onStateChange={(phase) => { if (phase === "idle") closeQuestion(); else setQuestion({ id: asking.id, phase }); }}
                    onConfirm={asking.onSelect}
                  />
                </Region>
              ) : (
                <Region key="actions" reduce={reduce}>
                  {inline.map((action) => <ActionButton key={action.id} action={action} compact={icons} pending={pendingId === action.id} waiting={busy && pendingId !== action.id} onRun={(next) => void run(next, next.id)} />)}
                  <AnimatePresence initial={false}>
                    {menu.length > 0 && <Slot key="more" reduce={reduce}><DropdownMenu label={t.more} items={menu} iconOnly size="sm" align="end" /></Slot>}
                  </AnimatePresence>
                </Region>
              )}
            </AnimatePresence>
          </div>
          <AnimatePresence initial={false}>
            {!confirm && <Slot key="clear" reduce={reduce}>
              <span className={styles.divider} aria-hidden="true" />
              <Tip content={icons ? clearLabel : undefined}>
                <Button size="sm" variant="ghost" className={icons ? styles.iconOnly : styles.ghostText} aria-label={icons ? clearLabel : undefined} onClick={onClear}>
                  {icons ? <X size={16} strokeWidth={1.75} aria-hidden="true" /> : clearLabel}
                </Button>
              </Tip>
            </Slot>}
          </AnimatePresence>
        </div>
      </motion.div>
    </motion.div>
  );
}

/**
 * A selection toolbar that docks to the bottom edge while anything is selected. The count rolls, the noun follows it, and
 * the bar springs to its content, so it grows into "Select all", an overflow menu or an inline confirmation instead of
 * snapping. Destructive actions ask in place through confirm-morph; Escape clears; arrows move between controls.
 */
export const BulkActionBar = forwardRef<HTMLDivElement, BulkActionBarProps>(function BulkActionBar(
  { count, itemName, position = "sticky", labels, className, style, ...barProps },
  forwardedRef,
) {
  const rootRef = useRef<HTMLDivElement>(null);
  const reduce = useReducedMotion();
  const t = useLabels("bulk-action-bar", DEFAULT_LABELS, labels);
  const { notice, say } = useAnnouncer(ANNOUNCE_DELAY);
  const noun = (itemName ?? t.items)[count === 1 ? "one" : "other"];

  useImperativeHandle(forwardedRef, () => rootRef.current as HTMLDivElement);

  // The bar never takes focus when it appears; the count is spoken instead, once it settles.
  const spoken = useRef(count);
  useEffect(() => {
    if (spoken.current === count) return;
    spoken.current = count;
    say(count > 0 ? t.announce(count, noun) : t.cleared);
  }, [count, noun, say, t]);

  return (
    <div ref={rootRef} className={[styles.root, className].filter(Boolean).join(" ")} style={style} data-position={position} data-bulk-action-bar="">
      <AnimatePresence initial={false}>
        {count > 0 && <Bar key="bar" count={count} noun={noun} t={t} reduce={reduce} say={say} {...barProps} />}
      </AnimatePresence>
      <span className={styles.srOnly} aria-live="polite">{notice}</span>
    </div>
  );
});

BulkActionBar.displayName = "BulkActionBar";

export default BulkActionBar;
