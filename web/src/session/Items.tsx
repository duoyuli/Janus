// The rows of a conversation, each the counterpart of the Mac app's (UI/Run/ChatItem.swift,
// ChatRows.swift): every event as the service worded it in the view's `lines`, a turn's head, a
// group of calls, the agent's text, the files a turn changed. Nothing here words an event itself
// (docs/architecture/20261031-frontend.md#session_turns).

import { useEffect, useState } from "preact/hooks";
import type { Json } from "../lib/api.ts";
import { dur, tilde } from "../lib/format.ts";
import { t } from "../lib/i18n.ts";
import { roles } from "../lib/store.ts";
import { Avatar, Pill } from "../ui/kit.tsx";
import { Icon } from "../ui/Icon.tsx";
import { Markdown } from "./Markdown.tsx";
import type { RunModel } from "./model.ts";
import { RequestCard } from "./Requests.tsx";

/** A run's member as the conversation shows it: its name and model, and the role for its tile. */
export function who(run: RunModel, id: string) {
  if (id === "user") return { name: t`你`, model: "", runtime: "", lead: false, role: { name: t`你`, color: "#0f766e" } };
  if (id === "janus") return { name: "Janus", model: "", runtime: "", lead: false, role: { name: "Janus", init: "JN", color: "#8a94a6" } };
  const m = (run.run.value?.config?.team ?? []).find((x: Json) => x.id === id);
  const lead = !!m?.lead && run.run.value?.kind !== "chat";
  const role = roles.value.find((r) => r.id === m?.role_id) ?? { name: m?.name ?? id, init: m?.init, color: m?.color || (lead ? "#0b6bcb" : "") };
  return { name: m?.name ?? (id === "lead" ? t`负责人` : id), model: m?.model ?? "", runtime: m?.connection_name ?? "", lead, role };
}

/** Paths inside the run's directory written relative to it, as the agent's own interface shows them. */
export function inWorkspace(text: string, cwd: string): string {
  if (!cwd) return text;
  const root = cwd.endsWith("/") ? cwd.slice(0, -1) : cwd;
  return text.split(root + "/").join("");
}

/** Text that is in progress: a light passing over it (`Shimmer`), still under reduced motion. */
export function Shimmer({ text }: { text: string }) {
  return <span class="shimmer">{text}</span>;
}

function Chevron({ open }: { open: boolean }) {
  return (
    <span class={"chevron" + (open ? " open" : "")} aria-hidden="true">
      <Icon name="chevron.right" size={10} />
    </span>
  );
}

/** A fold: its label is a button, its body shows when open. */
function Fold({ label, children, open: start = false, class: cls }: { label: preact.ComponentChildren; children: preact.ComponentChildren; open?: boolean; class?: string }) {
  const [open, setOpen] = useState(start);
  return (
    <div class={"fold" + (cls ? ` ${cls}` : "")}>
      <button type="button" class="fold-row" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Chevron open={open} />
        <span class="fold-label">{label}</span>
      </button>
      {open && <div class="fold-body">{children}</div>}
    </div>
  );
}

export function CodeBlock({ text, max = 240 }: { text: string; max?: number }) {
  return (
    <pre class="code-block" style={{ maxHeight: `${max}px` }}>
      <code>{text}</code>
    </pre>
  );
}

/** A file change with a little context, as the service worked it out (`diffs`). */
export function InlineDiff({ path, lines }: { path: string; lines: Json[] }) {
  return (
    <div class="inline-diff">
      <div class="inline-diff-path">{path}</div>
      {lines.map((l) => (
        <div class={"diff-line" + (l.mark === "+" ? " add" : l.mark === "-" ? " del" : "")}>
          {l.mark}
          {l.text}
        </div>
      ))}
    </div>
  );
}

/** Images stored with the run (`GET /api/runs/{id}/image`). */
export function StoredImages({ run, paths, size = 72 }: { run: RunModel; paths: string[]; size?: number }) {
  if (!paths?.length) return null;
  return (
    <div class="stored-images">
      {paths.map((p) => {
        const src = `${run.path}/image?path=${encodeURIComponent(p)}`;
        return (
          <a href={src} target="_blank" rel="noreferrer" title={t`在预览中打开`}>
            <img src={src} alt={t`图片 ${p.split("/").pop() ?? ""}`} style={{ width: `${size}px`, height: `${size}px` }} />
          </a>
        );
      })}
    </div>
  );
}

/** One acceptance criterion with the check that answered it, as the service wrote it (`CheckLine`,
 * backend/src/run_view/checks.rs): passed or not, how it was checked, the evidence. */
export function CheckLine({ line }: { line: Json }) {
  const [whole, setWhole] = useState(false);
  const m = line.method ?? {};
  return (
    <div class="check-line" aria-label={line.voice}>
      <span class={"check-mark " + (line.mark === "passed" ? "fg-green" : line.mark === "failed" ? "fg-red" : "secondary")} aria-hidden="true">
        {line.mark === "passed" ? "✓" : line.mark === "failed" ? "✗" : "○"}
      </span>
      <div class="grow">
        <div class="check-head">
          <span class={`method-tag fg-${m.tone}`} title={t`证据方法：${m.text} · ${m.help}`}>
            <Icon name={m.icon} size={10} /> {m.text}
          </span>
          <span class="check-criterion">{line.criterion}</span>
        </div>
        {line.evidence ? (
          <>
            <div class={"caption check-evidence" + (whole ? "" : " clamp2")}>{line.evidence}</div>
            {line.folds && (
              <button type="button" class="link-btn caption" onClick={() => setWhole(!whole)}>
                {whole ? t`收起` : t`全文`}
              </button>
            )}
          </>
        ) : (
          line.noEvidence && <div class="caption fg-red">{line.noEvidence}</div>
        )}
      </div>
    </div>
  );
}

/** Lines of checks. */
export function CheckLines({ lines }: { lines: Json[] }) {
  return (
    <>
      {(lines ?? []).map((l) => (
        <CheckLine line={l} />
      ))}
    </>
  );
}

/** One event of a conversation, as the service worded it. */
export function ChatItem({ run, e, more = [], docked }: { run: RunModel; e: Json; more?: Json[]; docked: boolean }) {
  const v = run.view.value;
  const line: Json | undefined = v?.lines?.[e.seq];
  const d = e.data ?? {};
  if (!line) {
    if (e.type === "agent.message") return <TextRow events={[e, ...more]} reply />;
    if (e.type === "agent.thought") return <ThoughtLine text={d.text ?? ""} />;
    return null;
  }
  switch (line.kind) {
    case "divider":
      return (
        <div class="chat-section" data-seq={e.seq}>
          <div class="chat-divider">
            <span>{line.text}</span>
          </div>
          {line.sub && <div class="caption">{line.sub}</div>}
          {line.git && <div class="caption">{line.git}</div>}
          <StoredImages run={run} paths={line.images ?? []} size={56} />
        </div>
      );
    case "attempt":
      return (
        <div class="chat-attempt" data-seq={e.seq}>
          <hr />
          <div class="caption strong">{line.title}</div>
          {line.git && <div class="caption">{line.git}</div>}
          <Fold label={<span class="caption">{line.brief}</span>}>
            <CodeBlock text={d.prompt ?? ""} max={360} />
          </Fold>
        </div>
      );
    case "note":
      return (
        <div class={"chat-note" + (line.tone ? ` fg-${line.tone}` : " secondary")} data-seq={e.seq}>
          {line.text}
        </div>
      );
    case "prompt":
      return (
        <Fold
          class="chat-prompt"
          label={
            <span title={line.hint}>
              <span class="secondary strong">→ {line.label}</span> <span class="caption tertiary">{line.hint}</span>
            </span>
          }
        >
          <CodeBlock text={d.prompt ?? ""} max={360} />
        </Fold>
      );
    case "bubbles":
      return (
        <>
          {line.items.map((b: Json) => (
            <div class="bubble-row" data-seq={e.seq}>
              <div class={"bubble" + (b.insert ? " insert" : "")}>
                {b.insert && <div class="caption strong accent">↪ {b.via}</div>}
                <StoredImages run={run} paths={b.images ?? []} />
                {b.text && <div class="bubble-text">{b.text}</div>}
                {!b.insert && b.via && <div class="caption">{b.via}</div>}
              </div>
            </div>
          ))}
        </>
      );
    case "result":
      return (
        <div class="chat-result" data-seq={e.seq}>
          <Fold
            label={
              <span class="result-head">
                <span class="strong">{line.title}</span>
                {(line.pills ?? []).map((p: Json) => (
                  <Pill text={p.text} tone={p.tone} />
                ))}
              </span>
            }
          >
            <CheckLines lines={v?.checks?.[e.seq] ?? []} />
            {line.evidence && <div class="caption">{line.evidence}</div>}
            {line.risks && <div class="caption fg-amber">{line.risks}</div>}
          </Fold>
        </div>
      );
    case "request": {
      const card = line.open ? (v?.cards ?? []).find((c: Json) => c.seq === e.seq) : undefined;
      if (card && !docked) return <RequestCard run={run} card={card} />;
      return (
        <div class={"chat-note" + (card ? " fg-amber" : line.tone ? ` fg-${line.tone}` : " secondary")} data-seq={e.seq}>
          {card ? line.docked : line.text}
        </div>
      );
    }
    case "tool":
      return <ToolRow run={run} e={e} line={line} />;
    default:
      return null;
  }
}

/** One tool call, readable at a glance; its full input and output a click away. A running call
 * shows its newest frame from the stream: its title and output so far. */
export function ToolRow({ run, e, line }: { run: RunModel; e: Json; line: Json }) {
  const d = e.data ?? {};
  const cwd = run.view.value?.run?.cwd ?? "";
  const key = e.task ? `task:${e.task}` : `agent:${e.actor}`;
  const live: Json | undefined = line.running ? run.preview.value[key]?.tools?.[d.callId] : undefined;
  const output = inWorkspace(live?.output ?? d.output ?? "", cwd);
  const input = live ? live.input || live.title : d.input ? String(d.input) : (e.text ?? "");
  const verb = live?.verb ?? line.verb;
  const what = live?.what ?? line.what;
  const mono = live?.mono ?? line.mono;
  const full = line.board ? JSON.stringify(d.args ?? d.input ?? null, null, 2) : input;
  const diffs: Json[] = run.view.value?.diffs?.[e.seq] ?? [];
  const tail = output.split("\n");
  const cut = tail.length > 16;
  return (
    <div class="tool-row" data-seq={e.seq}>
      <Fold
        label={
          <span class="tool-label" title={input}>
            <span class="tool-line">
              {line.board ? (
                <>
                  <span class="tool-tag">{line.tag}</span>
                  <span class="ellipsis">{what}</span>
                </>
              ) : line.running ? (
                <Shimmer text={[verb, what].filter(Boolean).join(" ")} />
              ) : (
                <>
                  {verb && <span class={line.failed ? "fg-red" : "secondary"}>{verb}</span>}
                  <span class={"ellipsis" + (mono ? " mono" : "") + (line.failed ? " fg-red" : "")}>{what}</span>
                </>
              )}
              {line.waiting && <Pill text={t`等待返回`} tone="blue" />}
              {line.failed && <Pill text={t`失败`} tone="red" />}
              {line.files && <span class="caption">{line.files}</span>}
            </span>
            {line.summary && <span class="caption ellipsis tool-summary">{line.summary}</span>}
          </span>
        }
      >
        <CodeBlock text={full + (output ? `\n\n→ ${output}` : "")} max={300} />
        {diffs.map((x) => (
          <InlineDiff path={tilde(x.path)} lines={x.lines} />
        ))}
      </Fold>
      {line.running && output && (
        <div class="tool-tail">
          {cut && <div class="caption tertiary">{t`更早的输出没有显示`}</div>}
          <pre>{tail.slice(-16).join("\n")}</pre>
        </div>
      )}
    </div>
  );
}

/** The agent's thinking, quiet: its first lines, all of it a click away. */
export function ThoughtLine({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  return (
    <button type="button" class={"thought" + (open ? "" : " clamp2")} onClick={() => setOpen(!open)} title={open ? t`收起思考` : t`展开思考`} aria-label={t`思考`}>
      {text.trim()}
    </button>
  );
}

/** The agent's own words: the reply that ended a turn in full color, what it said along the way quieter. */
export function TextRow({ events, reply }: { events: Json[]; reply: boolean }) {
  // Pieces keep their own whitespace (code blocks may be cut anywhere): join them as they came.
  const words = events.map((e) => e.data?.text ?? "").join("").trim();
  if (!words) return null;
  return <Markdown text={words} class={"chat-text" + (reply ? "" : " secondary")} />;
}

/** The agent's line at the top of its turn: who, and what the turn did; a turn with work folds here. */
export function TurnHead({ run, row, onToggle }: { run: RunModel; row: Json; onToggle: () => void }) {
  const w = who(run, row.agent);
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!row.running) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [row.running]);
  const summary = row.running ? [t`正在工作 · ${dur(Math.max(0, now - row.start))}`, ...(row.stats ? [row.stats] : [])].join(" · ") : row.summary;
  return (
    <div class="turn-head">
      <div class="turn-who">
        <Avatar role={w.role} size={20} />
        <span class="caption ellipsis">{[w.name, w.model].filter(Boolean).join(" · ")}</span>
      </div>
      {row.open !== null && (
        <button type="button" class="fold-row" aria-expanded={row.open} aria-label={row.open ? t`收起这一轮的工作过程` : t`展开这一轮的工作过程`} onClick={onToggle}>
          <Chevron open={row.open} />
          {row.running ? <Shimmer text={summary} /> : <span class="secondary ellipsis">{summary}</span>}
        </button>
      )}
    </div>
  );
}

/** Calls and thinking in a row: more than one shows as a line of what they did that opens to every call. */
export function ActivityRow({ run, row, onToggle }: { run: RunModel; row: Json; onToggle: () => void }) {
  const line = (e: Json) =>
    e.type === "agent.thought" ? <ThoughtLine text={e.data?.text ?? ""} /> : <ChatItem run={run} e={e} docked={false} />;
  if (row.events.length === 1) return line(row.events[0]);
  if (row.open === null) return <div class="activity">{row.events.map(line)}</div>;
  return (
    <div class="activity">
      <button type="button" class="fold-row" aria-expanded={row.open} onClick={onToggle}>
        <Chevron open={row.open} />
        <span class="secondary ellipsis">{row.summary}</span>
      </button>
      {row.open && <div class="activity-body">{row.events.map(line)}</div>}
    </div>
  );
}

/** `+42 −10` in a diff's colors. */
export function Counts({ added, removed }: { added: number; removed: number }) {
  return (
    <span class="counts">
      <span class="fg-green">+{added}</span> <span class="fg-red">−{removed}</span>
    </span>
  );
}

/** The files a turn changed, under its reply: each one's lines added and removed, the change a click away. */
export function EditsCard({ run, files }: { run: RunModel; files: Json[] }) {
  const [shown, setShown] = useState<Set<string>>(new Set());
  const [all, setAll] = useState(false);
  const cwd = run.view.value?.run?.cwd ?? "";
  const diffs = run.view.value?.diffs ?? {};
  const added = files.reduce((n, f) => n + (f.added ?? 0), 0);
  const removed = files.reduce((n, f) => n + (f.removed ?? 0), 0);
  return (
    <div class="edits-card">
      <div class="edits-head">
        <span class="strong">{t`改了 ${files.length} 个文件`}</span>
        <Counts added={added} removed={removed} />
      </div>
      {(all ? files : files.slice(0, 4)).map((f) => {
        const rel = inWorkspace(f.path, cwd);
        const i = rel.lastIndexOf("/");
        const open = shown.has(f.path);
        return (
          <div class="edits-file">
            <button
              type="button"
              class="edits-row"
              title={tilde(f.path)}
              aria-expanded={open}
              onClick={() => {
                const next = new Set(shown);
                if (open) next.delete(f.path);
                else next.add(f.path);
                setShown(next);
              }}
            >
              <span class="mono ellipsis">{rel.slice(i + 1)}</span>
              <span class="caption ellipsis grow">{i > 0 ? rel.slice(0, i) : ""}</span>
              <Counts added={f.added ?? 0} removed={f.removed ?? 0} />
            </button>
            {open &&
              (f.changes ?? []).map((c: Json) => {
                const list = diffs[c.seq];
                return list && list[c.index] ? <InlineDiff path={tilde(f.path)} lines={list[c.index].lines} /> : null;
              })}
          </div>
        );
      })}
      {files.length > 4 && !all && (
        <button type="button" class="edits-more caption" onClick={() => setAll(true)}>
          {t`还有 ${files.length - 4} 个`}
        </button>
      )}
    </div>
  );
}

/** What a session is writing or thinking right now, before it is on record. */
export function LiveRow({ preview }: { preview: Json }) {
  const thought = String(preview.thought ?? "").trim();
  const text = String(preview.text ?? "");
  if (!text && !thought) return null;
  return (
    <div class="live-row">
      {thought && !text && <div class="thought clamp3">{thought.split("\n").slice(-3).join("\n")}</div>}
      {text && <Markdown text={text} class="chat-text" />}
    </div>
  );
}
