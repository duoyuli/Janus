// A goal's page (Mac: UI/Run/RunView.swift): its workbench, its audit log with replay, its team,
// and a subtask's or an agent's page, a diff and a document under it. The title bar has the three
// views, the run's documents, copying the delivery or the log, and deleting; a replayed position
// shows the replay bar; a queued goal shows what it will do and the two choices. The run is the
// one the run page opened (`pages/run.tsx`), shared with its conversations.

import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { Json } from "../lib/api.ts";
import { t } from "../lib/i18n.ts";
import { go, link, search } from "../lib/router.ts";
import { confirmDeleteRun } from "../lib/confirm.ts";
import { loadInbox, loadRuns, show } from "../lib/store.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageActions, usePageTitle } from "../lib/title.ts";
import { Button, Card, Menu, Pill, Spinner, useEdgeFade } from "../ui/kit.tsx";
import { Icon } from "../ui/Icon.tsx";
import { Markdown } from "../session/Markdown.tsx";
import { BoardView } from "./board.tsx";
import { DiffPage } from "./diff.tsx";
import { docGroups, DocPage } from "./doc.tsx";
import { api } from "../lib/api.ts";
import { who } from "../session/Items.tsx";
import type { RunModel } from "../session/model.ts";
import { agentLink, docLink, runLink, type RunSub, taskLink } from "./nav.ts";
import { AgentPage, AuditPage, TaskPage, TeamPage } from "./pages.tsx";
import { clock, short } from "./parts.tsx";
import "./run.css";
import { copyText } from "../lib/plain-http.ts";

/** Delete a run (`DELETE /api/runs/{id}`); what the service says when it may not. */
async function deleteRun(id: string): Promise<boolean> {
  try {
    await api.delete(`/api/runs/${encodeURIComponent(id)}`);
    return true;
  } catch (e) {
    show(e instanceof Error ? e.message : String(e));
    return false;
  }
}

/** A conversation's key as the run page's path: `task:T1` is the subtask's page, `agent:m1` the agent's. */
function sessionPath(id: string, key: string): string {
  const at = key.indexOf(":");
  const [kind, x] = [key.slice(0, at), key.slice(at + 1)];
  return kind === "task" ? taskLink(id, x) : agentLink(id, x);
}

export function GoalPage({ run, sub }: { run: RunModel; sub: RunSub }) {
  const full = run.view.value;
  // A session's card opens another conversation of the run on its page; so does `?session=<key>`.
  run.openSession = (key) => go(sessionPath(run.id, key));
  const session = new URLSearchParams(search.value).get("session");
  useEffect(() => {
    if (session) go(sessionPath(run.id, session), true);
  }, [session]);
  const title: string = full?.title ?? "";
  const ws = run.run.value?.config?.workspace?.id;
  usePageTitle(title, sub.kind === "board" ? (ws ? `/workspaces/${encodeURIComponent(ws)}` : "/inbox") : runLink(run.id));
  useDrawn("run", () => ({ id: run.id, sub: sub.kind, status: full?.run?.status ?? null, title, replay: run.replay.value, readOnly: !!full?.readOnly }));
  // Esc leaves replay first, then a task or agent page (Mac: `onExitCommand`).
  useEffect(() => {
    const esc = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || (e.target as HTMLElement)?.closest?.("input, textarea, dialog")) return;
      if (run.replaying.value) run.setReplay(null);
      else if (sub.kind !== "board") go(runLink(run.id));
    };
    addEventListener("keydown", esc);
    return () => removeEventListener("keydown", esc);
  }, [run, sub.kind]);
  if (!full) return <div class="page empty-state"><Spinner /></div>;
  if (full.run?.status === "queued") return <QueuedView run={run} st={full} />;
  const inboxCount = (full.cards ?? []).length;
  const tab = sub.kind === "audit" ? "audit" : sub.kind === "team" ? "team" : sub.kind === "board" ? "board" : "";
  const page = ((): ComponentChildren => {
    switch (sub.kind) {
      case "audit":
        return <AuditPage run={run} seq={sub.seq} />;
      case "team":
        return <TeamPage run={run} />;
      case "task":
        return <TaskPage run={run} id={sub.id} />;
      case "agent":
        return <AgentPage run={run} id={sub.id} />;
      case "diff":
        return <DiffPage run={run} path={sub.path} />;
      case "doc":
        return <DocPage run={run} path={sub.path} from={sub.from} />;
      default:
        return <BoardView run={run} />;
    }
  })();
  if (sub.kind === "diff" || sub.kind === "doc") return page;
  return <RunFrame run={run} full={full} title={title} tab={tab} inboxCount={inboxCount} replayBar={run.replaying.value && (sub.kind === "board" || sub.kind === "task" || sub.kind === "agent")} page={page} />;
}

/** The goal's frame: its title, the three views and its tools; on a phone the tools are one 「…」
 * menu in the top bar and the views a row of their own that scrolls sideways. */
function RunFrame({ run, full, title, tab, inboxCount, replayBar, page }: { run: RunModel; full: Json; title: string; tab: string; inboxCount: number; replayBar: boolean; page: ComponentChildren }) {
  const docs: Json[] = full.docs ?? [];
  const remove = useDelete(run, title, full.actions?.delete);
  const fade = useEdgeFade<HTMLElement>();
  usePageActions(
    <Menu
      icon="ellipsis.circle"
      label={t`更多`}
      items={[
        ...docs.map((d) => ({ label: t`文档：${d.name}`, onSelect: () => go(docLink(run.id, d.path)) })),
        ...(full.page?.summary ? [{ label: t`复制交付摘要`, onSelect: () => copySummary(full.page.summary) }] : []),
        { label: t`导出事件日志…`, onSelect: () => exportLog(run.id) },
        { label: t`删除任务…`, onSelect: () => void remove() },
      ]}
    />,
  );
  const views: [string, string, string][] = [
    ["board", inboxCount === 0 ? t`工作台` : t`工作台 · ${inboxCount}`, runLink(run.id)],
    ["audit", t`审计与回放`, runLink(run.id, "audit")],
    ["team", t`团队与权限`, runLink(run.id, "team")],
  ];
  return (
    <div class="page wide run-page">
      <header class="run-head">
        <div class="run-head-title">
          <h1 class="display ellipsis">{title}</h1>
          {full.readOnly && <span class="caption secondary">{t`只读 · 这个任务已停止`}</span>}
        </div>
        <div class="run-views-wrap">
          <nav class="segmented run-views" aria-label={t`视图`} ref={fade}>
            {views.map(([v, label, href]) => (
              <a class={"segment" + (tab === v ? " on" : "")} aria-current={tab === v ? "page" : undefined} {...link(href)} onClick={(e) => {
                // The audit and team pages show the live run: going there ends a replay.
                if (v !== "board") run.setReplay(null);
                link(href).onClick(e);
              }}>{label}</a>
            ))}
          </nav>
        </div>
        <div class="run-head-tools">
          <DocumentsMenu run={run} docs={docs} />
          <Menu icon="square.and.arrow.up" label={t`分享`} items={[
            ...(full.page?.summary ? [{ label: t`复制交付摘要`, onSelect: () => copySummary(full.page.summary) }] : []),
            { label: t`导出事件日志…`, onSelect: () => exportLog(run.id) },
          ]} />
          <button type="button" class="icon-btn" aria-label={t`删除任务`} aria-disabled={!full.actions?.delete?.ok} title={full.actions?.delete?.ok ? t`删掉这个任务和它的全部记录、产物，删了就找不回来` : full.actions?.delete?.why ?? ""} onClick={() => void remove()}>
            <Icon name="trash" />
          </button>
        </div>
      </header>
      {replayBar && <ReplayBar run={run} />}
      {page}
    </div>
  );
}

function copySummary(text: string) {
  void copyText(text).then(() => show(t`已复制交付摘要`), () => show(text));
}

/** The run's event log as a file, through the browser's download. */
function exportLog(id: string) {
  const a = document.createElement("a");
  a.href = `/api/runs/${encodeURIComponent(id)}/export`;
  a.download = `janus-${id}.jsonl`;
  document.body.append(a);
  a.click();
  a.remove();
}

/** Delete the goal after asking; when the service does not allow it, say why instead. */
function useDelete(run: RunModel, title: string, why?: Json) {
  return async () => {
    if (!why?.ok) {
      show(why?.why ?? "");
      return;
    }
    const name = title ? t`「${title}」` : t`这个任务`;
    if (!(await confirmDeleteRun(name))) return;
    if (!(await deleteRun(run.id))) return;
    await loadRuns();
    await loadInbox();
    const ws = run.run.value?.config?.workspace?.id;
    go(ws ? `/workspaces/${encodeURIComponent(ws)}` : "/inbox", true);
    show(t`已删除任务`);
  };
}

/** The run's Markdown files, one click away: deliverables, artifacts, documents the agents wrote or mentioned. */
function DocumentsMenu({ run, docs }: { run: RunModel; docs: Json[] }) {
  const [open, setOpen] = useState(false);
  const groups = docGroups(docs);
  return (
    <div class="menu">
      <button type="button" class="icon-btn" aria-label={t`文档`} title={t`查看这个任务交付、保存或提到的 Markdown 文档`} aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen(!open)}>
        <Icon name="doc.richtext" />
      </button>
      {open && (
        <div class="menu-list docs-menu" role="menu" onClick={() => setOpen(false)}>
          {!groups.length && <div class="menu-note caption secondary">{t`还没有 Markdown 文档`}</div>}
          {groups.map(([title, list]) => (
            <>
              <div class="menu-section caption secondary">{title}</div>
              {list.map((d) => <a role="menuitem" class="menu-item" {...link(docLink(run.id, d.path))}>{d.label}</a>)}
            </>
          ))}
        </div>
      )}
    </div>
  );
}

function ReplayBar({ run }: { run: RunModel }) {
  const n = run.events.value.length;
  const pos = run.replay.value ?? n;
  const e = pos >= 1 && pos <= n ? run.events.value[pos - 1] : null;
  return (
    <div class="replay-bar">
      <span class="card-title">{t`回放模式`}</span>
      <span class="caption secondary">{t`只读 · 当时的状态`}</span>
      <button type="button" class="icon-btn" aria-label={t`上一条事件`} title={t`上一条事件（←）`} onClick={() => run.setReplay(Math.max(1, pos - 1))}><Icon name="chevron.left" /></button>
      <input type="range" class="grow" min={1} max={Math.max(n, 2)} value={pos} aria-label={t`回放位置`} aria-valuetext={t`第 ${pos} 条，共 ${n} 条`} onInput={(ev) => run.setReplay(Number(ev.currentTarget.value))} />
      <button type="button" class="icon-btn" aria-label={t`下一条事件`} title={t`下一条事件（→）`} onClick={() => run.setReplay(Math.min(n, pos + 1))}><Icon name="chevron.right" /></button>
      <span class="caption ellipsis replay-where">{e ? t`第 ${pos} / ${n} 条 · ${clock(e.ts)} · ${who(run, e.actor).name}：${short(e.text ?? "", 40)}` : ""}</span>
      <Button kind="primary" small title={t`退出回放（Esc）`} onClick={() => run.setReplay(null)}>{t`回到实时`}</Button>
    </div>
  );
}

/** A run waiting in the dispatch queue: it starts by itself once a slot is free; the person may
 * start it now (past the slots) or cancel it. */
function QueuedView({ run, st }: { run: RunModel; st: Json }) {
  const pos = run.run.value?.queuePosition;
  const goal: string = st.run?.goal ?? "";
  usePageTitle(st.title ?? "");
  return (
    <div class="page">
      <Card>
        <Pill text={typeof pos === "number" ? t`排队中 · 第 ${pos} 位` : t`排队中`} />
        <h1 class="display">{st.title}</h1>
        {goal.includes("\n") && <Markdown text={goal} />}
        <div class="callout secondary">{t`并行任务已满，排队中。前面的任务结束或暂停后自动开始。`}</div>
        <div class="flow">
          <Button onClick={() => void run.act({ type: "cancel" })}>{t`取消任务`}</Button>
          <Button kind="primary" title={t`跳过排队，马上开工`} onClick={() => void run.act({ type: "start" })}>{t`马上开工`}</Button>
        </div>
      </Card>
    </div>
  );
}
