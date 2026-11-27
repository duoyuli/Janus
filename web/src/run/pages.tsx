// A run's other pages (Mac: UI/Run/TaskView.swift, AgentView.swift, AuditView.swift): a subtask
// with its conversation, criteria and evidence, trace and setting; an agent with its conversation
// and subtasks; the audit log with the system's rules and replay; and the team snapshot. The
// trace, the downstream tasks, the setting and the notes are the service's (`page`); the audit
// filters only pick among the events read.

import { useEffect, useRef, useState } from "preact/hooks";
import type { Json } from "../lib/api.ts";
import { dateTime, tilde } from "../lib/format.ts";
import { t } from "../lib/i18n.ts";
import { go, link } from "../lib/router.ts";
import { useDrawn } from "../lib/testing.ts";
import { Avatar, Card, Note, Pill, useEdgeFade } from "../ui/kit.tsx";
import { Icon } from "../ui/Icon.tsx";
import { Inline } from "../session/Markdown.tsx";
import type { RunModel } from "../session/model.ts";
import { who } from "../session/Items.tsx";
import { diffLink, docLink, runLink, taskLink } from "./nav.ts";
import { categories, category, clock, EventIcon, EvidenceReminders, KV, ListNote, phaseLabel, RiskList, SeqButton, short, TaskTag, taskLabel, attemptLabel, VerificationList } from "./parts.tsx";
import { Folded } from "./review.tsx";
import { SessionPanel } from "../session/SessionPanel.tsx";
import { CheckLine, CheckLines, CodeBlock } from "../session/Items.tsx";

function back(run: RunModel) {
  return <a class="link callout back-link" {...link(runLink(run.id))}><Icon name="chevron.left" size={12} /> {t`返回工作台`}</a>;
}

export function TaskPage({ run, id }: { run: RunModel; id: string }) {
  const st = run.shown.value;
  const x = (st?.tasks ?? []).find((y: Json) => y.id === id);
  useDrawn("task", () => (x ? { id, status: x.status, trace: (st.page?.tasks?.[id]?.trace ?? []).map((r: Json) => r.seq), downstream: st.page?.tasks?.[id]?.downstream ?? [], criteria: st.evidence?.[id]?.criteria ?? 0, passed: st.evidence?.[id]?.passed ?? 0 } : { id, missing: true }));
  if (!st) return null;
  if (!x) {
    return (
      <div class="empty-state">
        <div class="secondary">{run.replaying.value ? t`在当前回放位置，这个子任务还不存在。` : t`这个任务里没有 ${id}。`}</div>
        {back(run)}
      </div>
    );
  }
  const [label, tone] = taskLabel(x.status);
  const page = st.page?.tasks?.[id] ?? { trace: [], downstream: [], structure: "", env: [] };
  const trace: Json[] = page.trace ?? [];
  const w = who(run, x.agent);
  const summary = st.evidence?.[id] ?? { criteria: 0, passed: 0, counts: "", reminders: [], rows: [], verifications: [] };
  const session = st.sessions?.[`task:${id}`] ?? {};
  const title = (d: string) => (st.tasks ?? []).find((y: Json) => y.id === d)?.title ?? "";
  const deps = (label: string, ids: string[]): [string, preact.ComponentChildren] => [
    label,
    ids.length ? <span class="stack">{ids.map((d) => <a class="link" {...link(taskLink(run.id, d))}>{`${d} ${title(d)}`}</a>)}</span> : <span class="secondary">{t`无`}</span>,
  ];
  return (
    <div class="task-page">
      {back(run)}
      <div class="task-head">
        <div class="grow min0">
          <div class="row-inline">
            <TaskTag id={x.id} />
            <Pill text={label} tone={tone} />
            {x.attempts.length > 1 && <Pill text={t`第 ${x.attempts.length} 次尝试`} />}
          </div>
          <h2 class="display">{x.title}</h2>
          {x.desc && <div class="secondary">{x.desc}</div>}
          {page.structure && <div class="caption secondary">{page.structure}</div>}
          {session.git && <div class="caption secondary">{session.git}</div>}
        </div>
        <a class="btn btn-bordered" {...link(runLink(run.id, "audit"))} onClick={(e) => { e.preventDefault(); auditFilter.task = x.id; go(runLink(run.id, "audit")); }}>{t`在审计日志中查看`}</a>
      </div>
      <div class="split">
        <div class="split-main">
          <SessionPanel run={run} sessionKey={`task:${id}`} title={t`会话`} />
          {trace.length > 0 && (
            <Card>
              <Folded title={`${t`执行轨迹`} · ${t`${trace.length} 条事件`}`}>
                <div class="trace">{trace.map((r) => <TraceRow run={run} row={r} />)}</div>
              </Folded>
            </Card>
          )}
        </div>
        <div class="split-side">
          {((x.accept ?? []).length > 0 || (x.checks ?? []).length > 0 || x.summary) && (
            <Card>
              <div class="row-inline">
                <span class="card-title grow">{t`验收标准`}</span>
                {summary.criteria > 0 && <span class="caption secondary">{t`${summary.criteria} 条 · 通过 ${summary.passed} 条`}</span>}
              </div>
              <ListNote items={x.traceGaps ?? []} tone="amber" format={(l) => t`和工作目录对不上：\n${l}`} />
              {(summary.rows ?? []).map((r: Json) => <CheckLine line={r.line} />)}
              {(x.checks ?? []).length > 0 && <div class="caption secondary">{t`证据：${summary.counts}`}</div>}
              <EvidenceReminders reminders={summary.reminders ?? []} />
              <ListNote items={x.verificationFailures ?? []} tone="red" format={(l) => t`最近一次核对没有通过：\n${l}`} />
              {(x.evidence ?? []).length > 0 && (
                <>
                  <hr />
                  <div class="callout secondary">{t`协作者提交的其他证据`}</div>
                  {x.evidence.map((e: string) => <div class="callout"><Inline text={e} /></div>)}
                </>
              )}
              {(summary.verifications ?? []).length > 0 && (
                <>
                  <hr />
                  <div class="callout secondary">{t`独立验证`}</div>
                  <VerificationList records={summary.verifications} />
                </>
              )}
              {(x.risks ?? []).length > 0 && (
                <>
                  <hr />
                  <div class="callout secondary">{t`风险（协作者自报）`}</div>
                  <RiskList risks={x.risks} />
                </>
              )}
              {x.summary && (
                <>
                  <hr />
                  <div class="callout secondary">{t`结果`}</div>
                  <div class="callout"><Inline text={x.summary} /></div>
                </>
              )}
            </Card>
          )}
          <Card>
            <KV rows={[[t`协作者`, w.name], deps(t`上游`, x.deps ?? []), deps(t`下游`, page.downstream ?? [])]} />
            {x.attempts.length > 0 && (
              <>
                <hr />
                <div class="callout secondary">{t`尝试`}</div>
                {x.attempts.map((a: Json) => {
                  const [al, at] = attemptLabel(a.status);
                  return (
                    <div class="attempt" title={a.session}>
                      <div class="row-inline">
                        <b>#{a.n}</b>
                        <Pill text={al} tone={at} />
                        <span class="grow" />
                        <span class="caption secondary">{clock(a.start)}</span>
                      </div>
                      {a.error && <Note text={a.error} tone="red" />}
                    </div>
                  );
                })}
              </>
            )}
            <hr />
            <Folded title={t`运行环境`}>
              <KV rows={(page.env ?? []).map((l: Json) => [l.label, l.value] as [string, string])} />
            </Folded>
          </Card>
          {(x.artifacts ?? []).length > 0 && (
            <Card>
              <div class="row-inline">
                <span class="card-title">{t`产物`}</span>
                <span class="caption secondary">{x.artifacts.length}</span>
              </div>
              {x.artifacts.map((a: Json) => {
                const diff = a.kind === "git_diff" || a.kind === "run_diff";
                return (
                  <div class="artifact" title={t`${tilde(a.path)} · 尝试 ${a.attempt} · sha256 ${a.hash}…`}>
                    <div class="row-inline">
                      <span class="strong grow">{a.name}</span>
                      <a class="link" {...link(diff ? diffLink(run.id, a.path) : docLink(run.id, a.path))}>{diff ? t`审阅` : t`查看`}</a>
                    </div>
                    {a.desc && <div class="caption secondary">{a.desc}</div>}
                  </div>
                );
              })}
            </Card>
          )}
        </div>
      </div>
    </div>
  );
}

/** One row of a task's trace, as the service wrote it (Mac: `TimelineRow`). */
function TraceRow({ run, row }: { run: RunModel; row: Json }) {
  const a = row.attempt;
  if (a) {
    return (
      <div class="trace-attempt callout">
        <b>{a.title}</b>
        <span class="secondary">{a.runtime}</span>
        <span class="mono caption secondary">{a.session}</span>
        <span class="caption secondary ellipsis grow" title={a.place}>{a.place}</span>
        <SeqButton run={run} seq={row.seq} />
      </div>
    );
  }
  const d = row.detail;
  return (
    <div class="trace-row">
      <EventIcon mark={row.mark} />
      <div class="grow min0">
        <div class="callout"><b>{row.who}</b>{"　" + (row.text ?? "")}</div>
        {d?.kind === "call" && <div class="mono caption secondary pre-line">{d.text}</div>}
        {d?.kind === "note" && <Note text={d.text} tone={d.tone} />}
        {d?.kind === "code" && <CodeBlock text={d.text} />}
        {d?.kind === "checks" && (
          <>
            <CheckLines lines={d.checks ?? []} />
            {d.text && <Note text={d.text} tone="amber" />}
          </>
        )}
      </div>
      <span class="caption secondary">{clock(row.ts)}</span>
      <SeqButton run={run} seq={row.seq} />
    </div>
  );
}

export function AgentPage({ run, id }: { run: RunModel; id: string }) {
  const st = run.shown.value;
  const w = who(run, id);
  const tasks: Json[] = (st?.tasks ?? []).filter((x: Json) => x.agent === id);
  useDrawn("agent", () => ({ id, tasks: tasks.map((x) => x.id) }));
  if (!st) return null;
  return (
    <div class="agent-page">
      {back(run)}
      <div class="row-inline">
        <Avatar role={w.role} size={34} />
        <div>
          <h2 class="display">{w.name}</h2>
          <div class="secondary">{[w.runtime, w.model].filter(Boolean).join(" · ")}</div>
        </div>
      </div>
      <div class="split">
        <div class="split-main">
          <SessionPanel run={run} sessionKey={`agent:${id}`} title={t`对话`} />
        </div>
        <Card pad={12} class="split-side">
          <div class="row-inline">
            <span class="card-title grow">{t`负责的子任务`}</span>
            <span class="caption secondary">{t`${tasks.length} 个`}</span>
          </div>
          {!tasks.length && <div class="callout secondary">{id === "lead" ? t`负责人管规划和协调。` : t`还没有分配子任务。`}</div>}
          {tasks.map((x) => {
            const [l, tone] = taskLabel(x.status);
            return (
              <div class="row-inline">
                <a class="link grow clamp-2" {...link(taskLink(run.id, x.id))}>{`${x.id} ${x.title}`}</a>
                <Pill text={l} tone={tone} />
              </div>
            );
          })}
        </Card>
      </div>
    </div>
  );
}

/** The audit log's filters, kept while the run is open (Mac: `auditCat`, `auditTask`, `auditActor`). */
export const auditFilter = { cat: "all", task: "", actor: "" };

export function AuditPage({ run, seq }: { run: RunModel; seq: number | null }) {
  const full = run.view.value;
  const events = run.events.value;
  const [, redraw] = useState(0);
  const [invOpen, setInvOpen] = useState(false);
  const highlight = useRef<HTMLDivElement>(null);
  const catFade = useEdgeFade<HTMLDivElement>();
  const set = (k: keyof typeof auditFilter, v: string) => {
    auditFilter[k] = v;
    redraw((n) => n + 1);
  };
  // Opening an event clears the filters, so the event is in the list.
  useEffect(() => {
    if (seq !== null) {
      auditFilter.cat = "all";
      auditFilter.task = "";
      auditFilter.actor = "";
      redraw((n) => n + 1);
      setTimeout(() => highlight.current?.scrollIntoView({ block: "center" }), 50);
    }
  }, [seq]);
  const f = auditFilter;
  const rows = events.filter(
    (e) =>
      (f.cat === "all" || category(e.type) === f.cat) &&
      (!f.task || e.task === f.task || e.data?.target === f.task) &&
      (!f.actor || e.actor === f.actor || e.to === f.actor),
  );
  const actors: string[] = [];
  for (const e of events) for (const a of [e.actor, e.to]) if (a && !actors.includes(a)) actors.push(a);
  // A run can have thousands of events: draw a page of them, and more as the list's end comes
  // into view (all at once made a phone wait seconds on tens of thousands of elements).
  const [limit, setLimit] = useState(AUDIT_PAGE);
  const opened = seq === null ? -1 : rows.findIndex((e) => e.seq === seq);
  const shown = rows.slice(0, Math.max(limit, opened + AUDIT_PAGE / 4));
  const more = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = more.current;
    if (!el || typeof IntersectionObserver === "undefined") return;
    const io = new IntersectionObserver((es) => {
      if (es.some((x) => x.isIntersecting)) setLimit((n) => n + AUDIT_PAGE);
    }, { rootMargin: "600px" });
    io.observe(el);
    return () => io.disconnect();
  }, [shown.length < rows.length]);
  const invs: Json[] = full?.audit ?? [];
  const broken = invs.filter((i) => !i.ok).length;
  useDrawn("audit", () => ({ rules: invs.map((i) => ({ text: i.text, ok: i.ok })), rows: rows.length, drawn: shown.length, events: events.length, highlight: seq }));
  return (
    <div class="audit-page">
      <section class="disclosure">
        <button type="button" class="disclosure-row" aria-expanded={broken > 0 || invOpen} onClick={() => setInvOpen(!invOpen)} title={t`工作台的状态都由这些事件推出。选中一条看原文，点「回放到这里」看当时的样子。`}>
          <span class={broken ? "fg-red" : "fg-green"}><Icon name={broken ? "exclamationmark.triangle.fill" : "checkmark.seal"} size={14} /></span>
          <span class="callout strong">{broken ? t`${broken} 条系统规则没满足` : t`系统规则全部满足`}</span>
          <span class="caption secondary">{t`${invs.length} 条，基于 ${events.length} 条事件`}</span>
        </button>
        {(broken > 0 || invOpen) && (
          <div class="rules">
            {invs.map((i) => (
              <div class="row-inline baseline">
                <b class={i.ok ? "fg-green" : "fg-red"} aria-label={i.ok ? t`满足` : t`不满足`}>{i.ok ? "✓" : "✕"}</b>
                <span class="callout">{i.text}</span>
                <span class="caption secondary">{i.note}</span>
              </div>
            ))}
          </div>
        )}
      </section>
      <div class="audit-filters">
        <div class="segmented scroll" role="tablist" aria-label={t`事件类别`} ref={catFade}>
          {categories().map(([v, label]) => (
            <button type="button" role="tab" aria-selected={f.cat === v} class={"segment" + (f.cat === v ? " on" : "")} onClick={() => set("cat", v)}>{label}</button>
          ))}
        </div>
        <select class="text-field" aria-label={t`子任务`} value={f.task} onChange={(e) => set("task", e.currentTarget.value)}>
          <option value="">{t`全部子任务`}</option>
          {(full?.tasks ?? []).map((x: Json) => <option value={x.id}>{`${x.id} ${short(x.title, 14)}`}</option>)}
        </select>
        <select class="text-field" aria-label={t`参与者`} value={f.actor} onChange={(e) => set("actor", e.currentTarget.value)}>
          <option value="">{t`全部参与者`}</option>
          {actors.map((a) => <option value={a}>{who(run, a).name}</option>)}
        </select>
        <span class="caption secondary">{t`${rows.length} 条`}</span>
      </div>
      <Card pad={0} class="audit-list">
        {shown.map((e) => {
          const on = seq === e.seq;
          return (
            <div class={"audit-row" + (on ? " on" : "")} ref={on ? highlight : undefined} data-seq={e.seq}>
              <div class="audit-line" onClick={() => go(runLink(run.id, on ? "audit" : `audit/${e.seq}`), true)}>
                <span class="mono caption audit-seq">{e.seq}</span>
                <span class="mono caption secondary">{clock(e.ts)}</span>
                <span class="mono caption audit-type ellipsis">{e.type}</span>
                <span class="callout audit-who ellipsis">{who(run, e.actor).name + (e.to ? ` → ${who(run, e.to).name}` : "")}</span>
                <span class="mono caption audit-task ellipsis">{(e.task ?? "—") + (e.attempt ? ` #${e.attempt}` : "")}</span>
                <span class="callout ellipsis grow">{e.text}</span>
                <button type="button" class="btn btn-bordered btn-small" title={t`把工作台回放到第 ${e.seq} 条事件时的状态`} onClick={(ev) => { ev.stopPropagation(); run.setReplay(events.indexOf(e) + 1); go(runLink(run.id)); }}>{t`回放到这里`}</button>
              </div>
              {on && (e.data?.checks ?? []).length > 0 && ["result.submitted", "verification.passed", "verification.failed", "task.succeeded"].includes(e.type) && (
                <div class="audit-checks">
                  <div class="caption strong secondary">{t`逐条验收`}</div>
                  <CheckLines lines={full?.checks?.[e.seq] ?? []} />
                </div>
              )}
              {on && <pre class="code-block mono audit-raw">{JSON.stringify(e, null, 2)}</pre>}
            </div>
          );
        })}
        {shown.length < rows.length && (
          <div class="row secondary" ref={more}>
            {t`还有 ${rows.length - shown.length} 条`}
          </div>
        )}
      </Card>
    </div>
  );
}

/** How many audit rows are drawn at first, and added each time the list's end comes into view. */
const AUDIT_PAGE = 300;

export function TeamPage({ run }: { run: RunModel }) {
  const full = run.view.value;
  const team: Json[] = run.run.value?.config?.team ?? [];
  const ws = run.run.value?.config?.workspace?.id;
  useDrawn("team", () => ({ members: team.map((m) => m.id), workspace: full?.page?.team?.workspace ?? "", branch: full?.page?.team?.branch ?? null }));
  if (!full) return null;
  const notes = full.page?.team ?? {};
  return (
    <div class="team-page">
      <div class="row-inline">
        <div class="grow">
          <h2 class="display">{t`团队与权限`}</h2>
          <div class="callout secondary">{t`这个任务创建时的团队快照。`}</div>
        </div>
        {ws && <a class="btn btn-bordered" {...link(`/workspaces/${encodeURIComponent(ws)}`)}>{t`工作区设置…`}</a>}
      </div>
      <Card>
        <KV rows={[
          [t`负责人`, who(run, "lead").name],
          [t`工作区`, notes.workspace ?? ""],
          ...(notes.branch ? ([[t`工作目录`, full.run?.cwd ?? ""], [t`分支`, notes.branch]] as [string, string][]) : []),
          [t`开始`, dateTime(full.run?.start ?? 0)],
        ]} />
      </Card>
      <div class="grid-cards">
        {team.map((m) => {
          const w = who(run, m.id);
          const [pl, pt] = phaseLabel(full.agents?.[m.id]?.phase ?? "idle");
          return (
            <Card>
              <div class="row-inline">
                <Avatar role={w.role} size={34} />
                <div class="grow min0">
                  <div class="card-title">{w.name + (w.lead ? t` · 负责人` : "")}</div>
                  <div class="caption secondary">{`${w.runtime} · ACP`}</div>
                </div>
                <Pill text={pl} tone={pt} />
              </div>
              {m.desc && <div class="callout">{m.desc}</div>}
              <KV rows={[
                [t`权限`, m.permission || t`Agent 没有权限模式`],
                ["Agent", `${w.runtime} · ${w.model || t`默认模型`}${m.effort ? ` · ${m.effort}` : ""}`],
              ]} />
            </Card>
          );
        })}
      </div>
    </div>
  );
}

