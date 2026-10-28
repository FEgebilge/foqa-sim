// The Markdown subset of our bundled release notes, parsed to a plain model; markdown.tsx draws it.
export type Span = { kind: "text"; text: string } | { kind: "bold" | "code"; text: string } | { kind: "link"; text: string; href: string };
export type Block =
  | { kind: "heading"; text: string }
  | { kind: "para"; text: string }
  | { kind: "list"; items: { text: string; children: string[] }[] };

const INLINE = /\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g;

export function spans(text: string): Span[] {
  const out: Span[] = [];
  let last = 0;
  for (const m of text.matchAll(INLINE)) {
    if (m.index > last) out.push({ kind: "text", text: text.slice(last, m.index) });
    if (m[1] !== undefined) out.push({ kind: "bold", text: m[1] });
    else if (m[2] !== undefined) out.push({ kind: "code", text: m[2] });
    else out.push({ kind: "link", text: m[3]!, href: m[4]! });
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push({ kind: "text", text: text.slice(last) });
  return out;
}

/** `##`/`###` headings, `-`/`*` lists (items indented two spaces or more nest one level; a blank line inside a list keeps it one list) and paragraphs. */
export function blocks(source: string): Block[] {
  const out: Block[] = [];
  let list: { text: string; children: string[] }[] = [];
  let para: string[] = [];
  const flush = () => {
    if (list.length) out.push({ kind: "list", items: list });
    if (para.length) out.push({ kind: "para", text: para.join(" ") });
    list = [];
    para = [];
  };
  for (const line of source.split(/\r?\n/)) {
    const heading = /^(#{2,3})\s+(.*)$/.exec(line);
    const item = /^( *)[-*]\s+(.*)$/.exec(line);
    if (heading) {
      flush();
      out.push({ kind: "heading", text: heading[2]! });
    } else if (item) {
      if (para.length) flush();
      const parent = list.at(-1);
      if (item[1]!.length >= 2 && parent) parent.children.push(item[2]!);
      else list.push({ text: item[2]!, children: [] });
    } else if (line.trim()) {
      if (list.length) flush();
      para.push(line.trim());
    } else if (!list.length) flush();
  }
  flush();
  return out;
}
