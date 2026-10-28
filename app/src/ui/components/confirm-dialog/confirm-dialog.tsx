"use client";

import { useEffect, useId, useRef, useState, useSyncExternalStore } from "react";
import type { FormEvent, ReactNode } from "react";
import { Alert } from "@ui/components/alert/alert";
import { Button } from "@ui/components/button/button";
import { Dialog, DialogContent } from "@ui/components/dialog/dialog";
import type { DialogContentProps } from "@ui/components/dialog/dialog";
import { Input } from "@ui/components/input/input";
import { useArcLocale, useLabels } from "@ui/lib/arc-provider";
import styles from "./confirm-dialog.module.css";

export interface ConfirmDialogLabels {
  confirm: string;
  cancel: string;
  /** Label of the typed-confirmation field, from the text to type. */
  typePrompt: (text: string) => string;
  /** Title of the inline error shown when `onConfirm` throws. */
  failed: string;
}
const DEFAULT_LABELS: ConfirmDialogLabels = { confirm: "Confirm", cancel: "Cancel", typePrompt: text => `Type ${text} to confirm`, failed: "Could not complete this action" };
declare module "@ui/lib/arc-provider" { interface ArcLabels { "confirm-dialog": ConfirmDialogLabels } }

export interface ConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  /** More to read before deciding, such as what will be affected. Fields in it submit with Enter; do not nest a form. */
  children?: ReactNode;
  /** `danger` fills Confirm in the danger colour and opens the dialog on Cancel, so Enter never confirms by accident. */
  tone?: "default" | "danger";
  /** Wins over `labels.confirm`. Name the action ("Delete project"), not "OK". */
  confirmLabel?: string;
  /** Wins over `labels.cancel`. */
  cancelLabel?: string;
  /** Keeps Confirm locked until this text is typed. Case, extra spaces and the locale's letter case are forgiven ("ışık" matches "IŞIK" in tr-TR). */
  typeToConfirm?: string;
  /**
   * Runs when the user confirms. While it runs, Confirm shows a spinner and the dialog cannot be dismissed. When it finishes the dialog
   * closes itself; when it throws, the dialog stays open with the error inline, so the user can retry or cancel.
   */
  onConfirm: () => void | Promise<void>;
  /** Which button takes focus on open. Defaults to Cancel for `danger` and Confirm otherwise; the typed-confirmation field takes it first unless this is set. */
  initialFocus?: "confirm" | "cancel";
  size?: DialogContentProps["size"];
  labels?: Partial<ConfirmDialogLabels>;
  /** BCP 47 locale for the typed comparison. Defaults to the ArcProvider's. */
  locale?: string;
}

const fold = (text: string, locale: string) => text.trim().replace(/\s+/g, " ").toLocaleLowerCase(locale);

/** A question that waits for a decision. For a promise instead of state, see `createConfirm`. */
export function ConfirmDialog({ open, onOpenChange, title, description, children, tone = "default", confirmLabel, cancelLabel, typeToConfirm, onConfirm, initialFocus, size = "sm", labels, locale }: ConfirmDialogProps) {
  const t = useLabels("confirm-dialog", DEFAULT_LABELS, labels);
  const arc = useArcLocale();
  const [typed, setTyped] = useState("");
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<Error | null>(null);
  // Every opening starts clean.
  const [wasOpen, setWasOpen] = useState(open);
  if (open !== wasOpen) { setWasOpen(open); if (open) { setTyped(""); setFailure(null); } }
  const input = useRef<HTMLInputElement>(null), cancel = useRef<HTMLButtonElement>(null), confirm = useRef<HTMLButtonElement>(null);
  const danger = tone === "danger";
  const formId = useId();
  const unlocked = !typeToConfirm || fold(typed, locale ?? arc.locale) === fold(typeToConfirm, locale ?? arc.locale);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (pending || !unlocked) return;
    setPending(true);
    setFailure(null);
    try { await onConfirm(); } catch (cause) { setFailure(cause instanceof Error ? cause : new Error()); setPending(false); return; }
    setPending(false);
    onOpenChange(false);
  }

  return <Dialog open={open} onOpenChange={next => { if (!pending) onOpenChange(next); }}>
    <DialogContent role="alertdialog" size={size} title={title} description={description} hideClose aria-busy={pending || undefined}
      onOpenAutoFocus={event => { event.preventDefault(); (initialFocus ? { confirm, cancel }[initialFocus] : typeToConfirm ? input : danger ? cancel : confirm).current?.focus(); }}
      footer={<>
        <Button ref={cancel} variant="secondary" disabled={pending} onClick={() => onOpenChange(false)}>{cancelLabel ?? t.cancel}</Button>
        <Button ref={confirm} variant="primary" className={danger ? styles.destructive : undefined} loading={pending} disabled={!unlocked} type="submit" form={formId}>{confirmLabel ?? t.confirm}</Button>
      </>}>
      {/* The footer sits outside the body, so Confirm joins this form by id: Enter in any field submits through the same guarded path. */}
      <form id={formId} className={styles.stack} onSubmit={submit}>
        {children}
        {typeToConfirm ? <Input ref={input} label={t.typePrompt(typeToConfirm)} value={typed} placeholder={typeToConfirm} autoComplete="off" spellCheck={false} disabled={pending} onChange={event => setTyped(event.target.value)} /> : null}
        {failure ? <Alert tone="danger" title={t.failed}>{failure.message}</Alert> : null}
      </form>
    </DialogContent>
  </Dialog>;
}

/** What `confirm()` takes: the dialog's content, and `onConfirm` is optional. */
export type ConfirmOptions = Omit<ConfirmDialogProps, "open" | "onOpenChange" | "onConfirm"> & { onConfirm?: ConfirmDialogProps["onConfirm"] };
interface Request { id: number; options: ConfirmOptions; resolve: (confirmed: boolean) => void }

/**
 * Confirmation as a promise, for event handlers and modules that are not components. Create it once at module level,
 * mount `ConfirmHost` once inside the app (below the ArcProvider, so labels apply), then await `confirm` anywhere:
 *
 *   export const { confirm, ConfirmHost } = createConfirm();
 *   if (await confirm({ title: "Delete project?", tone: "danger", confirmLabel: "Delete", onConfirm: () => remove(id) })) refresh();
 *
 * It resolves true once `onConfirm` has succeeded (or straight away without one) and false on Cancel, Escape, a press
 * outside, or when the host unmounts. A request made while another is open waits for its turn.
 */
export function createConfirm() {
  let queue: Request[] = [], seq = 0, hosts = 0;
  const confirmed = new WeakSet<Request>();
  const listeners = new Set<() => void>();
  const subscribe = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };
  const commit = (next: Request[]) => { queue = next; listeners.forEach(listener => listener()); };
  const settle = (request: Request) => { if (queue[0] !== request) return; commit(queue.slice(1)); request.resolve(confirmed.has(request)); };
  const confirm = (options: ConfirmOptions) => new Promise<boolean>(resolve => commit([...queue, { id: ++seq, options, resolve }]));

  function ConfirmHost() {
    const current = useSyncExternalStore(subscribe, () => queue[0] ?? null, () => null);
    // The answered request stays rendered while its dialog leaves, so the copy does not vanish mid-exit.
    const [last, setLast] = useState(current);
    if (current && current !== last) setLast(current);
    // Pending requests are dropped only when no host remains after this tick, so StrictMode's remount and a host that moves
    // in the tree keep them.
    useEffect(() => {
      hosts += 1;
      return () => {
        hosts -= 1;
        queueMicrotask(() => { if (hosts) return; const abandoned = queue; commit([]); abandoned.forEach(request => request.resolve(false)); });
      };
    }, []);
    const shown = current ?? last;
    return shown && <ConfirmDialog key={shown.id} {...shown.options} open={current === shown} onOpenChange={next => { if (!next) settle(shown); }} onConfirm={async () => { await shown.options.onConfirm?.(); confirmed.add(shown); }} />;
  }
  return { confirm, ConfirmHost };
}

export default ConfirmDialog;
