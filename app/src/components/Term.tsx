import type { ReactNode } from "react";
import { Tooltip } from "@ui/components/tooltip/tooltip";
import { GLOSSARY, type TermId } from "../lib/glossary";

/** A glossary term: dotted underline, its definition on hover or keyboard focus (lib/glossary.ts). Never inside a button:
 * it is a focus stop of its own. */
export function Term({ id, children }: { id: TermId; children?: ReactNode }) {
  const entry: { term: string; long?: string; text: string } = GLOSSARY[id];
  return (
    <Tooltip content={<span className="block max-w-[300px] text-xs">{entry.long ? <b className="block font-semibold">{entry.long}</b> : null}{entry.text}</span>}>
      <span tabIndex={0} aria-description={`${entry.long ? `${entry.long}. ` : ""}${entry.text}`}
        className="cursor-help underline decoration-dotted decoration-1 underline-offset-[3px] [text-decoration-color:color-mix(in_oklab,currentColor_55%,transparent)] focus-visible:rounded-sm focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-[var(--accent)]">
        {children ?? entry.term}
      </span>
    </Tooltip>
  );
}
