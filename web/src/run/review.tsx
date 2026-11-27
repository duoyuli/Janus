// The delivery on the run page, reviewed one round at a time (Mac: UI/Run/ReviewCard.swift and the
// delivery rows of UI/Run/Evidence.swift). While it waits for the person the card is the whole
// review; at any other stage it folds to the one line the service writes (`page.reviewLine`).
// Stages, signals, numbers and lines are the service's.

import { useState } from "preact/hooks";
import type { Json } from "../lib/api.ts";
import { t } from "../lib/i18n.ts";
import { go, link } from "../lib/router.ts";
import { Button, Card, Disclosure, SectionHeader } from "../ui/kit.tsx";
import { Icon } from "../ui/Icon.tsx";
import { Inline, Markdown } from "../session/Markdown.tsx";
import type { RunModel } from "../session/model.ts";
import { CheckLines } from "../session/Items.tsx";
import { briefDraft } from "../pages/workspace-edit.tsx";
import { diffLink, docLink, runLink, taskLink } from "./nav.ts";
import { EvidenceReminders, ListNote, RiskList, SeqButton, StrengthBar, TaskTag, VerificationList } from "./parts.tsx";
import { leadFocus } from "./board.tsx";

/** Open rows of the delivery's acceptance, by task: a row without an entry uses its default. */
const taskOpen = new Map<string, boolean>();

export function ResultCard({ run, st }: { run: RunModel; st: Json }) {
  const [open, setOpen] = useState(false);
  const r = st.deliveries?.at(-1);
  const stage = st.review?.stage;
  if (!r || !stage) return null;
  const reviews = stage === "awaiting" || stage === "blocked";
  return (
    <>
      <SectionHeader title={t`交付`} />
      {reviews ? (
        <Card class={stage === "awaiting" ? "tint-green" : "tint-amber"}>
          <ReviewBody run={run} st={st} r={r} stage={stage} />
        </Card>
      ) : (
        <Card pad={14}>
          <div class="review-line">
            <RoundTag n={r.n} />
            <span class={"callout grow" + (stage === "held" ? " fg-amber" : "")}>{st.page?.reviewLine ?? ""}</span>
            <button type="button" class="link callout" onClick={() => setOpen(!open)}>{open ? t`收起` : t`看这次交付`}</button>
          </div>
          {stage === "held" && run.canAct.value && (
            <div class="flow">
              <Button kind="primary" title={t`交还负责人，它接着做你交代的事`} onClick={() => void run.act({ type: "release", task: "", agent: "lead" })}>{t`让它接着做`}</Button>
              <Button title={t`这一轮做到哪里就交付到哪里，交付时合并`} onClick={() => void run.act({ type: "finish", task: "", agent: "lead" })}>{t`按现在的样子交付`}</Button>
            </div>
          )}
          {open && (
            <>
              <hr />
              <ReviewBody run={run} st={st} r={r} stage={stage} />
            </>
          )}
        </Card>
      )}
    </>
  );
}

/** 「第 2 次交付」. */
export function RoundTag({ n }: { n: number }) {
  return <span class="round-tag caption">{t`第 ${n} 次交付`}</span>;
}

function ReviewBody({ run, st, r, stage }: { run: RunModel; st: Json; r: Json; stage: string }) {
  const [whole, setWhole] = useState(false);
  const d = r.d ?? {};
  const delivery = st.delivery ?? { facts: "", tasks: [] };
  const signals: Json[] = st.review?.signals ?? [];
  const asked: Json[] = st.review?.requests ?? [];
  const many = (st.deliveries?.length ?? 0) > 1;
  const headline: string = d.headline ?? "";
  const summary: string = d.summary ?? "";
  const changes = st.page?.changes ?? { diff: null, others: [] };
  const changed: string[] = d.changed ?? [];
  const waits = d.merge?.state === "conflict" || d.merge?.state === "blocked";
  // Not only while the run is live: a run delivered before the service restarted is accepted
  // from its record, and a message to its lead reopens the session.
  const offers = (stage === "awaiting" || stage === "blocked") && !run.replaying.value;
  const earlier: Json[] = st.page?.earlier ?? [];
  const evidence: Json[] = d.evidence ?? [];
  return (
    <div class="review-body">
      <div class="row-inline">
        {many && <RoundTag n={r.n} />}
        <SeqButton run={run} seq={r.seq} />
      </div>
      {asked.length > 0 && (
        <div class="quote-bar">
          <div class="caption secondary">{t`你在交付后的要求`}</div>
          {asked.map((m) => <div class="callout clamp-4">{m.text}</div>)}
        </div>
      )}
      {headline ? (
        <div class="row-inline baseline">
          <span class="lead-tag caption secondary">{t`负责人`}</span>
          <span class="title3"><Inline text={headline} /></span>
        </div>
      ) : summary ? (
        summary.length > 360 && !whole ? (
          <>
            <div class="clamp-8"><Inline text={summary} /></div>
            <button type="button" class="link callout" onClick={() => setWhole(true)}>{t`展开全文`}</button>
          </>
        ) : (
          <>
            <Markdown text={summary} />
            {summary.length > 360 && <button type="button" class="link callout" onClick={() => setWhole(false)}>{t`收起`}</button>}
          </>
        )
      ) : null}
      <div class="callout secondary" data-facts>{delivery.facts}</div>
      <div class="attention">
        <div class="row-inline">
          <span class="callout strong">{t`需要你看的`}</span>
          {signals.length > 0 && <span class="callout strong fg-amber">{signals.length}</span>}
        </div>
        {signals.length === 0 && <div class="callout secondary">{t`没有。负责人没留下要你决定的事，Janus 也没查出问题。`}</div>}
        {signals.map((s) => (
          <div class="signal">
            <span class={s.source === "lead" ? "fg-blue" : "fg-amber"} aria-hidden="true">{s.source === "lead" ? "?" : "⚠"}</span>
            <div class="grow">
              <div class="callout"><Inline text={s.text} /></div>
              {s.detail && <div class="caption secondary">{s.detail}</div>}
            </div>
            <span class={"source-tag caption " + (s.source === "lead" ? "fg-blue" : "fg-violet")} title={s.source === "lead" ? t`负责人留给你的问题` : t`Janus 从记录里查出来的`}>
              {s.source === "lead" ? t`负责人` : "Janus"}
            </span>
            {s.task && (st.tasks ?? []).some((x: Json) => x.id === s.task) && <a class="link caption" {...link(taskLink(run.id, s.task))}>{t`查看`}</a>}
          </div>
        ))}
      </div>
      <div class="changes">
        <div class="callout strong">{many ? t`这一轮改了什么` : t`改了什么`}</div>
        {d.merge?.note && <div class={"callout " + (waits ? "fg-amber" : "secondary")}>{d.merge.note}</div>}
        {changed.length > 0 && (
          <div class="flow" title={changed.join("\n")}>
            {changed.slice(0, 5).map((f) => <span class="file-chip mono caption">{f}</span>)}
            {changed.length > 5 && <span class="caption secondary">{t`还有 ${changed.length - 5} 个`}</span>}
            {changes.diff && <a class="link caption" {...link(diffLink(run.id, changes.diff.path))}>{t`审阅改动`}</a>}
          </div>
        )}
        {(changes.others ?? []).map((a: Json) => (
          <div class="row-inline">
            <span class="callout strong">{a.name}</span>
            {a.desc && <span class="caption secondary ellipsis">{a.desc}</span>}
            <a class="link callout" {...link(docLink(run.id, a.path))}>{t`查看`}</a>
            <SeqButton run={run} seq={a.seq} />
          </div>
        ))}
      </div>
      {offers && (
        <div class="review-actions">
          {stage === "blocked" && st.review?.conflict ? (
            <Button kind="primary" title={t`把冲突交给负责人，它在任务的副本里解决后再次交付`} onClick={() => void run.act({ type: "accept" })}>{t`让负责人再解决`}</Button>
          ) : stage === "blocked" ? (
            <Button kind="primary" title={t`你那边挡路的东西处理掉之后，验收时把结果合并到你的分支`} onClick={() => void run.act({ type: "accept" })}>{t`处理好了，合并并验收`}</Button>
          ) : (
            <Button kind="primary" data-action="accept" onClick={() => void run.act({ type: "accept" })}>{signals.length ? t`看过了，验收` : t`验收`}</Button>
          )}
          <Button title={t`在下面负责人的对话里写下要改什么；它改完会再交付一次`} onClick={() => { leadFocus.value++; }}>{t`让负责人改…`}</Button>
          <span class="caption secondary grow end-text">{t`下面是细节，不用看完也能验收`}</span>
        </div>
      )}
      {(delivery.tasks ?? []).length > 0 && (
        <div class="acceptance">
          <div class="callout strong">{many ? t`这一轮的子任务` : t`子任务`}</div>
          {delivery.tasks.map((row: Json) => <DeliveryTaskRow run={run} row={row} />)}
        </div>
      )}
      {headline && summary && <Folded title={t`负责人的完整报告`}><Markdown text={summary} /></Folded>}
      {evidence.length > 0 && (
        <Folded title={t`其他证据 ${evidence.length} 条`}>
          {evidence.map((e) => (
            <div class="row-inline baseline">
              <Tagged text={e.text ?? ""} />
              <SeqButton run={run} seq={e.seq} />
            </div>
          ))}
        </Folded>
      )}
      {earlier.length > 0 && (
        <Folded title={t`之前的交付 ${earlier.length} 次`}>
          {earlier.map((e) => (
            <div class="row-inline baseline">
              <RoundTag n={e.n} />
              <span class="callout clamp-2 grow"><Inline text={e.line} /></span>
              {e.accepted && <span class="caption fg-green">{t`已验收`}</span>}
              <SeqButton run={run} seq={e.seq} />
            </div>
          ))}
        </Folded>
      )}
      {(d.briefSuggestions ?? []).length > 0 && <BriefSuggestions run={run} lines={d.briefSuggestions} />}
      <a class="link callout" {...link(runLink(run.id, "audit"))}>{t`在审计日志中逐条核对`}</a>
    </div>
  );
}

/** A disclosure that keeps its own state. */
export function Folded({ title, children, start = false }: { title: string; children: preact.ComponentChildren; start?: boolean }) {
  const [open, setOpen] = useState(start);
  return (
    <Disclosure title={title} open={open} onToggle={setOpen}>
      {children}
    </Disclosure>
  );
}

const TASK_ID = /^[A-Za-z][\w-]*$/;

/** 「T1：text」 from the orchestrator: the task id as a tag. */
function Tagged({ text }: { text: string }) {
  const sep = t`：`;
  const i = text.indexOf(sep);
  const head = i > 0 ? text.slice(0, i) : "";
  const tag = TASK_ID.test(head);
  return (
    <span class="row-inline baseline">
      {tag && <TaskTag id={head} />}
      <span class="callout"><Inline text={tag ? text.slice(i + sep.length) : text} /></span>
    </span>
  );
}

/** One task's acceptance in the delivery under review. */
function DeliveryTaskRow({ run, row }: { run: RunModel; row: Json }) {
  const [, redraw] = useState(0);
  const open = taskOpen.get(row.task) ?? !!row.needsAttention;
  const toggle = () => {
    taskOpen.set(row.task, !open);
    redraw((n) => n + 1);
  };
  return (
    <div class="delivery-row" data-task={row.task}>
      <div class="delivery-row-head">
        <TaskTag id={row.task} />
        <span class="callout strong ellipsis">{row.title}</span>
        <span class="caption secondary nowrap">{row.metrics}</span>
        <StrengthBar strength={row.strength} />
        {(row.markers ?? []).length > 0 && <span class="caption fg-amber">{row.markers.join(" · ")}</span>}
        <span class="grow" />
        <button type="button" class="link caption" aria-expanded={open} title={open ? t`收起这个子任务的验收证据` : t`展开这个子任务的验收标准、证据、独立验证、痕迹和风险`} onClick={toggle}>
          <Icon name={open ? "chevron.down" : "chevron.right"} size={10} /> {open ? t`收起` : t`展开`}
        </button>
      </div>
      {open && (
        <div class="delivery-row-body">
          <div class="callout strong">{row.title}</div>
          <div class="caption secondary">{t`证据：${row.counts}`}</div>
          <CheckLines lines={row.lines ?? []} />
          <EvidenceReminders reminders={row.reminders ?? []} />
          {(row.verifications ?? []).length > 0 && (
            <>
              <hr />
              <div class="caption strong secondary">{t`独立验证`}</div>
              <VerificationList records={row.verifications} />
            </>
          )}
          <ListNote items={row.traceGaps ?? []} tone="amber" format={(l) => t`改动核对（结果和工作目录对不上）：\n${l}`} />
          {(row.risks ?? []).length > 0 && (
            <div>
              <div class="caption strong secondary">{t`执行者自述的风险`}</div>
              <RiskList risks={row.risks} small />
            </div>
          )}
          <a class="link caption" {...link(taskLink(run.id, row.task))}>{t`打开子任务`}</a>
        </div>
      )}
    </div>
  );
}

/** What the lead suggests keeping in the workspace's shared material; written only when the
 * person saves it on the shared material's page, which opens with the lines added (Mac:
 * `BriefSuggestions`). */
function BriefSuggestions({ run, lines }: { run: RunModel; lines: string[] }) {
  const ws = run.run.value?.config?.workspace?.id;
  return (
    <div class="brief-suggestions">
      <div class="callout strong">{t`负责人建议写入共享资料`}</div>
      {lines.map((l) => <div class="callout">· {l}</div>)}
      <div class="row-inline">
        <span class="caption secondary grow">{t`采纳后，这个工作区之后的任务都能看到。`}</span>
        {ws ? (
          <Button small disabled={run.replaying.value} title={t`打开共享资料，把这些内容加到末尾，确认后保存`} onClick={() => { briefDraft.value = { workspace: ws, lines }; go(`/workspaces/${encodeURIComponent(ws)}/brief`); }}>
            {t`加入共享资料…`}
          </Button>
        ) : (
          <span class="caption secondary">{t`这个工作区已经删除`}</span>
        )}
      </div>
    </div>
  );
}
