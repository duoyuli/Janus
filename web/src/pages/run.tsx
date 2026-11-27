// A run's page. A chat is one conversation with who it is with above it (the Mac app's ChatsView);
// a Loop's session and a team memory consolidation have pages of their own (loops.tsx, memory.tsx)
// with the conversation in them (LoopView's and MemoryView's ChatCard), and a Loop's requests
// beside it (`InboxSection`). A goal's page (board, sessions, review) is the run page's own; until
// it is in, a goal shows what it is and how it stands, and opens one of its conversations by key.

import { useEffect, useMemo, useState } from "preact/hooks";
import { api, type Json } from "../lib/api.ts";
import { joinList, tilde } from "../lib/format.ts";
import { t } from "../lib/i18n.ts";
import { connections, loadSetup, show } from "../lib/store.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { Avatar, Button, Note, Pill } from "../ui/kit.tsx";
import { who } from "../session/Items.tsx";
import { RunModel } from "../session/model.ts";
import { SessionPanel } from "../session/SessionPanel.tsx";
import { RequestList } from "../session/Requests.tsx";
import { LoopPage } from "./loops.tsx";
import { DreamPage } from "./memory.tsx";
import { GoalPage } from "../run/RunPage.tsx";
import { runSub } from "../run/nav.ts";
import "./run.css";

export function RunPage({ id, sub }: { id: string; sub: readonly string[] }) {
  const run = useMemo(() => new RunModel(id), [id]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    run.start().catch((e) => setError(e instanceof Error ? e.message : String(e)));
    return () => run.stop();
  }, [run]);
  const row = run.run.value;
  const v = run.view.value;
  useDrawn("run", () => ({ id, kind: row?.kind ?? null, title: v?.title ?? "", status: v?.run?.status ?? null }));
  if (error) return <div class="page"><Note text={error} tone="red" /></div>;
  if (!row || !v) return <div class="page" />;
  const page = { ...v.run, id, title: v.title };
  switch (row.kind) {
    case "goal":
      return <GoalPage run={run} sub={runSub(sub)} />;
    case "loop":
      return <LoopPage run={page} session={<SessionPanel run={run} sessionKey="agent:lead" title={v.title} />} asks={<RequestList run={run} />} />;
    case "dream":
      return <DreamPage run={page} session={<div class="dream-session"><SessionPanel run={run} sessionKey="agent:lead" title={who(run, "lead").name} /></div>} />;
    default:
      return <ConversationPage run={run} />;
  }
}

/** A chat: who it is with and where it works, a proposal of digital employees, then the conversation. */
function ConversationPage({ run }: { run: RunModel }) {
  const v = run.view.value;
  const row = run.run.value ?? {};
  const me = who(run, "lead");
  usePageTitle(v?.title ?? me.name, "/chats");
  const proposal = useProposal(run, true);
  const [sheet, setSheet] = useState(false);
  useDrawn("run", () => ({ id: run.id, kind: "chat", title: v?.title ?? "", status: v?.run?.status ?? row.status, proposal: proposal?.proposal ?? null }));
  const m = (row.config?.team ?? []).find((x: Json) => x.id === "lead") ?? {};
  return (
    <div class="page conversation-page">
      {(
        <div class="conversation-who">
          <Avatar role={me.role} size={34} />
          <div class="grow" style={{ minWidth: 0 }}>
            <div class="display title3">{me.name}</div>
            <div class="caption ellipsis">{[m.runtime ?? m.connection, me.model, m.permission].filter(Boolean).join(" · ")}</div>
          </div>
          <span class="caption ellipsis conversation-cwd" title={t`在 Finder 中显示工作目录（对话专用，随对话删除）`}>
            {tilde(v?.run?.cwd ?? "")}
          </span>
        </div>
      )}
      {v?.run?.status === "failed" && v?.run?.error && <Note text={t`会话已结束：${v.run.error}`} tone="amber" />}
      {proposal && (
        <div class="proposal-banner">
          <div class="grow">
            <div class="strong">{t`${me.name} 提议了 ${proposal.roles.length} 位数字员工`}</div>
            <div class="caption clamp2">{joinList(proposal.roles.map((r: Json) => r.name))}</div>
          </div>
          <Button
            onClick={() =>
              void api.post(`${run.path}/proposal/decline`, { proposal: proposal.proposal }).catch((e) => show(e instanceof Error ? e.message : String(e)))
            }
          >
            {t`不用了`}
          </Button>
          <Button kind="primary" onClick={() => setSheet(true)}>
            {t`查看并创建…`}
          </Button>
        </div>
      )}
      {sheet && proposal && <ProposalSheet run={run} proposal={proposal} onClose={() => setSheet(false)} />}
      <div class="conversation-body">{v && <SessionPanel run={run} sessionKey="agent:lead" title={me.name} />}</div>
    </div>
  );
}

/** A chat's open proposal of digital employees, read again when the chat proposes or the person acts on one. */
function useProposal(run: RunModel, chat: boolean): Json | null {
  const [p, setP] = useState<Json | null>(null);
  const marks = run.events.value.filter((e) => ["roles.proposed", "roles.created", "roles.declined"].includes(e.type)).length;
  useEffect(() => {
    if (!chat) return;
    api.get(`${run.path}/proposal`).then(setP, () => {});
  }, [chat, marks]);
  return p;
}

/** The digital employees a chat proposed, to create: each with its duty and instructions, ticked
 * or not (those whose name is taken start unticked), and one agent for the batch (`ProposalSheet`). */
function ProposalSheet({ run, proposal, onClose }: { run: RunModel; proposal: Json; onClose: () => void }) {
  const roles: Json[] = proposal.roles ?? [];
  const ok = connections.value.filter((c) => c.phase === "ok");
  const [picked, setPicked] = useState<Set<number>>(new Set(roles.filter((r) => !r.existing).map((r) => r.n)));
  const [connection, setConnection] = useState<string>(ok[0]?.id ?? "");
  const [open, setOpen] = useState<Set<number>>(new Set());
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const groups = [...new Set(roles.map((r) => r.group ?? ""))].sort((a, b) => a.localeCompare(b));
  const create = async () => {
    setCreating(true);
    try {
      const made = await api.post(`${run.path}/proposal`, { proposal: proposal.proposal, picks: roles.map((r) => r.n).filter((n) => picked.has(n)), connection });
      await loadSetup();
      onClose();
      show(t`已创建 ${(made ?? []).length} 位数字员工，在「设置 › 数字员工」里可以改`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setCreating(false);
    }
  };
  return (
    <div class="sheet-backdrop" role="dialog" aria-modal="true" aria-label={t`创建数字员工`}>
      <div class="sheet">
        <div class="sheet-head">
          <div class="display title2">{t`创建数字员工`}</div>
          <div class="secondary">{t`对话里提议的数字员工。看过指令，勾选要的人，选好由哪个 Agent 来做。`}</div>
        </div>
        <div class="sheet-body">
          {error && <Note text={error} tone="red" />}
          {groups.map((g) => (
            <section>
              <h3>{g || t`未分组`}</h3>
              {roles
                .filter((r) => (r.group ?? "") === g)
                .map((r) => (
                  <div class="proposed">
                    <div class="proposed-row">
                      <input
                        type="checkbox"
                        aria-label={t`创建 ${r.name}`}
                        checked={picked.has(r.n)}
                        onChange={(e) => {
                          const next = new Set(picked);
                          if ((e.target as HTMLInputElement).checked) next.add(r.n);
                          else next.delete(r.n);
                          setPicked(next);
                        }}
                      />
                      <Avatar role={{ name: r.name, init: r.init, color: r.color }} size={26} />
                      <div class="grow">
                        <div>
                          <span class="strong">{r.name}</span> {r.lead && <Pill text={t`负责人`} tone="violet" />} {r.existing && <Pill text={t`已有同名`} tone="amber" />}
                        </div>
                        <div class="caption clamp2">{r.desc || t`没有写职责`}</div>
                      </div>
                      <Button
                        kind="plain"
                        title={t`创建前看看它的指令`}
                        onClick={() => {
                          const next = new Set(open);
                          if (next.has(r.n)) next.delete(r.n);
                          else next.add(r.n);
                          setOpen(next);
                        }}
                      >
                        {t`指令`}
                      </Button>
                    </div>
                    {open.has(r.n) && <pre class="proposed-prompt">{r.prompt}</pre>}
                  </div>
                ))}
            </section>
          ))}
        </div>
        <div class="sheet-foot">
          <span class="secondary">{t`由谁来做`}</span>
          <select value={connection} title={t`这一批都用这个 Agent 的默认模型和权限，之后可以逐个改`} onChange={(e) => setConnection((e.target as HTMLSelectElement).value)}>
            <option value="">{t`稍后再选`}</option>
            {ok.map((c) => (
              <option value={c.id}>{c.name}</option>
            ))}
          </select>
          <span class="grow" />
          <Button onClick={onClose}>{t`取消`}</Button>
          <Button kind="primary" disabled={picked.size === 0 || creating} onClick={() => void create()}>
            {creating ? t`正在保存…` : t`创建 ${picked.size} 位`}
          </Button>
        </div>
      </div>
    </div>
  );
}
