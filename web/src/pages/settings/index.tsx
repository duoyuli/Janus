// Settings: a pane each for general preferences, digital employees (the Agents they run on and the
// employees themselves), the resource library, the usage bill and phone access, as the Mac app's
// settings panel has them (UI/Settings/SettingsView.swift). On a desktop the panes are listed
// beside the one shown; on a phone the list is a page of its own and each pane opens over it, and
// an editor fills the screen. Updates and macOS permissions belong to the Mac app.

import type { ComponentChildren } from "preact";
import { signal } from "@preact/signals";
import { appearance, chooseAppearance, type Appearance } from "../../lib/appearance.ts";
import { choice, choose, t, type Choice } from "../../lib/i18n.ts";
import { load, save } from "../../lib/prefs.ts";
import { link } from "../../lib/router.ts";
import { connections, meta, roleGroups, roles } from "../../lib/store.ts";
import { useDrawn } from "../../lib/testing.ts";
import { usePageTitle } from "../../lib/title.ts";
import { Card, Note, Pill } from "../../ui/kit.tsx";
import { Icon } from "../../ui/Icon.tsx";
import { Segmented } from "../../ui/form.tsx";
import { PhoneAccess } from "../pair.tsx";
import { AgentsSection, ConnectionEditor } from "./agents.tsx";
import { adapters, library, loadAdapters, loadLibrary, loadRoleStats, useFollow } from "./data.ts";
import { LibraryPane, McpEditor, SkillEditor, type LibraryKind } from "./library.tsx";
import { RoleEditor, RolesSection } from "./roles.tsx";
import { UsagePane } from "./usage.tsx";
import "./settings.css";

export type SendKey = "commandReturn" | "return";
/** How the composer sends a message on this browser (Mac: `SendKeySetting`). */
export const sendKey = signal<SendKey>(load<SendKey>("sendKey", "commandReturn"));

const TABS = ["general", "agents", "library", "usage", "devices"] as const;
type Tab = (typeof TABS)[number];

function tabTitle(tab: Tab): string {
  switch (tab) {
    case "general":
      return t`通用`;
    case "agents":
      return t`数字员工`;
    case "library":
      return t`资源库`;
    case "usage":
      return t`用量账单`;
    default:
      return t`手机访问`;
  }
}

const ICONS: Record<Tab, string> = { general: "gearshape", agents: "person.2", library: "books.vertical", usage: "list.bullet.rectangle", devices: "iphone" };

/** `/settings[/<tab>[/<sub>[/<id>]]]`. */
export function SettingsPage({ tab, sub, id }: { tab: string; sub?: string; id?: string }) {
  const shown = (TABS as readonly string[]).includes(tab) ? (tab as Tab) : null;
  const editor = editorOf(shown, sub, id);
  usePageTitle(shown ? tabTitle(shown) : t`设置`, editor ? `/settings/${shown}${shown === "library" && sub ? `/${sub}` : ""}` : shown ? "/settings" : null);
  useFollow(() => void loadAdapters(), ["adapters"]);
  useFollow(() => void loadRoleStats(), ["connection"]);
  const updates = (library.value?.sources ?? []).filter((s: { updateAvailable?: boolean }) => s.updateAvailable).length;
  useDrawn("settings", () => ({ tab: shown, sub: sub ?? null, id: id ?? null, adapters: adapters.value.map((a) => ({ id: a.id, state: a.line?.state })) }));
  return (
    <div class={"page settings" + (shown ? " has-pane" : "") + (editor ? " has-editor" : "")}>
      <nav class="settings-nav" aria-label={t`设置`}>
        <h1 class="display">{t`设置`}</h1>
        {TABS.map((tb) => (
          <a class={"settings-nav-item" + (tb === (shown ?? "general") ? " selected" : "")} aria-current={tb === shown ? "page" : undefined} {...link(`/settings/${tb}`)}>
            <Icon name={ICONS[tb]} />
            <span class="grow">{tabTitle(tb)}</span>
            {tb === "library" && updates > 0 && <Pill text={String(updates)} tone="amber" />}
            <Icon name="chevron.right" size={11} class="phone-only secondary" />
          </a>
        ))}
        {meta.value?.version && <div class="caption">{t`Janus 服务 ${meta.value.version}`}</div>}
      </nav>
      <div class="settings-main">{editor ?? <Pane tab={shown ?? "general"} sub={sub} />}</div>
    </div>
  );
}

function editorOf(tab: Tab | null, sub?: string, id?: string): ComponentChildren | null {
  if (tab === "agents" && sub === "agent" && id) return <ConnectionEditor id={id} key={`a-${id}`} />;
  if (tab === "agents" && sub === "role" && id) return <RoleEditor id={id} key={`r-${id}`} />;
  if (tab === "library" && sub === "skills" && id) return <SkillEditor id={id} key={`s-${id}`} />;
  if (tab === "library" && sub === "servers" && id) return <McpEditor id={id} key={`m-${id}`} />;
  return null;
}

function Pane({ tab, sub }: { tab: Tab; sub?: string }) {
  switch (tab) {
    case "agents":
      return <AgentsPane />;
    case "library":
      return <LibraryPane kind={(sub === "servers" ? "servers" : "skills") as LibraryKind} />;
    case "usage":
      return <UsagePane />;
    case "devices":
      return (
        <div class="settings-pane">
          <h2 class="display title2">{t`手机访问`}</h2>
          <PhoneAccess />
        </div>
      );
    default:
      return <General />;
  }
}

/** 数字员工: the Agents, then the digital employees. */
function AgentsPane() {
  useDrawn("agents", () => ({
    connections: connections.value.map((c) => ({ id: c.id, summary: c.summary, checks: (c.checks ?? []).map((s: { state: string }) => s.state) })),
    roles: roles.value.map((r) => ({ id: r.id, line: r.line, modelLine: r.modelLine, avatar: r.avatar })),
    groups: roleGroups.value.map((g) => ({ title: g.title, roles: g.roles })),
  }));
  return (
    <div class="settings-pane">
      <AgentsSection />
      <RolesSection />
    </div>
  );
}

function Setting({ title, detail, children }: { title: string; detail: string; children: ComponentChildren }) {
  return (
    <div class="setting">
      <div class="grow">
        <div class="h2">{title}</div>
        <div class="secondary">{detail}</div>
      </div>
      {children}
    </div>
  );
}

/** 通用: this browser's language, appearance and send key; the rest of the Mac's general pane
 * (updates, macOS permissions, what of the home folder agents see) is the Mac app's. */
function General() {
  useFollow(() => void loadLibrary(), ["library"]);
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
  const keys: [SendKey, string][] = [
    ["commandReturn", t`⌘↩ 发送`],
    ["return", t`↩ 发送`],
  ];
  useDrawn("general", () => ({ language: choice.value, appearance: appearance.value, sendKey: sendKey.value }));
  return (
    <div class="settings-pane">
      <h2 class="display title2">{t`通用`}</h2>
      <Card>
        <Setting title={t`语言`} detail={t`Janus 的界面语言`}>
          <Segmented label={t`语言`} value={choice.value} options={languages} onChange={choose} />
        </Setting>
        <Setting title={t`外观`} detail={t`Janus 的界面外观`}>
          <Segmented label={t`外观`} value={appearance.value} options={appearances} onChange={chooseAppearance} />
        </Setting>
        <Setting title={t`发送消息`} detail={t`输入框里发送消息的按键。${sendKey.value === "commandReturn" ? t`现在 ↩ 换行。` : t`现在 ⇧↩ 换行，⌘↩ 也能发送。`}`}>
          <Segmented
            label={t`发送消息`}
            value={sendKey.value}
            options={keys}
            onChange={(k) => {
              sendKey.value = k;
              save("sendKey", k);
            }}
          />
        </Setting>
      </Card>
      <h3 class="display title3">{t`更新`}</h3>
      <Note text={t`Web 版随 Mac 上的 Janus 一起更新：在 Mac 上的 Janus 设置 › 通用 › 更新。`} />
      <h3 class="display title3">{t`权限`}</h3>
      <Note text={t`macOS 权限和本机环境只能在 Mac 上的 Janus 设置：它们决定 Agent 能用到这台 Mac 的哪些东西。`} />
    </div>
  );
}
