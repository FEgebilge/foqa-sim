import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import { AnimatePresence, motion, MotionConfig } from "motion/react";
import { NoticeContext, type Notice } from "../lib/context";
import { autoCloses, Countdown } from "../lib/notice-timing";

/** Messages in the corner: failed actions, and landings reported while the app is open.
 * A plain notice closes after 12 s (paused while the pointer or focus is on it); an error, or a notice with an action, stays until dismissed.
 * Escape on a focused notice dismisses it. */
export function NoticeProvider({ children }: { children: ReactNode }) {
  const [notices, setNotices] = useState<(Notice & { id: number })[]>([]);
  const dismiss = useCallback((id: number) => setNotices((list) => list.filter((n) => n.id !== id)), []);
  const notify = useCallback((notice: Notice) => {
    const id = Date.now() + Math.random();
    setNotices((list) => [...list.slice(-2), { ...notice, id }]);
  }, []);

  return (
    <NoticeContext value={notify}>
      <MotionConfig reducedMotion="user">
      {children}
      <div className="pointer-events-none fixed right-4 bottom-4 z-50 flex w-[22rem] max-w-[calc(100vw-2rem)] flex-col gap-2" role="region" aria-label="Notifications">
        <AnimatePresence initial={false}>
          {notices.map((notice) => <NoticeCard key={notice.id} notice={notice} dismiss={dismiss} />)}
        </AnimatePresence>
      </div>
      </MotionConfig>
    </NoticeContext>
  );
}

function NoticeCard({ notice, dismiss }: { notice: Notice & { id: number }; dismiss: (id: number) => void }) {
  const { id, tone, title, body, action } = notice;
  const countdown = useRef<Countdown | null>(null);
  const held = useRef({ pointer: false, focus: false });
  useEffect(() => {
    if (!autoCloses(notice)) return;
    const c = (countdown.current = new Countdown(() => dismiss(id)));
    return () => { c.pause(); countdown.current = null; };
  }, [id, notice, dismiss]);
  /** Paused while the pointer is over the card or focus is inside it. */
  const hold = (key: "pointer" | "focus", on: boolean) => {
    held.current[key] = on;
    if (held.current.pointer || held.current.focus) countdown.current?.pause();
    else countdown.current?.resume();
  };

  return (
    <motion.div layout initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, scale: 0.97 }}
      role={tone === "error" ? "alert" : "status"}
      onMouseEnter={() => hold("pointer", true)} onMouseLeave={() => hold("pointer", false)}
      onFocus={() => hold("focus", true)} onBlur={() => hold("focus", false)}
      onKeyDown={(event) => { if (event.key === "Escape") { event.stopPropagation(); dismiss(id); } }}
      className="pointer-events-auto rounded-[var(--radius-panel)] border border-line bg-raised p-3 shadow-[var(--shadow-floating)]"
      style={{ borderLeft: `3px solid ${tone === "error" ? "var(--danger)" : "var(--accent)"}` }}>
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium">{title}</p>
          {body ? <div className="mt-1 text-xs text-fg-2">{body}</div> : null}
          {action ? (
            <button type="button" className="mt-2 text-xs font-medium text-accent-strong hover:underline" onClick={() => { action.run(); dismiss(id); }}>{action.label}</button>
          ) : null}
        </div>
        <button type="button" className="rounded p-0.5 text-fg-3 hover:text-fg" aria-label={`Dismiss: ${title}`} onClick={() => dismiss(id)}><X size={14} aria-hidden="true" /></button>
      </div>
    </motion.div>
  );
}
