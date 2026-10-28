import { Fragment, type ReactNode } from "react";
import { blocks, spans } from "./markdown-blocks.ts";

/* A deliberately small Markdown subset for our own bundled devlog text (see markdown-blocks.ts). It builds React elements
   (no HTML strings, no dangerouslySetInnerHTML), so nothing in a file can inject markup, and we avoid a dependency. */
function inline(text: string): ReactNode[] {
  return spans(text).map((span, key) => {
    switch (span.kind) {
      case "bold": return <strong key={key} className="font-semibold text-fg">{span.text}</strong>;
      case "code": return <code key={key} className="readout rounded bg-muted px-1 py-0.5 text-[0.85em]">{span.text}</code>;
      case "link": return <a key={key} href={span.href} target="_blank" rel="noopener noreferrer" className="text-accent underline-offset-2 hover:underline">{span.text}</a>;
      default: return span.text;
    }
  });
}

export function Markdown({ source }: { source: string }) {
  return (
    <div className="flex flex-col gap-3 text-sm leading-relaxed text-fg-2">
      {blocks(source).map((block, i) => (
        <Fragment key={i}>
          {block.kind === "heading" ? <h3 className="mt-1 font-display text-[11px] font-semibold uppercase tracking-wider text-fg-3">{inline(block.text)}</h3>
            : block.kind === "para" ? <p>{inline(block.text)}</p>
            : (
              <ul className="flex list-disc flex-col gap-1.5 pl-5 marker:text-fg-3">
                {block.items.map((item, j) => (
                  <li key={j}>
                    {inline(item.text)}
                    {item.children.length ? <ul className="mt-1.5 flex list-[circle] flex-col gap-1 pl-5">{item.children.map((c, k) => <li key={k}>{inline(c)}</li>)}</ul> : null}
                  </li>
                ))}
              </ul>
            )}
        </Fragment>
      ))}
    </div>
  );
}
