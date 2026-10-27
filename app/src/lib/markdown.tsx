import { Fragment, type ReactNode } from "react";

/* A deliberately small Markdown subset for our own bundled devlog text: ## / ### headings, "- " lists, paragraphs,
   **bold**, `code` and [links](http...). It builds React elements (no HTML strings, no dangerouslySetInnerHTML), so
   nothing in a file can inject markup, and we avoid a dependency for ~40 lines. */
const INLINE = /\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g;

function inline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  let last = 0;
  for (const m of text.matchAll(INLINE)) {
    if (m.index > last) out.push(text.slice(last, m.index));
    const key = m.index;
    if (m[1] !== undefined) out.push(<strong key={key} className="font-semibold text-fg">{m[1]}</strong>);
    else if (m[2] !== undefined) out.push(<code key={key} className="readout rounded bg-muted px-1 py-0.5 text-[0.85em]">{m[2]}</code>);
    else out.push(<a key={key} href={m[4]} target="_blank" rel="noopener noreferrer" className="text-accent underline-offset-2 hover:underline">{m[3]}</a>);
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

export function Markdown({ source }: { source: string }) {
  const blocks: ReactNode[] = [];
  let list: string[] = [];
  let para: string[] = [];
  const flush = () => {
    const key = blocks.length;
    if (list.length) blocks.push(<ul key={key} className="flex list-disc flex-col gap-1.5 pl-5 marker:text-fg-3">{list.map((item, i) => <li key={i}>{inline(item)}</li>)}</ul>);
    if (para.length) blocks.push(<p key={key}>{inline(para.join(" "))}</p>);
    list = [];
    para = [];
  };
  for (const line of source.split(/\r?\n/)) {
    const heading = /^(#{2,3})\s+(.*)$/.exec(line);
    const item = /^[-*]\s+(.*)$/.exec(line);
    if (heading) {
      flush();
      blocks.push(<h3 key={blocks.length} className="mt-1 font-display text-[11px] font-semibold uppercase tracking-wider text-fg-3">{inline(heading[2]!)}</h3>);
    } else if (item) {
      if (para.length) flush();
      list.push(item[1]!);
    } else if (line.trim()) {
      if (list.length) flush();
      para.push(line.trim());
    } else flush();
  }
  flush();
  return <div className="flex flex-col gap-3 text-sm leading-relaxed text-fg-2">{blocks.map((b, i) => <Fragment key={i}>{b}</Fragment>)}</div>;
}
