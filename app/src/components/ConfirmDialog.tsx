import { useEffect, useRef, type ReactNode } from "react";
import { Button } from "@ui/components/button/button";

interface Props {
  open: boolean;
  title: string;
  children: ReactNode;
  confirmLabel: string;
  busy?: boolean;
  /** The confirm button: destructive by default; `primary` for a question that changes nothing for good. */
  variant?: "danger" | "primary";
  cancelLabel?: string;
  onConfirm: () => void;
  onCancel: () => void;
}

/** A modal question on the native <dialog>: focus is trapped and Escape cancels. The confirm button is the destructive one unless `variant` says otherwise. */
export function ConfirmDialog({ open, title, children, confirmLabel, busy, variant = "danger", cancelLabel = "Cancel", onConfirm, onCancel }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog || open === dialog.open) return;
    if (open) dialog.showModal();
    else dialog.close();
  }, [open]);

  return (
    <dialog ref={ref} onCancel={(event) => { event.preventDefault(); if (!busy) onCancel(); }} aria-labelledby="confirm-title"
      className="panel m-auto w-[min(28rem,calc(100vw-2rem))] p-5 text-fg backdrop:bg-black/50">
      <h2 id="confirm-title" className="font-display text-base font-semibold">{title}</h2>
      <div className="mt-2 text-sm text-fg-2">{children}</div>
      <div className="mt-5 flex justify-end gap-2">
        <Button variant="secondary" size="sm" onClick={onCancel} disabled={busy && variant === "danger"}>{cancelLabel}</Button>
        <Button variant={variant} size="sm" onClick={onConfirm} loading={busy}>{confirmLabel}</Button>
      </div>
    </dialog>
  );
}
