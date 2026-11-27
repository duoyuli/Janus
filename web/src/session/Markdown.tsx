// An agent's Markdown, drawn the way the Mac app's `MarkdownText` (UI/Markdown.swift) draws it:
// headings, paragraphs, lists, quotes, fenced code, tables and rules, with inline code, bold,
// italics, strikethrough and links. Only how text is shown; nothing is read out of it. No HTML
// in the text is ever rendered: everything is a text node.

import type { ComponentChildren, VNode } from "preact";

type Block =
  | { kind: "heading"; level: number; text: string }
  | { kind: "para"; text: string }
  | { kind: "code"; lang: string; text: string }
  | { kind: "quote"; text: string }
  | { kind: "list"; ordered: boolean; start: number; items: string[] }
  | { kind: "table"; head: string[]; rows: string[][] }
  | { kind: "rule" };

const cells = (line: string) =>
  line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((c) => c.trim());

/** The blocks of `text`, line by line. */
export function blocks(text: string): Block[] {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const out: Block[] = [];
  let i = 0;
  const isList = (l: string) => /^\s{0,3}([-*+]|\d+[.)])\s+/.test(l);
  const startsBlock = (l: string) => /^\s{0,3}(#{1,6}\s|```|~~~|>|([-*_])\s*\2\s*\2[\s\-*_]*$)/.test(l) || isList(l);
  while (i < lines.length) {
    const line = lines[i]!;
    if (!line.trim()) {
      i++;
      continue;
    }
    const fence = line.match(/^\s{0,3}(```|~~~)\s*(\S*)/);
    if (fence) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !lines[i]!.trim().startsWith(fence[1]!)) body.push(lines[i++]!);
      i++;
      out.push({ kind: "code", lang: fence[2] ?? "", text: body.join("\n") });
      continue;
    }
    const h = line.match(/^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/);
    if (h) {
      out.push({ kind: "heading", level: h[1]!.length, text: h[2]! });
      i++;
      continue;
    }
    if (/^\s{0,3}([-*_])\s*\1\s*\1[\s\-*_]*$/.test(line)) {
      out.push({ kind: "rule" });
      i++;
      continue;
    }
    if (/^\s{0,3}>/.test(line)) {
      const body: string[] = [];
      while (i < lines.length && /^\s{0,3}>/.test(lines[i]!)) body.push(lines[i++]!.replace(/^\s{0,3}>\s?/, ""));
      out.push({ kind: "quote", text: body.join("\n") });
      continue;
    }
    if (line.includes("|") && i + 1 < lines.length && /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/.test(lines[i + 1]!)) {
      const head = cells(line);
      const rows: string[][] = [];
      i += 2;
      while (i < lines.length && lines[i]!.includes("|") && lines[i]!.trim()) rows.push(cells(lines[i++]!));
      out.push({ kind: "table", head, rows });
      continue;
    }
    if (isList(line)) {
      const ordered = /^\s*\d/.test(line);
      const start = ordered ? parseInt(line.trim(), 10) : 1;
      const items: string[] = [];
      while (i < lines.length && lines[i]!.trim()) {
        const l = lines[i]!;
        if (isList(l) && /^\s{0,3}\S/.test(l)) items.push(l.replace(/^\s*([-*+]|\d+[.)])\s+/, ""));
        else if (items.length) items[items.length - 1] += "\n" + l.trim().replace(/^([-*+]|\d+[.)])\s+/, "• ");
        else break;
        i++;
      }
      out.push({ kind: "list", ordered, start, items });
      continue;
    }
    const body: string[] = [];
    while (i < lines.length && lines[i]!.trim() && !(body.length && startsBlock(lines[i]!))) body.push(lines[i++]!);
    out.push({ kind: "para", text: body.join("\n") });
  }
  return out;
}

const INLINE = /(`+)([\s\S]*?)\1|\*\*([\s\S]+?)\*\*|__([\s\S]+?)__|~~([\s\S]+?)~~|\*([^*\n]+)\*|(?<![\w])_([^_\n]+)_(?![\w])|\[([^\]\n]+)\]\(([^)\s]+)\)|(https?:\/\/[^\s<>()]+[^\s<>().,;:!?'"])/;

/** Inline Markdown as text, code, emphasis and links. */
export function inline(text: string): ComponentChildren[] {
  const out: ComponentChildren[] = [];
  let rest = text;
  while (rest) {
    const m = rest.match(INLINE);
    if (!m || m.index === undefined) {
      out.push(rest);
      break;
    }
    if (m.index > 0) out.push(rest.slice(0, m.index));
    const [whole, , code, bold1, bold2, strike, em1, em2, label, href, url] = m;
    if (code !== undefined) out.push(<code>{code.trim()}</code>);
    else if (bold1 !== undefined || bold2 !== undefined) out.push(<strong>{inline(bold1 ?? bold2!)}</strong>);
    else if (strike !== undefined) out.push(<s>{inline(strike)}</s>);
    else if (em1 !== undefined || em2 !== undefined) out.push(<em>{inline(em1 ?? em2!)}</em>);
    else if (label !== undefined) out.push(link(href!, inline(label)));
    else if (url !== undefined) out.push(link(url, url));
    rest = rest.slice(m.index + whole.length);
  }
  return out;
}

function link(href: string, children: ComponentChildren): VNode {
  const safe = /^(https?:|mailto:)/i.test(href);
  return safe ? (
    <a href={href} target="_blank" rel="noreferrer noopener">
      {children}
    </a>
  ) : (
    <span class="md-link">{children}</span>
  );
}

/** Lines of a paragraph keep their breaks, as the agent wrote them. */
function lines(text: string): ComponentChildren[] {
  return text.split("\n").flatMap((l, i) => (i ? [<br />, ...inline(l)] : inline(l)));
}

export function Markdown({ text, class: cls }: { text: string; class?: string }) {
  return (
    <div class={"markdown" + (cls ? ` ${cls}` : "")}>
      {blocks(text).map((b) => {
        switch (b.kind) {
          case "heading": {
            const H = `h${Math.min(6, b.level + 2)}` as "h3";
            return <H>{inline(b.text)}</H>;
          }
          case "para":
            return <p>{lines(b.text)}</p>;
          case "code":
            return (
              <pre class="code-block">
                <code>{b.text}</code>
              </pre>
            );
          case "quote":
            return <blockquote>{lines(b.text)}</blockquote>;
          case "list": {
            const items = b.items.map((it) => <li>{lines(it)}</li>);
            return b.ordered ? <ol start={b.start}>{items}</ol> : <ul>{items}</ul>;
          }
          case "table":
            return (
              <div class="table-wrap">
                <table>
                  <thead>
                    <tr>{b.head.map((c) => <th>{inline(c)}</th>)}</tr>
                  </thead>
                  <tbody>
                    {b.rows.map((r) => (
                      <tr>{r.map((c) => <td>{inline(c)}</td>)}</tr>
                    ))}
                  </tbody>
                </table>
              </div>
            );
          case "rule":
            return <hr />;
        }
      })}
    </div>
  );
}
