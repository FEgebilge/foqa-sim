import { useCallback, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { NoticeContext, type Notice } from "../lib/context";

/** Short-lived messages in the corner: failed actions, and landings reported while the app is open. */
export function NoticeProvider({ children }: { children: ReactNode }) {
  const [notices, setNotices] = useState<(Notice & { id: number })[]>([]);
  const dismiss = useCallback((id: number) => setNotices((list) => list.filter((n) => n.id !== id)), []);
  const notify = useCallback((notice: Notice) => {
    const id = Date.now() + Math.random();
    setNotices((list) => [...list.slice(-2), { ...notice, id }]);
    window.setTimeout(() => dismiss(id), 12_000);
  }, [dismiss]);

  return (
    <NoticeContext value={notify}>
      {children}
      <div className="pointer-events-none fixed right-4 bottom-4 z-50 flex w-[22rem] max-w-[calc(100vw-2rem)] flex-col gap-2" role="region" aria-label="Notifications">
        <AnimatePresence initial={false}>
          {notices.map((notice) => (
            <motion.div key={notice.id} layout initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, scale: 0.97 }}
              role={notice.tone === "error" ? "alert" : "status"}
              className="pointer-events-auto rounded-[var(--radius-panel)] border border-line bg-raised p-3 shadow-[var(--shadow-floating)]"
              style={{ borderLeft: `3px solid ${notice.tone === "error" ? "var(--danger)" : "var(--accent)"}` }}>
              <div className="flex items-start gap-2">
                <div className="min-w-0 flex-1">
                  <p className="text-sm font-medium">{notice.title}</p>
                  {notice.body ? <div className="mt-1 text-xs text-fg-2">{notice.body}</div> : null}
                  {notice.action ? (
                    <button type="button" className="mt-2 text-xs font-medium text-accent-strong hover:underline" onClick={() => { notice.action?.run(); dismiss(notice.id); }}>{notice.action.label}</button>
                  ) : null}
                </div>
                <button type="button" className="rounded p-0.5 text-fg-3 hover:text-fg" aria-label="Dismiss" onClick={() => dismiss(notice.id)}><X size={14} /></button>
              </div>
            </motion.div>
          ))}
        </AnimatePresence>
      </div>
    </NoticeContext>
  );
}
