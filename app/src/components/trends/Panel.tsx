import type { ReactNode } from "react";

/** One panel of the Trends grid: what it shows and why it matters in one line, then the content. */
export function Panel({ title, why, aside, className = "", children }: { title: string; why: string; aside?: ReactNode; className?: string; children: ReactNode }) {
  return (
    <section className={`panel flex min-w-0 flex-col ${className}`} aria-label={title}>
      <header className="flex items-start justify-between gap-3 border-b border-line-subtle px-4 py-3">
        <div className="min-w-0">
          <h2 className="font-display text-sm font-semibold">{title}</h2>
          <p className="mt-0.5 text-xs text-fg-3">{why}</p>
        </div>
        {aside ? <div className="shrink-0">{aside}</div> : null}
      </header>
      <div className="min-w-0 flex-1">{children}</div>
    </section>
  );
}
