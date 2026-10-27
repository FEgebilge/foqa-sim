import { useEffect, useRef, useState } from "react";
import { Button } from "@ui/components/button/button";
import { appVersion, markSeen, unseenEntries, type DevlogEntry } from "../../lib/devlog";
import { openDevlog } from "../../lib/route";
import { DevlogEntryView } from "./DevlogEntryView";

/** After an update, the notes of every version since the last one the user saw. A first install only records the version. */
export function WhatsNewDialog() {
  const [entries, setEntries] = useState<DevlogEntry[]>([]);
  const [version, setVersion] = useState("");
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    appVersion().then((current) => {
      const unseen = unseenEntries(current);
      if (unseen?.length) { setVersion(current); setEntries(unseen); }
      else markSeen(current);
    }, () => undefined);
  }, []);
  useEffect(() => { if (entries.length && !ref.current?.open) ref.current?.showModal(); }, [entries]);

  const close = (then?: () => void) => { markSeen(version); ref.current?.close(); setEntries([]); then?.(); };
  if (!entries.length) return null;

  return (
    <dialog ref={ref} onCancel={(event) => { event.preventDefault(); close(); }} aria-labelledby="whatsnew-title"
      className="panel m-auto flex max-h-[min(40rem,calc(100vh-2rem))] w-[min(36rem,calc(100vw-2rem))] flex-col p-0 text-fg backdrop:bg-black/50">
      <div className="border-b border-line px-5 py-4">
        <p className="eyebrow">Updated</p>
        <h2 id="whatsnew-title" className="mt-1 font-display text-xl font-semibold tracking-tight">What's new in FOQA-Sim</h2>
      </div>
      <div className="flex flex-col gap-6 overflow-y-auto px-5 py-4">
        {entries.map((entry) => <DevlogEntryView key={entry.version} entry={entry} />)}
      </div>
      <div className="flex justify-end gap-2 border-t border-line px-5 py-3">
        <Button variant="secondary" size="sm" onClick={() => close(openDevlog)}>See all updates</Button>
        <Button size="sm" onClick={() => close()}>Close</Button>
      </div>
    </dialog>
  );
}
