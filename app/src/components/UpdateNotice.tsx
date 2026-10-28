import { useState } from "react";
import { X } from "lucide-react";
import { Button } from "@ui/components/button/button";
import { Markdown } from "../lib/markdown";
import type { Updater } from "../lib/use-updater";

/** A small card in the corner while a newer version waits: its notes, and "Install and restart" (refused while a flight records). */
export function UpdateNotice({ updater }: { updater: Updater }) {
  const [notes, setNotes] = useState(false);
  const { state, error, recording, dismissed } = updater;
  if (state.phase !== "available" && state.phase !== "installing") return null;
  if (dismissed && state.phase === "available") return null;
  const installing = state.phase === "installing";
  const percent = installing && state.total ? Math.min(100, Math.round((state.done / state.total) * 100)) : null;

  return (
    <section aria-label="Update available" role="status" className="panel fixed bottom-4 left-4 z-50 flex w-[22rem] max-w-[calc(100vw-2rem)] flex-col gap-2 p-3 shadow-[var(--shadow-floating)]" style={{ borderLeft: "3px solid var(--accent)" }}>
      <div className="flex items-start gap-2">
        <p className="min-w-0 flex-1 text-sm font-medium">FOQA-Sim {state.update.version} is available</p>
        {installing ? null : <button type="button" className="rounded p-0.5 text-fg-3 hover:text-fg" aria-label="Dismiss" onClick={updater.dismiss}><X size={14} /></button>}
      </div>
      {notes ? <div className="max-h-64 overflow-y-auto"><Markdown source={state.update.notes || "No release notes."} /></div> : null}
      {installing ? (
        <div className="text-xs text-fg-2" aria-live="polite">
          <div className="h-1.5 overflow-hidden rounded-full bg-muted"><div className="h-full bg-accent transition-[width]" style={{ width: percent === null ? "100%" : `${percent}%`, opacity: percent === null ? 0.4 : 1 }} /></div>
          <p className="mt-1">{percent === null ? "Downloading…" : `Downloading… ${percent}%`} The app restarts when it is done.</p>
        </div>
      ) : (
        <>
          <div className="flex items-center gap-2">
            <Button size="sm" variant="ghost" onClick={() => setNotes((open) => !open)} aria-expanded={notes}>What's new</Button>
            <Button size="sm" className="ml-auto" onClick={() => void updater.install()} disabled={recording}>Install and restart</Button>
          </div>
          {recording ? <p className="text-xs text-fg-3">Not while a flight is being recorded: install after it closes.</p> : null}
        </>
      )}
      {error ? <p role="alert" className="text-xs text-danger">Update failed: {error}</p> : null}
    </section>
  );
}
