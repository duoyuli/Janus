// The smaller pages of the first web version: the chats list (the sidebar's, the 「对话」 tab on a
// phone), a run's page while it is being built, this browser's settings, and a path that names
// nothing.

import type { Json } from "../lib/api.ts";
import { chooseAppearance, appearance, type Appearance } from "../lib/appearance.ts";
import { dateTime } from "../lib/format.ts";
import { choice, choose, t, type Choice } from "../lib/i18n.ts";
import { link } from "../lib/router.ts";
import { ACTIVE, runLabel } from "../lib/status.ts";
import { attention, chats, inbox, meta, runs } from "../lib/store.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { chatTitle, InitialMark } from "../app/Shell.tsx";
import { Card, Note, Pill, SectionHeader } from "../ui/kit.tsx";
import { More } from "./workspace.tsx";
import { PhoneAccess } from "./pair.tsx";
import { goalTitle } from "../lib/format.ts";

export function ChatsPage() {
  usePageTitle(t`对话`);
  const list = chats.runs.value;
  useDrawn("chats", () => ({ chats: list.map((c) => ({ id: c.id, title: chatTitle(c) })), more: chats.more }));
  return (
    <div class="page">
      <SectionHeader title={t`对话`} />
      {list.length > 0 && (
        <Card pad={4} class="rows">
          {list.map((c) => {
            const waiting = inbox.open.value.filter((e) => e.run === c.id).length;
            const [label, tone] = ACTIVE.has(c.status) ? [t`会话中`, "blue" as const] : [t`已结束`, "gray" as const];
            return (
              <a class="row" {...link(`/runs/${encodeURIComponent(c.id)}`)}>
                <InitialMark text={c.config?.team?.[0]?.name ?? ""} />
                <div class="grow">
                  <div class="ellipsis">{chatTitle(c)}</div>
                  <div class="caption ellipsis">{[c.config?.team?.[0]?.name, dateTime(c.createdAt ?? 0)].filter(Boolean).join(" · ")}</div>
                </div>
                {waiting > 0 && <span class="badge">{waiting}</span>}
                <Pill text={label} tone={tone} />
              </a>
            );
          })}
        </Card>
      )}
      {chats.more && <More onMore={() => void chats.loadMore()} />}
    </div>
  );
}

/** A run's page: what it is and how it stands, until the full page (board, sessions, review) is in. */
export function RunPage({ id }: { id: string }) {
  const r: Json | undefined = [...runs.value, ...attention.value, ...chats.runs.value].find((x) => x.id === id);
  const title = r ? (r.kind === "chat" ? chatTitle(r) : goalTitle(r.goal)) : "";
  usePageTitle(title, r?.kind === "chat" ? "/chats" : r?.config?.workspace?.id ? `/workspaces/${encodeURIComponent(r.config.workspace.id)}` : "/inbox");
  useDrawn("run", () => ({ id, status: r?.status ?? null }));
  if (!r) return <div class="page" />;
  const [label, tone] = runLabel(r);
  return (
    <div class="page">
      <div class="section-header">
        <h1 class="display">{title}</h1>
        <Pill text={label} tone={tone} />
      </div>
      <Note text={t`这个页面在网页上还没有做完：先在 Mac 上的 Janus 打开它。`} />
    </div>
  );
}

function Choices<T extends string>({ name, value, options, onChange }: { name: string; value: T; options: [T, string][]; onChange: (v: T) => void }) {
  return (
    <div class="choices" role="radiogroup" aria-label={name}>
      {options.map(([v, label]) => (
        <button type="button" role="radio" aria-checked={v === value} class={"choice" + (v === value ? " on" : "")} onClick={() => onChange(v)}>
          {label}
        </button>
      ))}
    </div>
  );
}

export function SettingsPage() {
  usePageTitle(t`设置`);
  useDrawn("settings", () => ({ language: choice.value, appearance: appearance.value }));
  const languages: [Choice, string][] = [
    ["system", t`跟随系统`],
    ["zh-Hans", "简体中文"],
    ["en", "English"],
  ];
  const appearances: [Appearance, string][] = [
    ["system", t`跟随系统`],
    ["light", t`浅色`],
    ["dark", t`深色`],
  ];
  return (
    <div class="page">
      <SectionHeader title={t`设置`} />
      <Card>
        <div class="setting">
          <span class="h2">{t`语言`}</span>
          <Choices name={t`语言`} value={choice.value} options={languages} onChange={choose} />
        </div>
        <div class="setting">
          <span class="h2">{t`外观`}</span>
          <Choices name={t`外观`} value={appearance.value} options={appearances} onChange={chooseAppearance} />
        </div>
      </Card>
      <PhoneAccess />
      {meta.value?.version && <div class="caption">{t`Janus 服务 ${meta.value.version}`}</div>}
    </div>
  );
}

export function MissingPage() {
  usePageTitle("");
  return (
    <div class="page">
      <Note text={t`没有这个页面。`} />
    </div>
  );
}
