// The run page's small pieces, each the counterpart of one in the Mac app (UI/Run/*.swift,
// UI/Components.swift): a task's tag, an event's `#seq` that opens it in the audit log, a labelled
// list, a code block, the evidence of a check, and the names and colors the service's states and
// event kinds are drawn with. Names of states, not judgments: the states are the service's.

import type { ComponentChildren } from "preact";
import type { Json } from "../lib/api.ts";
import { t } from "../lib/i18n.ts";
import type { Tone } from "../lib/status.ts";
import { Icon } from "../ui/Icon.tsx";
import { Pill } from "../ui/kit.tsx";
import { Inline } from "../session/Markdown.tsx";
import type { RunModel } from "../session/model.ts";
import { CheckLines } from "../session/Items.tsx";
import { runLink } from "./nav.ts";
import { link } from "../lib/router.ts";
import { meta } from "../lib/store.ts";
import { pattern } from "../lib/format.ts";

/** A subtask's id as a small tag (Mac: `TaskTag`). */
export function TaskTag({ id }: { id: string }) {
  return <span class="task-tag mono">{id}</span>;
}

/** `#12`: opens event 12 in the audit log (Mac: `SeqButton`). */
export function SeqButton({ run, seq }: { run: RunModel; seq?: number | null }) {
  if (!seq) return null;
  return (
    <a class="seq mono" title={t`在审计日志中查看这条事件`} aria-label={t`事件 ${seq}，在审计日志中查看`} {...link(runLink(run.id, `audit/${seq}`))}>
      #{seq}
    </a>
  );
}

/** Labelled rows (Mac: `KV`). */
export function KV({ rows, mono }: { rows: [string, ComponentChildren][]; mono?: boolean }) {
  return (
    <dl class="kv">
      {rows.map(([k, v]) => (
        <>
          <dt class="secondary">{k}</dt>
          <dd class={mono ? "mono" : ""}>{v}</dd>
        </>
      ))}
    </dl>
  );
}

/** A section of a card: a quiet heading and what is under it. */
export function Sub({ title, children }: { title: string; children: ComponentChildren }) {
  return (
    <div class="sub-block">
      <div class="callout secondary">{title}</div>
      {children}
    </div>
  );
}

/** A time of day to the second (Mac: `L.clock`). */
export function clock(ms: number): string {
  return pattern("HH:mm:ss", new Date(ms));
}

/** A string cut to `n` characters with an ellipsis (Mac: `L.short`). */
export function short(text: string, n = 26): string {
  const chars = [...(text ?? "")];
  return chars.length > n ? chars.slice(0, n).join("") + "…" : text ?? "";
}

// MARK: States

export function taskLabel(status: string): [string, Tone] {
  const map: Record<string, [string, Tone]> = {
    pending: [t`等上游`, "gray"],
    ready: [t`待开工`, "gray"],
    assigned: [t`已派活`, "blue"],
    running: [t`干活中`, "blue"],
    waiting_input: [t`等你回答`, "amber"],
    waiting_approval: [t`等你审批`, "amber"],
    blocked: [t`上游出错`, "amber"],
    skipped_by_decision: [t`负责人取消`, "gray"],
    verifying: [t`核对中`, "violet"],
    verification_failed: [t`验收没通过`, "red"],
    succeeded: [t`已完成`, "green"],
    failed: [t`失败`, "red"],
    cancelled: [t`已取消`, "gray"],
  };
  return map[status] ?? [t`未知`, "gray"];
}

export function attemptLabel(status: string): [string, Tone] {
  if (status === "succeeded") return [t`成功`, "green"];
  if (status === "failed") return [t`失败`, "red"];
  return [t`进行中`, "blue"];
}

export function phaseLabel(phase: string): [string, Tone] {
  const map: Record<string, [string, Tone]> = {
    idle: [t`摸鱼中`, "gray"],
    thinking: [t`思考中`, "blue"],
    coordinating: [t`协调中`, "blue"],
    reading_files: [t`翻文件`, "blue"],
    editing: [t`改文件`, "blue"],
    running_tool: [t`用工具`, "blue"],
    running_test: [t`跑测试`, "blue"],
    waiting_for_input: [t`等你输入`, "amber"],
    waiting_for_agent: [t`等队友`, "amber"],
    waiting_approval: [t`等你审批`, "amber"],
    waiting_for_seat: [t`等空位`, "amber"],
    error: [t`出错了`, "red"],
  };
  return map[phase] ?? [t`摸鱼中`, "gray"];
}

/** The glyph a status tone is drawn with in lists (Mac: `RunTimeline.statusSymbol`). */
export function statusSymbol(tone: Tone): string {
  switch (tone) {
    case "green":
      return "checkmark.circle.fill";
    case "red":
      return "xmark.circle.fill";
    case "amber":
      return "exclamationmark.circle.fill";
    case "blue":
    case "violet":
      return "circle.lefthalf.filled";
    default:
      return "circle.dashed";
  }
}

export function StatusGlyph({ tone, label }: { tone: Tone; label: string }) {
  return (
    <span class={`fg-${tone} status-glyph`}>
      <Icon name={statusSymbol(tone)} size={14} label={label} />
    </span>
  );
}

// MARK: Event marks (the service names each event kind's symbol: `mark` of a trace row)

const GLYPH: Record<string, string> = {
  created: "✦", stop: "■", pause: "Ⅱ", start: "▶", next: "▸", assign: "↗", fail: "✕", retry: "↻", reopen: "↺", submit: "⇪",
  done: "✓", artifact: "▤", handoff: "⇄", question: "?", escalate: "⇡", write: "✎", reply: "↩", flag: "⚑", delivered: "★",
  blocked: "⊘", forward: "→", waiting: "◌", tool: "⌘", route: "↪", quote: "❝", hand: "✋", interrupt: "‖", warning: "⚠",
  stall: "⏳", split: "✂", dot: "·",
};

export function EventIcon({ mark }: { mark: { icon: string; tone: Tone | null } }) {
  return <span class={`event-icon${mark.tone ? ` tone-bg-${mark.tone}` : ""}`} aria-hidden="true">{GLYPH[mark.icon] ?? "·"}</span>;
}

/** The audit filter an event kind shows under, as the service says (`auditCategories` of
 * `GET /api/meta`); a kind it does not know shows under 「子任务」. */
export function category(type: string): string {
  return meta.value?.auditCategories?.[type] ?? "tasks";
}

/** The audit filter's choices. */
export function categories(): [string, string][] {
  return [["all", t`全部`], ["tasks", t`子任务`], ["messages", t`消息`], ["artifacts", t`产物`], ["decisions", t`决策`], ["progress", t`进度与工具`]];
}

// MARK: Evidence (Mac: UI/Run/Evidence.swift)

/** A note of lines, `format` given the lines as 「· a\n· b」 (the Mac app's keys say 「…：\n%@」). */
export function ListNote({ items, tone, format }: { items: string[]; tone: Tone; format: (lines: string) => string }) {
  if (!items.length) return null;
  return (
    <div class={`note tone-border-${tone}`}>
      <span class={`dot tone-${tone}`} aria-hidden="true" />
      <span class="pre-line">{format(items.map((x) => `· ${x}`).join("\n"))}</span>
    </div>
  );
}

export function EvidenceReminders({ reminders }: { reminders: string[] }) {
  return <ListNote items={reminders} tone="amber" format={(l) => t`提醒（不影响是否通过）：\n${l}`} />;
}

export function verdictLabel(v: string): string {
  return v === "approved" ? t`通过` : v === "rejected" ? t`需要修改` : t`证据不足`;
}

export function VerificationList({ records }: { records: Json[] }) {
  return (
    <>
      {records.map((v) => {
        const label = taskLabel(v.status)[0] ?? v.label;
        return (
          <div class="verification">
            <div class="row-inline">
              <span class="mono strong caption">{v.task}</span>
              <Pill text={label} tone={v.passed ? "green" : "amber"} />
              {v.verdict && <Pill text={t`判定：${verdictLabel(v.verdict)}`} tone={v.verdict === "approved" ? "green" : v.verdict === "rejected" ? "red" : "amber"} />}
              <span class={"caption " + (v.passed ? "secondary" : "fg-amber")}>{v.passed ? t`已确认这个子任务的结论` : t`结论没有得到确认`}</span>
            </div>
            {v.summary && (
              <div class="callout">
                <Inline text={v.summary} />
              </div>
            )}
            <CheckLines lines={v.lines ?? []} />
            {(v.evidence ?? []).map((e: string) => (
              <div class="caption secondary">
                <Inline text={e} />
              </div>
            ))}
            <RiskList risks={v.risks ?? []} small />
          </div>
        );
      })}
    </>
  );
}

export function RiskList({ risks, small }: { risks: string[]; small?: boolean }) {
  return (
    <>
      {risks.map((r) => (
        <div class={"risk " + (small ? "caption" : "callout")}>
          <span class="fg-amber" aria-hidden="true">⚠</span>
          <Inline text={r} />
        </div>
      ))}
    </>
  );
}

export function StrengthBar({ strength }: { strength: Json }) {
  const total = (strength?.ran ?? 0) + (strength?.read ?? 0) + (strength?.said ?? 0);
  if (!total) return null;
  const pct = (n: number) => `${(100 * n) / total}%`;
  return (
    <span class="strength" title={t`证据：${strength.text}`} role="img" aria-label={t`证据：${strength.text}`}>
      <span class="tone-green" style={{ width: pct(strength.ran) }} />
      <span class="tone-blue" style={{ width: pct(strength.read) }} />
      <span class="said" style={{ width: pct(strength.said) }} />
    </span>
  );
}
