// The chats: the list (the sidebar's on a wide screen, the 「对话」 tab on a phone), and starting
// one (Mac: UI/ChatsView.swift): one card per digital employee, by group, two to a row on a wide
// screen; a tap picks one and 「开始对话」 opens the chat with it. A chat is a run of its own kind
// (docs/domains/20261120-chats.md#chat_app).

import { useSignal } from "@preact/signals";
import type { Json } from "../lib/api.ts";
import { dateTime } from "../lib/format.ts";
import { t } from "../lib/i18n.ts";
import { load, save } from "../lib/prefs.ts";
import { go, link } from "../lib/router.ts";
import { chats, createChat, deleteRun, inbox, role, roleGroups, roles } from "../lib/store.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { InitialMark } from "../app/Shell.tsx";
import { Icon } from "../ui/Icon.tsx";
import { Avatar, Button, Card, Menu, Note, Pill, Spinner } from "../ui/kit.tsx";
import { More } from "./workspace.tsx";
import "./chats.css";

export function ChatsPage() {
  usePageTitle(t`对话`);
  const list = chats.runs.value;
  useDrawn("chats", () => ({ chats: list.map((c) => ({ id: c.id, title: c.title, label: c.label?.text })), more: chats.more }));
  return (
    <div class="page">
      <div class="section-header">
        <h1 class="display">{t`对话`}</h1>
        <span class="grow" />
        <a class="btn btn-primary btn-small" {...link("/chats/new")}>
          {t`新对话`}
        </a>
      </div>
      {list.length > 0 && (
        <Card pad={4} class="rows">
          {list.map((c) => {
            const waiting = inbox.open.value.filter((e) => e.run === c.id).length;
            const member = c.config?.team?.[0]?.name ?? "";
            return (
              <div class="row chat-row">
                <a class="grow chat-link" {...link(`/runs/${encodeURIComponent(c.id)}`)}>
                  <InitialMark text={member} />
                  <div class="grow">
                    <div class="ellipsis">{c.title}</div>
                    <div class="caption ellipsis">{[member, dateTime(c.createdAt ?? 0)].filter(Boolean).join(" · ")}</div>
                  </div>
                  {waiting > 0 && <span class="badge">{waiting}</span>}
                  <Pill text={c.label?.text ?? ""} tone={c.label?.tone} />
                </a>
                <Menu icon="ellipsis" label={t`更多`} items={[{ label: t`删除对话…`, onSelect: () => void deleteRun(c) }]} />
              </div>
            );
          })}
        </Card>
      )}
      {chats.more && <More onMore={() => void chats.loadMore()} />}
    </div>
  );
}

/** The employee a new chat is with: the one picked last if it can still run, otherwise the first that can. */
function chosen(picked: string): Json | undefined {
  const ready = roles.value.filter((r) => !r.unavailable);
  return ready.find((r) => r.id === picked) ?? ready[0];
}

export function NewChatPage() {
  usePageTitle(t`新对话`, "/chats");
  const picked = useSignal(load<string>("chatRole", ""));
  const starting = useSignal<string | null>(null);
  const pick = chosen(picked.value);
  const groups = roleGroups.value;
  const start = async (r: Json) => {
    if (r.unavailable || starting.value) return;
    picked.value = r.id;
    save("chatRole", r.id);
    starting.value = r.id;
    const id = await createChat(r.id);
    starting.value = null;
    if (id) go(`/runs/${encodeURIComponent(id)}`);
  };
  useDrawn("newChat", () => ({ groups: groups.map((g) => ({ title: g.title, roles: g.roles })), chosen: pick?.id ?? null }));
  return (
    <div class="page">
      <div class="stack">
        <h1 class="display new-chat-title">{t`新对话`}</h1>
        <div class="secondary">{t`选一名数字员工一对一聊。它带着自己的指令和资源库，在专属目录里干活，不带团队和看板。`}</div>
      </div>
      {roles.value.length === 0 ? (
        <div class="ws-problem">
          <Note text={t`还没有数字员工。`} tone="amber" />
          <a class="btn btn-bordered btn-small" {...link("/settings")}>
            {t`打开数字员工设置`}
          </a>
        </div>
      ) : (
        <>
          {groups.map((g) => (
            <section class="stack">
              {groups.length > 1 && <span class="h2">{g.title}</span>}
              <div class="role-grid">
                {g.roles.map((id: string) => {
                  const r = role(id);
                  return r && <RoleChoice r={r} selected={pick?.id === r.id} starting={starting.value === r.id} onPick={() => (picked.value = r.id)} onStart={() => void start(r)} />;
                })}
              </div>
            </section>
          ))}
          <div class="new-chat-bar">
            <span class="caption">{t`双击卡片直接开始`}</span>
            <span class="grow" />
            <Button kind="primary" disabled={!pick || !!starting.value} title={pick ? t`开聊，你先说（↩）` : t`没有可以运行的数字员工`} onClick={() => pick && void start(pick)}>
              {starting.value ? t`正在打开…` : pick ? t`和 ${pick.name} 开始对话` : t`开始对话`}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

/** One digital employee to chat with: who it is, what it does, and why it cannot run if it can't. */
function RoleChoice({ r, selected, starting, onPick, onStart }: { r: Json; selected: boolean; starting: boolean; onPick: () => void; onStart: () => void }) {
  const why: string | null = r.unavailable ?? null;
  return (
    <button
      type="button"
      class={"role-choice" + (selected ? " selected" : "")}
      aria-pressed={selected}
      disabled={!!why}
      title={why ? t`${why}：去「设置 › 数字员工」处理` : r.setupLine}
      onClick={onPick}
      onDblClick={onStart}
    >
      <Avatar role={r} size={36} />
      <span class="role-choice-body">
        <span class="role-choice-head">
          <span class="h2 ellipsis grow">{r.name}</span>
          {starting ? <Spinner /> : selected && <Icon name="checkmark.circle.fill" class="fg-blue" />}
        </span>
        <span class="secondary clamp3">{r.desc || t`没有写职责`}</span>
        {why && <span class="caption fg-amber ellipsis">{why}</span>}
      </span>
    </button>
  );
}
