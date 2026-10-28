import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { ChevronDown, Share2 } from "lucide-react";
import { Button } from "@ui/components/button/button";

/** The menu's width, `w-80`. */
const MENU_PX = 320;

export interface ExportItem {
  label: string;
  /** What the file is for, under the label. */
  hint: string;
  icon: ReactNode;
  /** `identity`: the user asked for the aircraft's title and tail number. */
  run: (identity: boolean) => void;
  /** Takes no identity (the landing card names the type only). */
  anonymous?: boolean;
}

/**
 * The Export/Share button and its menu: one item per file, and the opt-in that adds the aircraft's title and tail number
 * (off each time the menu opens, so nothing identifying leaves by accident). Escape or a click outside closes it.
 */
export function ExportMenu({ items, label = "Export / Share" }: { items: ExportItem[]; label?: string }) {
  const [open, setOpen] = useState(false);
  const [identity, setIdentity] = useState(false);
  /** The menu opens towards the side with room: right-aligned under the button unless that leaves the window. */
  const [left, setLeft] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const menu = useId();

  useEffect(() => {
    if (!open) return;
    const close = (event: Event) => {
      if (event instanceof globalThis.KeyboardEvent ? event.key === "Escape" : !root.current?.contains(event.target as Node)) setOpen(false);
    };
    root.current?.querySelector<HTMLElement>("[role='menuitem']")?.focus();
    document.addEventListener("pointerdown", close);
    document.addEventListener("keydown", close);
    return () => { document.removeEventListener("pointerdown", close); document.removeEventListener("keydown", close); };
  }, [open]);

  /** Arrow keys move between the items, as in a menu. */
  const move = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const all = [...(root.current?.querySelectorAll<HTMLElement>("[role='menuitem'], [role='menuitemcheckbox']") ?? [])];
    const at = all.indexOf(document.activeElement as HTMLElement);
    all[(at + (event.key === "ArrowDown" ? 1 : all.length - 1)) % all.length]?.focus();
  };

  return (
    <div ref={root} className="relative">
      <Button variant="secondary" size="sm" aria-haspopup="menu" aria-expanded={open} aria-controls={menu} onClick={() => { setIdentity(false); setLeft((root.current?.getBoundingClientRect().right ?? 0) < MENU_PX + 16); setOpen((o) => !o); }}>
        <Share2 size={15} aria-hidden="true" />{label}<ChevronDown size={14} aria-hidden="true" />
      </Button>
      {open ? (
        <div id={menu} role="menu" aria-label={label} onKeyDown={move}
          className={`absolute ${left ? "left-0" : "right-0"} z-40 mt-1.5 flex w-80 flex-col rounded-[var(--radius-panel)] border border-line bg-raised p-1 shadow-[var(--shadow-floating)]`}>
          {items.map((item) => (
            <button key={item.label} type="button" role="menuitem" className="flex items-start gap-2.5 rounded-[var(--radius-control)] px-2.5 py-2 text-left outline-none hover:bg-muted focus-visible:bg-muted"
              onClick={() => { setOpen(false); item.run(identity && !item.anonymous); }}>
              <span className="mt-0.5 shrink-0 text-fg-2" aria-hidden="true">{item.icon}</span>
              <span className="min-w-0">
                <span className="block text-sm font-medium">{item.label}</span>
                <span className="block text-xs text-fg-3">{item.hint}</span>
              </span>
            </button>
          ))}
          <div className="mt-1 border-t border-line-subtle px-2.5 pt-2 pb-1.5">
            <label className="flex items-start gap-2 text-xs text-fg-2">
              <input type="checkbox" role="menuitemcheckbox" aria-checked={identity} checked={identity} onChange={(e) => setIdentity(e.target.checked)} className="mt-0.5 accent-[var(--accent)]" />
              <span>Include the aircraft's title and tail number. Off by default; files never include folder paths.</span>
            </label>
          </div>
        </div>
      ) : null}
    </div>
  );
}
