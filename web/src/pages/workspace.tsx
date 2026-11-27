// A workspace's page (Mac: UI/HomeView.swift): who it is (name, folder, team), what waits on the
// person in it (the inbox's entries of its runs, one row per run), and its task records, newest
// first, read a page at a time. Also the page of a deleted workspace whose runs stay. And the list
// of workspaces, which is the sidebar's on a wide screen and the 「团队」 tab on a phone.

import { useEffect } from "preact/hooks";
import type { Json } from "../lib/api.ts";
import { dateTime, goalTitle, joinList, tilde } from "../lib/format.ts";
import { t } from "../lib/i18n.ts";
import { link } from "../lib/router.ts";
import { runLabel, type Tone } from "../lib/status.ts";
import { history, inbox, pastWorkspaces, role, runWorkspaces, workspace, workspaces } from "../lib/store.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { InitialMark, waitingIn } from "../app/Shell.tsx";
import { Icon } from "../ui/Icon.tsx";
import { Avatar, Button, Card, Note, Pill, SectionHeader } from "../ui/kit.tsx";
import { badge } from "./inbox.tsx";
import "./workspace.css";

export function WorkspacesPage() {
  usePageTitle(t`团队`);
  useDrawn("workspaces", () => ({ workspaces: workspaces.value.map((w) => w.id), past: pastWorkspaces.value.map((w) => w.id) }));
  return (
    <div class="page">
      <SectionHeader title={t`团队`} />
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
              {n > 0 && <span class="badge">{n}</span>}
              <Icon name="chevron.right" size={14} class="secondary" />
            </a>
          );
        })}
        {pastWorkspaces.value.map((w) => (
          <a class="row secondary" {...link(`/workspaces/${encodeURIComponent(w.id)}`)}>
            <InitialMark text={w.name} />
            <div class="grow ellipsis">{t`${w.name}（已删除）`}</div>
            <Icon name="chevron.right" size={14} />
          </a>
        ))}
      </Card>
      {workspaces.value.length === 0 && <Note text={t`还没有工作区：在 Mac 上的 Janus 新建一个。`} />}
    </div>
  );
}

/** One row per run that waits on the person here: the inbox's entries of the workspace, grouped. */
type Waiting = { run: string; entries: Json[] };

function waitingRows(ws: string): Waiting[] {
  const rows: Waiting[] = [];
  for (const e of inbox.open.value) {
    if (e.workspace !== ws) continue;
    const row = rows.find((r) => r.run === e.run);
    if (row) row.entries.push(e);
    else rows.push({ run: e.run, entries: [e] });
  }
  return rows;
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

  useDrawn("workspace", () => ({
    id,
    name,
    deleted: !ws,
    waiting: waiting.map((w) => ({ run: w.run, entries: w.entries.map((e) => e.key) })),
    records: shown.map((r) => ({ id: r.id, status: r.status, label: runLabel(r)[0] })),
    more: records.more,
  }));

  if (!ws && !past) {
    return (
      <div class="page">
        <Note text={t`没有这个工作区。`} />
      </div>
    );
  }
  return (
    <div class="page wide">
      <div class="ws-identity">
        <h1 class="display ws-name">{name}</h1>
        <div class="secondary ellipsis" title={(ws ?? past).path}>
          {tilde((ws ?? past).path)}
        </div>
        {ws && <Team ws={ws} />}
      </div>
      {!ws && <Note text={t`工作区已删除。留下的任务记录还能查看、继续对话或删除；想在这里提新目标，新建一个工作区。`} />}

      {waiting.length > 0 && (
        <section class="stack">
          <div class="ws-heading">
            <span class="h2">{t`待处理`}</span>
            <span class="h2 fg-amber">{waiting.reduce((n, w) => n + w.entries.length, 0)}</span>
          </div>
          <Card pad={4} class="rows">
            {waiting.map((w) => (
              <WaitingRow w={w} />
            ))}
          </Card>
        </section>
      )}

      {shown.length > 0 && (
        <section class="stack">
          <span class="h2">{t`任务记录`}</span>
          <Card pad={4} class="rows">
            {shown.map((r) => (
              <RunRow r={r} />
            ))}
          </Card>
        </section>
      )}
      {records.more && <More onMore={() => void records.loadMore()} />}
    </div>
  );
}

function Team({ ws }: { ws: Json }) {
  const members: Json[] = ws.members ?? [];
  const lead = role(ws.lead);
  const slots = members.reduce((n, m) => n + (m.role === ws.lead ? 1 : Math.max(1, m.slots ?? 1)), 0);
  const count = slots > members.length ? t`${members.length} 个数字员工（共 ${slots} 个分身）` : t`${members.length} 个数字员工`;
  const leadText = lead ? (lead.name === t`负责人` ? t` · 有负责人` : t` · 负责人：${lead.name}`) : "";
  const names = members.map((m) => role(m.role)?.name).filter(Boolean) as string[];
  return (
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
      <span class="secondary">
        {count}
        {leadText}
      </span>
    </div>
  );
}

function WaitingRow({ w }: { w: Waiting }) {
  const one = w.entries.length === 1 ? w.entries[0] : undefined;
  const [symbol] = one ? badge(one) : ["tray.full.fill"];
  const tone: Tone = w.entries.every((e) => e.canWait) ? "amber" : "red";
  const label = one ? one.label : t`${w.entries.length} 件事`;
  return (
    <a class="row" {...link(`/runs/${encodeURIComponent(w.run)}`)} title={w.entries.map((e) => e.title).join("\n")}>
      <span class={`fg-${tone}`}>
        <Icon name={symbol} />
      </span>
      <span class="grow ellipsis">{goalTitle(w.entries[0]!.goal)}</span>
      <Pill text={label} tone={tone} />
    </a>
  );
}

export function RunRow({ r }: { r: Json }) {
  const waiting = inbox.open.value.some((e) => e.run === r.id);
  const [label, tone] = waiting ? [t`待处理`, "amber" as Tone] : runLabel(r);
  return (
    <a class="row run-row" {...link(`/runs/${encodeURIComponent(r.id)}`)}>
      <span class="grow ellipsis">{goalTitle(r.goal)}</span>
      <span class="caption run-date">{dateTime(r.createdAt ?? 0)}</span>
      <Pill text={label} tone={tone} />
    </a>
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
    <div ref={(x) => {
        el = x;
      }} class="more">
      <Button kind="plain" small onClick={onMore}>
        {t`更多`}
      </Button>
    </div>
  );
}
