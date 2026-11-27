// A goal's workbench (Mac: `BoardView`, `RunCard`, `InboxSection`, `AgentsPanel`, `RunInspector`
// in UI/Run/RunView.swift): the run card, what waits on the person, the delivery, the progress,
// and the lead's conversation with the inspector beside it (below it on a phone). What each part
// says is the service's; the page only lays it out.

import { signal } from "@preact/signals";
import { useState } from "preact/hooks";
import type { Json } from "../lib/api.ts";
import { t } from "../lib/i18n.ts";
import { link } from "../lib/router.ts";
import { load, save } from "../lib/prefs.ts";
import { confirm } from "../lib/confirm.ts";
import { useDrawn } from "../lib/testing.ts";
import { Avatar, Button, Card, Note, Pill } from "../ui/kit.tsx";
import { Icon } from "../ui/Icon.tsx";
import { Markdown } from "../session/Markdown.tsx";
import type { RunModel } from "../session/model.ts";
import { who } from "../session/Items.tsx";
import { agentLink, diffLink, docLink, taskLink } from "./nav.ts";
import { phaseLabel, StatusGlyph, TaskTag, taskLabel } from "./parts.tsx";
import { RequestList } from "../session/Requests.tsx";
import { ResultCard } from "./review.tsx";
import { SessionPanel } from "../session/SessionPanel.tsx";
import { ProgressBoard } from "./timeline.tsx";
import { cardLine, UsageSection, useTicking } from "./usage.tsx";
import { ago } from "../lib/format.ts";

/** Bumped by 「让负责人改…」: the lead's composer takes the focus. */
export const leadFocus = signal(0);

/** Stop the run, after asking (Mac: `cancelRun`). */
export async function cancelRun(run: RunModel) {
  if (await confirm({ title: t`停止这个任务？`, text: t`所有会话都会结束，已有改动保留。停了就不能再继续。`, ok: t`停止` })) await run.act({ type: "cancel" });
}


export function BoardView({ run }: { run: RunModel }) {
  const st = run.shown.value;
  const ticking = run.canAct.value && ["planning", "running"].includes(st?.run?.status);
  const now = useTicking(ticking);
  const live = !!st && !st.run?.status?.match(/delivered|accepted|cancelled|failed|interrupted/) && !!st.run?.goal && !run.replaying.value;
  const cards: Json[] = run.view.value?.cards ?? [];
  useDrawn("runBoard", () => ({
    capsule: st?.capsule,
    title: st?.title,
    summary: cardLine(run.usage.value, st?.page?.progress ?? null, now, st?.run?.start ?? 0, ticking).line,
    reminders: run.usage.value?.words?.reminders ?? null,
    requests: cards.map((c) => c.key),
    review: st?.review ? { stage: st.review.stage, line: st.page?.reviewLine ?? null, signals: (st.review.signals ?? []).map((s: Json) => s.text), facts: st.delivery?.facts ?? "" } : null,
    rows: (st?.delivery?.tasks ?? []).map((r: Json) => ({ task: r.task, metrics: r.metrics, markers: r.markers })),
    inspector: { tasks: (st?.tasks ?? []).map((x: Json) => x.id), diffs: (st?.page?.inspector?.diffs ?? []).map((a: Json) => a.path), files: (st?.page?.inspector?.files ?? []).map((a: Json) => a.path), delivered: (st?.page?.inspector?.delivered ?? []).map((d: Json) => d.path) },
    members: (st?.members ?? []).map((m: Json) => ({ id: m.id, phase: m.phase, line: m.line })),
    actions: st?.actions,
  }));
  if (!st) return null;
  return (
    <div class="board">
      <RunCard run={run} st={st} now={now} ticking={ticking} />
      <RequestList run={run} />
      <ResultCard run={run} st={st} />
      <ProgressBoard run={run} st={st} now={live ? Math.max(st.run?.now ?? 0, now) : st.run?.now ?? 0} live={live} />
      <div class="board-columns">
        <div class="board-main">
          <SessionPanel run={run} sessionKey="agent:lead" title={t`负责人`} />
        </div>
        <RunInspector run={run} st={st} />
      </div>
    </div>
  );
}

function RunCard({ run, st, now, ticking }: { run: RunModel; st: Json; now: number; ticking: boolean }) {
  const [goalOpen, setGoalOpen] = useState(false);
  const [usageOpen, setUsageOpen] = useState(false);
  const r = st.run ?? {};
  const title: string = st.title ?? "";
  const long = (r.goal ?? "").trim() !== title;
  const a = st.actions ?? {};
  const pauseOp = r.paused ? "resume" : "pause";
  const pause = !!a[pauseOp]?.ok;
  const stop = !!a.cancel?.ok;
  const usage = run.usage.value;
  const { line, voice } = cardLine(usage, st.page?.progress ?? null, now, r.start ?? 0, ticking);
  const reference = run.run.value?.config?.tokenLimit;
  return (
    <Card pad={18} class="run-card">
      <div class="run-card-top">
        <div class="run-card-heading">
          <Pill text={st.capsule?.text ?? ""} tone={st.capsule?.tone ?? "gray"} />
          <h2 class="display run-title">{title || t`正在连接 Janus 服务…`}</h2>
          {long && (
            <button type="button" class="link callout" onClick={() => setGoalOpen(!goalOpen)}>{goalOpen ? t`收起完整目标` : t`查看完整目标`}</button>
          )}
          {long && goalOpen && <Markdown text={r.goal} />}
          {(r.images ?? []).length > 0 && (
            <div class="flow">
              {r.images.map((p: string) => <img class="stored-image" src={`/api/runs/${encodeURIComponent(run.id)}/image?path=${encodeURIComponent(p)}`} alt="" width={64} height={64} />)}
            </div>
          )}
        </div>
        {(pause || stop) && (
          <div class="flow run-card-actions">
            {pause && (
              <Button title={r.paused ? t`继续调度子任务（⇧⌘P）` : t`暂停：不再派新活，已开工的照常跑（⇧⌘P）`} onClick={() => void run.act({ type: pauseOp })}>
                <Icon name={r.paused ? "play.fill" : "pause.fill"} size={12} /> {r.paused ? t`继续` : t`暂停`}
              </Button>
            )}
            {stop && <Button title={t`结束这个任务的所有会话`} data-action="cancel" onClick={() => void cancelRun(run)}>{t`停止…`}</Button>}
          </div>
        )}
      </div>
      {r.status === "failed" && r.error && <Note text={r.error} tone="red" />}
      {st.resumeNote && (
        <div class="resume-note">
          <span class="callout secondary grow">{st.resumeNote}</span>
          <Button kind="primary" disabled={!a.resumeRun?.ok} title={a.resumeRun?.why ?? ""} onClick={() => void run.act({ type: "resumeRun" })}>{t`从中断处继续`}</Button>
        </div>
      )}
      <div class="summary-row">
        <span class="callout secondary grow" title={voice} aria-label={voice} data-summary>{line}</span>
        {(usage?.words?.hasUsage || usageOpen) && (
          <button type="button" class="link callout" aria-expanded={usageOpen} aria-label={usageOpen ? t`收起用量明细` : t`展开用量明细`} title={t`展开用量：Token、费用、耗时、上下文`} onClick={() => setUsageOpen(!usageOpen)}>
            <Icon name={usageOpen ? "chevron.down" : "chevron.right"} size={10} /> {t`用量明细`}
          </button>
        )}
      </div>
      {typeof reference === "number" && reference > 0 && <div class="callout secondary">{t`Token 用量参考值：${reference.toLocaleString()}`}</div>}
      {usage?.words?.reminders && <Note text={usage.words.reminders} tone="amber" />}
      {usageOpen && <UsageSection run={run} st={st} />}
    </Card>
  );
}

/** Each member's row, as the service works it out (Mac: `AgentsPanel`). */
function AgentsPanel({ run, st }: { run: RunModel; st: Json }) {
  const members: Json[] = st.members ?? [];
  const active = members.filter((m) => m.phase !== "idle").length;
  return (
    <div class="agents-panel">
      {active > 0 && <div class="caption secondary">{t`${active} 个在干活`}</div>}
      <div class="rows">
        {members.map((m) => {
          const w = who(run, m.id);
          const [pl, pt] = phaseLabel(m.phase);
          const meta = [w.runtime, w.model, m.beat ? t`最近活动 ${ago(m.beat, st.run?.now ?? Date.now())}` : "", m.stats ?? ""].filter(Boolean).join(" · ");
          return (
            <a class="member-row" title={meta + t`\n点按查看 ${w.name} 的对话`} {...link(agentLink(run.id, m.id))}>
              <Avatar role={w.role} size={30} />
              <span class="grow min0">
                <span class="row-inline">
                  <span class="callout strong grow">{w.name}</span>
                  <Pill text={pl} tone={pt} />
                </span>
                <span class="caption secondary clamp-2">{m.line}</span>
                {typeof m.progress === "number" && <span class="bar"><span style={{ width: `${Math.round(m.progress * 100)}%` }} /></span>}
              </span>
            </a>
          );
        })}
      </div>
    </div>
  );
}

/** Beside the lead's conversation: subtasks, changes, files and the team, one at a time. */
function RunInspector({ run, st }: { run: RunModel; st: Json }) {
  const tasks: Json[] = st.tasks ?? [];
  const ins = st.page?.inspector ?? { diffs: [], files: [], delivered: [] };
  const saved = load<string | null>("inspectorTab", null);
  const [tab, setTab] = useState<string>(saved ?? (tasks.length ? "tasks" : "team"));
  const choose = (v: string) => {
    setTab(v);
    save("inspectorTab", v);
  };
  const tabs: [string, string][] = [
    ["tasks", t`子任务 ${tasks.length}`],
    ["changes", t`改动 ${ins.diffs.length}`],
    ["files", t`产物 ${ins.files.length + ins.delivered.length}`],
    ["team", t`团队`],
  ];
  const artifacts = (list: Json[], empty: string) => (
    <>
      {!list.length && empty && <div class="callout secondary pad-v">{empty}</div>}
      {list.map((a) => {
        const ofRun = a.kind === "run_diff";
        const diff = ofRun || a.kind === "git_diff";
        return (
          <div class="inspector-row">
            <span class="grow min0">
              <span class="row-inline">
                {ofRun ? <span class="callout strong">{t`整个任务`}</span> : <TaskTag id={a.task} />}
                <span class={"callout ellipsis" + (ofRun ? " secondary" : " strong")}>{a.name}</span>
              </span>
              {a.desc && <span class="caption secondary clamp-2">{a.desc}</span>}
            </span>
            <a class="link callout" {...link(diff ? diffLink(run.id, a.path) : docLink(run.id, a.path))}>{diff ? t`审阅` : t`查看`}</a>
          </div>
        );
      })}
    </>
  );
  return (
    <Card pad={12} class="inspector">
      <div class="segmented" role="tablist" aria-label={t`查看`}>
        {tabs.map(([v, label]) => (
          <button type="button" role="tab" aria-selected={tab === v} class={"segment" + (tab === v ? " on" : "")} onClick={() => choose(v)}>{label}</button>
        ))}
      </div>
      <div class="inspector-body">
        {tab === "changes" && artifacts(ins.diffs, t`还没有改动。`)}
        {tab === "files" && (
          <>
            {ins.delivered.length > 0 && (
              <>
                <div class="caption strong secondary pad-top">{t`交付物`}</div>
                {ins.delivered.map((d: Json) => (
                  <div class="inspector-row" title={d.path}>
                    <span class="grow min0">
                      <span class="callout strong ellipsis block">{d.name}</span>
                      <span class="caption secondary ellipsis block">{d.dir}</span>
                    </span>
                    <a class="link callout" {...link(docLink(run.id, d.path))}>{t`查看`}</a>
                  </div>
                ))}
                <hr />
              </>
            )}
            {artifacts(ins.files, ins.delivered.length ? "" : t`还没有产物。`)}
          </>
        )}
        {tab === "team" && <AgentsPanel run={run} st={st} />}
        {tab === "tasks" && (
          <>
            {!tasks.length && <div class="callout secondary pad-v">{t`负责人还没有派出子任务。`}</div>}
            {tasks.map((x) => {
              const [label, tone] = taskLabel(x.status);
              const w = who(run, x.agent);
              return (
                <a class="inspector-row" {...link(taskLink(run.id, x.id))}>
                  <StatusGlyph tone={tone} label={label} />
                  <span class="grow min0">
                    <span class="callout strong clamp-2">{x.title}</span>
                    <span class="row-inline caption secondary">
                      <TaskTag id={x.id} />
                      <Avatar role={w.role} size={14} />
                      <span class="ellipsis">{`${w.name} · ${label}`}</span>
                    </span>
                  </span>
                </a>
              );
            })}
          </>
        )}
      </div>
    </Card>
  );
}
