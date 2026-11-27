// A workspace's page (Mac: UI/HomeView.swift), all of it on one page: who it is (name, folder,
// team) and what its runs know (the shared material, the team memory), the goal to start, what
// keeps its team from running, what waits on the person in it, its Loops and its task records.
// Also the page of a deleted workspace whose runs stay, and the list of workspaces (the sidebar's
// on a wide screen, the 「团队」 tab on a phone). Everything here is as the service says it
// (docs/architecture/20261031-frontend.md#screens).

import { useSignal } from "@preact/signals";
import { useEffect } from "preact/hooks";
import type { Json } from "../lib/api.ts";
import { dateTime, tilde } from "../lib/format.ts";
import { joinList } from "../lib/format.ts";
import { t } from "../lib/i18n.ts";
import { go, link } from "../lib/router.ts";
import type { Tone } from "../lib/status.ts";
import {
  createRun,
  deleteRun,
  history,
  inbox,
  loadMemories,
  loops,
  loopsOf,
  memories,
  onboarding,
  pastWorkspaces,
  role,
  runs,
  runWorkspaces,
  show,
  workingWorkspaces,
  workspace,
  workspaces,
} from "../lib/store.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { InitialMark } from "../app/Shell.tsx";
import { Icon } from "../ui/Icon.tsx";
import { AddImages, ImageStrip, MAX_IMAGES, type PickedImage, readImages } from "../ui/form.tsx";
import { Avatar, Button, Card, Menu, Note, Pill } from "../ui/kit.tsx";
import { badge } from "./inbox.tsx";
import "./workspace.css";

export function WorkspacesPage() {
  usePageTitle(t`团队`);
  useDrawn("workspaces", () => ({ workspaces: workspaces.value.map((w) => w.id), past: pastWorkspaces.value.map((w) => w.id), working: workingWorkspaces.value }));
  return (
    <div class="page">
      <div class="section-header">
        <h1 class="display">{t`团队`}</h1>
        <span class="grow" />
        <a class="btn btn-bordered btn-small" {...link("/workspaces/new")}>
          {t`新建工作区`}
        </a>
      </div>
      <OnboardingBar />
      <Card pad={4} class="rows">
        {workspaces.value.map((w) => {
          const n = waitingIn(w.id);
          return (
            <a class="row" {...link(`/workspaces/${encodeURIComponent(w.id)}`)}>
              <InitialMark text={w.name} />
              <div class="grow">
                <div class="ellipsis">{w.name}</div>
                <div class="caption ellipsis">{tilde(w.path)}</div>
              </div>
              {n > 0 ? <span class="badge">{n}</span> : workingWorkspaces.value.includes(w.id) && <span class="live-dot" aria-label={t`进行中`} />}
              <Icon name="chevron.right" size={14} class="secondary" />
            </a>
          );
        })}
        {pastWorkspaces.value.map((w) => (
          <a class="row secondary" title={t`已删除的工作区，留下 ${w.runs ?? 0} 个任务记录`} {...link(`/workspaces/${encodeURIComponent(w.id)}`)}>
            <InitialMark text={w.name} />
            <div class="grow ellipsis">{t`${w.name}（已删除）`}</div>
            <Icon name="chevron.right" size={14} />
          </a>
        ))}
      </Card>
      {workspaces.value.length === 0 && pastWorkspaces.value.length === 0 && <NoWorkspace />}
    </div>
  );
}

/** How many things the runs of workspace `id` wait on the person for, as the server folds the inbox. */
export function waitingIn(id: string): number {
  return (inbox.byWorkspace.value[id] ?? []).reduce((n: number, w: Json) => n + w.entries.length, 0);
}

/** Steps still missing before a first task can run, with the place that does the next one. */
export function OnboardingBar() {
  const steps = onboarding.value;
  if (steps.length === 0 || steps.every((s) => s.done)) return null;
  const next = steps.find((s) => !s.done);
  return (
    <Card class="onboarding">
      <span class="h2">{t`开始使用 Janus`}</span>
      {steps.map((s, i) => (
        <span class={"onboarding-step" + (s.done ? " done" : "")}>
          <span class="step-mark">{s.done ? "✓" : i + 1}</span>
          {s.text}
        </span>
      ))}
      <span class="grow" />
      {next && (
        <a class="btn btn-primary btn-small" {...link(next.go === "workspace" ? "/workspaces/new" : "/settings")}>
          {next.text}
        </a>
      )}
    </Card>
  );
}

function NoWorkspace() {
  return (
    <Card>
      <div class="display h1">{t`还没有工作区`}</div>
      <div class="secondary">{t`工作区 = 一个目录 + 一支数字员工团队`}</div>
      <div style={{ marginTop: "10px" }}>
        <a class="btn btn-primary" {...link("/workspaces/new")}>
          {t`新建工作区`}
        </a>
      </div>
    </Card>
  );
}

const SUGGESTIONS = () => [
  [t`新功能上线`, t`为这个产品规划并上线一个新功能：先确认目标用户和成功标准，写 PRD，实现并测试，准备发布说明`],
  [t`竞品与定位`, t`调研这个产品的主要竞品和目标用户，整理定位差异和三个最值得做的机会，给出依据`],
  [t`发布前检查`, t`检查这个产品是否可以发布：核对主要用户流程、已知缺陷、回滚方式和发布说明，给出质量结论`],
];

/** What waits on the person in each workspace, one row per goal or Loop, as the server folds the inbox. */
function waitingRows(ws: string): Json[] {
  return inbox.byWorkspace.value[ws] ?? [];
}

export function WorkspacePage({ id }: { id: string }) {
  const ws = workspace(id);
  const past = ws ? undefined : runWorkspaces.value.find((w) => w.id === id);
  const name = ws?.name ?? (past ? past.name || t`已删除的工作区` : "");
  usePageTitle(name, "/workspaces");
  const records = history(id);
  const waiting = waitingRows(id);
  const hiding = new Set(waiting.map((w) => w.run));
  const shown = records.runs.value.filter((r) => !hiding.has(r.id));
  const memory = memories.value[id];
  useEffect(() => {
    if (ws) void loadMemories(id);
  }, [id, !!ws]);

  useDrawn("workspace", () => ({
    id,
    name,
    deleted: !ws,
    teamLine: ws?.teamLine ?? null,
    briefStatus: ws?.briefStatus ?? null,
    memoryStatus: memory?.status ?? null,
    problems: (ws?.problems ?? []).map((p: Json) => p.text),
    waiting: waiting.map((w) => ({ run: w.run, loop: w.loop ?? null, title: w.title, label: w.label, tone: w.tone, entries: w.entries })),
    loops: loopsOf(id).map((l) => ({ id: l.id, title: l.title, pill: l.pill.text, schedule: l.scheduleText })),
    records: shown.map((r) => ({ id: r.id, title: r.title, label: r.label.text })),
    more: records.more,
  }));

  if (!ws && !past) {
    return (
      <div class="page">
        <Note text={t`没有这个工作区。`} />
      </div>
    );
  }
  if (!ws) {
    return (
      <div class="page">
        <div class="ws-identity">
          <h1 class="display ws-name">{name}</h1>
          <div class="secondary ellipsis">{tilde(past.path)}</div>
        </div>
        <Note text={t`工作区已删除。留下的任务记录还能查看、继续对话或删除；想在这里提新目标，新建一个工作区。`} />
        <History records={shown} more={records.more} onMore={() => void records.loadMore()} />
      </div>
    );
  }
  const problems: Json[] = ws.problems ?? [];
  return (
    <div class="page wide">
      {problems.length === 0 && <OnboardingBar />}
      <div class="ws-top">
        <Identity ws={ws} />
        <div class="ws-knowledge">
          <KnowledgeTile icon="doc.text" tint="accent" title={t`共享资料`} status={ws.briefStatus} way={t`每个任务都带上`} to={`/workspaces/${encodeURIComponent(id)}/brief`} />
          <KnowledgeTile icon="brain" tint="memory" title={t`团队记忆`} status={memory?.status ?? ""} way={t`需要时检索`} to={`/workspaces/${encodeURIComponent(id)}/memory`} />
        </div>
      </div>
      <GoalComposer ws={ws} problems={problems} firstRun={!runs.value.some((r) => r.workspace === id)} />
      {problems.map((p) => (
        <div class="ws-problem">
          <Note text={p.text} tone="amber" />
          <a class="btn btn-bordered btn-small" {...link("/settings")}>
            {p.action}
          </a>
        </div>
      ))}
      <div class="ws-columns">
        <div class="ws-main">
          {waiting.length > 0 && (
            <section class="stack">
              <div class="ws-heading">
                <span class="h2">{t`待处理`}</span>
                <span class="h2 fg-amber" aria-label={t`${waitingIn(id)} 项`}>
                  {waitingIn(id)}
                </span>
              </div>
              <Card pad={4} class="rows">
                {waiting.map((w) => (
                  <WaitingRow w={w} />
                ))}
              </Card>
            </section>
          )}
          <div class="ws-loops-narrow">
            <LoopSection ws={id} />
          </div>
          <History records={shown} more={records.more} onMore={() => void records.loadMore()} />
        </div>
        <aside class="ws-side">
          <LoopSection ws={id} />
        </aside>
      </div>
    </div>
  );
}

/** The name, the folder and the team, one under the other; the workspace settings from the menu beside the name. */
function Identity({ ws }: { ws: Json }) {
  const members: Json[] = ws.members ?? [];
  const names = members.map((m) => role(m.role)?.name).filter(Boolean) as string[];
  return (
    <div class="ws-identity">
      <div class="ws-name-row">
        <h1 class="display ws-name">{ws.name}</h1>
        <Menu icon="ellipsis.circle" label={t`工作区操作`} items={[{ label: t`工作区设置…`, onSelect: () => go(`/workspaces/${encodeURIComponent(ws.id)}/settings`) }]} />
      </div>
      <div class="secondary ellipsis" title={ws.path}>
        {tilde(ws.path)}
      </div>
      <div class="ws-team">
        <span class="ws-avatars" role="img" aria-label={joinList(names)}>
          {members.map((m) => {
            const r = role(m.role);
            return (
              <span title={r?.name ?? t`已删除的数字员工`}>
                <Avatar role={r} />
              </span>
            );
          })}
        </span>
        <span class="secondary">{ws.teamLine}</span>
      </div>
    </div>
  );
}

/** One of the two things a workspace's runs know: a tinted glyph, its name, and whether there is anything and how it reaches the agents. */
function KnowledgeTile({ icon, tint, title, status, way, to }: { icon: string; tint: "accent" | "memory"; title: string; status: string; way: string; to: string }) {
  return (
    <a class={`knowledge-tile tint-${tint}`} aria-label={`${title}，${status}，${way}`} {...link(to)}>
      <span class="knowledge-glyph">
        <Icon name={icon} size={15} />
      </span>
      <span>
        <span class="knowledge-title">{title}</span>
        <span class="caption knowledge-line">{[status, way].filter(Boolean).join(" · ")}</span>
      </span>
    </a>
  );
}

/** The goal to start: what the page is for, wide; ⌘↩ (Ctrl+Enter) or the button sends it to the lead. */
function GoalComposer({ ws, problems, firstRun }: { ws: Json; problems: Json[]; firstRun: boolean }) {
  const draft = useSignal("");
  const images = useSignal<PickedImage[]>([]);
  const sending = useSignal(false);
  const empty = draft.value.trim() === "" && images.value.length === 0;
  const blocked = problems.length > 0;
  const add = async (files: File[]) => {
    const { images: got, refused } = await readImages(files);
    if (refused.length) show(t`图片太大，没有加上：${joinList(refused)}`);
    images.value = [...images.value, ...got].slice(0, MAX_IMAGES);
  };
  const start = async () => {
    if (empty || blocked || sending.value) return;
    sending.value = true;
    const id = await createRun(draft.value.trim(), ws.id, images.value.map(({ data, name, mimeType }) => ({ data, name, mimeType })));
    sending.value = false;
    if (id) {
      draft.value = "";
      images.value = [];
      go(`/runs/${encodeURIComponent(id)}`);
    }
  };
  return (
    <div class="stack">
      <div class="composer card">
        <ImageStrip images={images.value} onChange={(v) => (images.value = v)} />
        <textarea
          class="composer-field"
          rows={4}
          placeholder={t`想要什么结果？`}
          aria-label={t`想要什么结果？`}
          value={draft.value}
          onInput={(e) => (draft.value = (e.target as HTMLTextAreaElement).value)}
          onPaste={(e) => {
            const files = [...(e.clipboardData?.files ?? [])].filter((f) => f.type.startsWith("image/"));
            if (files.length) {
              e.preventDefault();
              void add(files);
            }
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              void start();
            }
          }}
        />
        <div class="composer-bar">
          <AddImages onPick={(f) => void add(f)} disabled={images.value.length >= MAX_IMAGES} />
          <span class="caption">{t`可以粘贴截图`}</span>
          <span class="grow" />
          <Button kind="primary" disabled={blocked || empty || sending.value} title={blocked ? t`还有数字员工不能运行` : empty ? t`先写下你想要的结果` : "⌘↩"} onClick={() => void start()}>
            {t`交给负责人`}
          </Button>
        </div>
      </div>
      {firstRun && (
        <div class="suggestions">
          {SUGGESTIONS().map(([name, text]) => (
            <Button small title={text} onClick={() => (draft.value = text!)}>
              {name}
            </Button>
          ))}
        </div>
      )}
    </div>
  );
}

/** A goal or Loop that waits on the person: a glyph for what it waits on, what it is, and the pill. */
function WaitingRow({ w }: { w: Json }) {
  const one = w.entries.length === 1 ? inbox.open.value.find((e) => e.key === w.entries[0]) : undefined;
  const symbol = one ? badge(one)[0] : "tray.full.fill";
  const tone = w.tone as Tone;
  const run = runs.value.find((r) => r.id === w.run);
  return (
    <a class="row" title={w.entries.map((k: string) => inbox.open.value.find((e) => e.key === k)?.title ?? "").join("\n")} {...link(`/runs/${encodeURIComponent(w.run)}`)}>
      <span class={`fg-${tone}`}>
        <Icon name={symbol} />
      </span>
      <span class="grow ellipsis">{w.title}</span>
      {w.loop ? <span class="caption">{t`值班`}</span> : run && <LoopMark r={run} />}
      <Pill text={w.label} tone={tone} />
    </a>
  );
}

/** A small tag after a goal a Loop dispatched: which Loop. */
export function LoopMark({ r }: { r: Json }) {
  if (r.origin !== "loop") return null;
  const name = loops.value.find((l) => l.id === r.loopId)?.title ?? t`已删除的值班`;
  const help = [t`来自值班：${name}`, r.dispatchKey, r.sourceUrl].filter(Boolean).join(" · ");
  return (
    <span class="loop-mark caption" title={help} aria-label={t`来自值班：${name}`}>
      <Icon name="clock.arrow.circlepath" size={12} />
      <span class="ellipsis">{name}</span>
    </span>
  );
}

/** A workspace's task records, newest first, with a menu to delete one; more as the list ends. */
function History({ records, more, onMore }: { records: Json[]; more: boolean; onMore: () => void }) {
  if (records.length === 0 && !more) return null;
  return (
    <section class="stack">
      {records.length > 0 && <span class="h2">{t`任务记录`}</span>}
      {records.length > 0 && (
        <Card pad={4} class="rows">
          {records.map((r) => (
            <RunRow r={r} />
          ))}
        </Card>
      )}
      {more && <More onMore={onMore} />}
    </section>
  );
}

export function RunRow({ r }: { r: Json }) {
  const waiting = inbox.open.value.some((e) => e.run === r.id);
  const [label, tone]: [string, Tone] = waiting ? [t`待处理`, "amber"] : [r.label.text, r.label.tone];
  return (
    <div class="row run-row">
      <a class="grow run-link" {...link(`/runs/${encodeURIComponent(r.id)}`)}>
        <span class="ellipsis">{r.title}</span>
        <LoopMark r={r} />
        <span class="grow" />
        <span class="caption run-date">{dateTime(r.createdAt ?? 0)}</span>
        <Pill text={label} tone={tone} />
      </a>
      <Menu icon="ellipsis" label={t`更多`} items={[{ label: r.delete?.ok === false ? `${t`删除任务…`}（${r.delete.why}）` : t`删除任务…`, onSelect: () => void deleteRun(r) }]} />
    </div>
  );
}

/** The Loops of one workspace: one compact row each, the ones to look at first on top; a few at a time, the rest a click away. */
function LoopSection({ ws }: { ws: string }) {
  const all = useSignal(false);
  const list = loopsOf(ws);
  const rows = all.value ? list : list.slice(0, 4);
  return (
    <section class="stack">
      <div class="ws-heading">
        <span class="h2 grow">{t`值班`}</span>
        <a class="icon-btn" aria-label={t`新建值班`} title={t`新建值班`} {...link(`/workspaces/${encodeURIComponent(ws)}/loops/new`)}>
          <Icon name="plus" />
        </a>
      </div>
      {list.length === 0 ? (
        <div class="secondary">{t`按时间表开工：看一眼、做判断，要改的事派成任务`}</div>
      ) : (
        <Card pad={4} class="rows">
          {rows.map((l) => {
            const schedule = [l.scheduleText, l.wakesAt ? t`${dateTime(l.wakesAt)} 开工` : ""].filter(Boolean).join(" · ");
            const last = l.status?.lastSummary ?? "";
            return (
              <a class="row loop-row" title={[l.title, schedule, l.pill.text, last ? t`上次：${last}` : ""].filter(Boolean).join("\n")} {...link(`/runs/${encodeURIComponent(l.run)}`)}>
                <LoopBadge size={20} />
                <span class="grow loop-lines">
                  <span class="loop-title">
                    <span class="ellipsis grow">{l.title}</span>
                    <Pill text={l.pill.text} tone={l.pill.tone} />
                  </span>
                  <span class="caption ellipsis">{schedule}</span>
                  {last && <span class="caption ellipsis">{t`上次：${last}`}</span>}
                </span>
              </a>
            );
          })}
          {list.length > 4 && (
            <button type="button" class="btn btn-plain btn-small" onClick={() => (all.value = !all.value)}>
              {all.value ? t`收起` : t`展开全部（${list.length}）`}
            </button>
          )}
        </Card>
      )}
    </section>
  );
}

/** A Loop's own mark: a tinted tile, so it reads apart from what waits and from the goals it dispatched. */
export function LoopBadge({ size = 24 }: { size?: number }) {
  return (
    <span class="loop-badge" style={{ width: `${size}px`, height: `${size}px`, borderRadius: `${size * 0.28}px` }} aria-hidden="true">
      <Icon name="clock.arrow.circlepath" size={Math.round(size * 0.6)} />
    </span>
  );
}

/** The end of a list that has more: reads the next page when it scrolls into sight, or on a tap. */
export function More({ onMore }: { onMore: () => void }) {
  let el: HTMLDivElement | null = null;
  useEffect(() => {
    if (!el) return;
    const seen = new IntersectionObserver((es) => es.some((x) => x.isIntersecting) && onMore());
    seen.observe(el);
    return () => seen.disconnect();
  });
  return (
    <div
      ref={(x) => {
        el = x;
      }}
      class="more"
    >
      <Button kind="plain" small onClick={onMore}>
        {t`更多`}
      </Button>
    </div>
  );
}
