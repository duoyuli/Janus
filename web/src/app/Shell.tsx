// The window: on a wide screen the Mac app's frame, the sidebar (inbox, workspaces, chats,
// settings) beside the page; on a phone a top bar with the page's title and back button, and a
// tab bar at the bottom for the four places the sidebar leads to
// (docs/architecture/20261126-web.md#layout).

import type { ComponentChildren } from "preact";
import { t } from "../lib/i18n.ts";
import { link, route } from "../lib/router.ts";
import { chats, chatAttention, inbox, pastWorkspaces, runs, toast, workspaces } from "../lib/store.ts";
import { connected } from "../lib/stream.ts";
import { backTo, pageTitle } from "../lib/title.ts";
import { goalTitle } from "../lib/format.ts";
import { ACTIVE } from "../lib/status.ts";
import type { Json } from "../lib/api.ts";
import { Icon } from "../ui/Icon.tsx";

export function Shell({ children }: { children: ComponentChildren }) {
  return (
    <div class="shell">
      <Sidebar />
      <div class="main">
        <TopBar />
        <main id="content">{children}</main>
        {toast.value && (
          <div class="toast" role="status">
            {toast.value}
          </div>
        )}
      </div>
      <TabBar />
    </div>
  );
}

/** How many open inbox entries belong to workspace `id`. */
export function waitingIn(id: string): number {
  return inbox.open.value.filter((e) => e.workspace === id).length;
}

/** Whether a run row is one of workspace `ws`'s: by the workspace it recorded, or by its folder. */
export function belongs(r: Json, ws: Json): boolean {
  const id = r?.config?.workspace?.id;
  return id ? id === ws.id : r.cwd === ws.path;
}

/** A chat is listed by its first message; before there is one, by who it is with. */
export function chatTitle(c: Json): string {
  return c.goal ? goalTitle(c.goal) : t`和 ${c.config?.team?.[0]?.name ?? ""} 的新对话`;
}

function Sidebar() {
  const r = route.value;
  const n = inbox.open.value.length;
  return (
    <nav class="sidebar" aria-label={t`侧栏`}>
      <div style={{ paddingTop: "10px" }}>
        <SidebarItem to="/inbox" selected={r.page === "inbox"} title={t`收件箱`} waiting={n} mark={<Icon name="tray" />} />
      </div>
      <div class="sidebar-scroll">
        <div class="sidebar-heading">{t`团队`}</div>
        {workspaces.value.map((w) => {
          const selected = (r.page === "workspace" && r.id === w.id) || (r.page === "run" && runs.value.some((x) => x.id === r.id && belongs(x, w)));
          const live = runs.value.some((x) => belongs(x, w) && ACTIVE.has(x.status));
          return <SidebarItem to={`/workspaces/${encodeURIComponent(w.id)}`} selected={selected} title={w.name} waiting={waitingIn(w.id)} live={live} mark={<InitialMark text={w.name} />} />;
        })}
        {pastWorkspaces.value.map((w) => (
          <SidebarItem to={`/workspaces/${encodeURIComponent(w.id)}`} selected={r.page === "workspace" && r.id === w.id} title={t`${w.name}（已删除）`} mark={<InitialMark text={w.name} />} dim />
        ))}
        <div class="sidebar-heading">
          <a {...link("/chats")}>{t`对话`}</a>
        </div>
        {chats.runs.value.map((c) => {
          const waiting = inbox.open.value.filter((e) => e.run === c.id).length;
          const live = ACTIVE.has(c.status) || chatAttention.value.some((x) => x.id === c.id && ACTIVE.has(x.status));
          return <SidebarItem to={`/runs/${encodeURIComponent(c.id)}`} selected={r.page === "run" && r.id === c.id} title={chatTitle(c)} waiting={waiting} live={live} mark={<InitialMark text={c.config?.team?.[0]?.name ?? ""} />} />;
        })}
        {chats.more && (
          <button type="button" class="btn btn-plain btn-small" style={{ margin: "6px 20px" }} onClick={() => void chats.loadMore()}>
            {t`更多`}
          </button>
        )}
      </div>
      <div class="sidebar-foot">
        <span class={`dot ${connected.value ? "tone-green" : "tone-amber"}`} aria-hidden="true" />
        <span style={{ flex: 1 }}>{connected.value ? t`服务已连接` : t`正在连接服务…`}</span>
        <a class="icon-btn" aria-label={t`设置`} title={t`设置`} {...link("/settings")}>
          <Icon name="gearshape" />
        </a>
      </div>
    </nav>
  );
}

function SidebarItem({ to, selected, title, waiting = 0, live = false, mark, dim }: { to: string; selected: boolean; title: string; waiting?: number; live?: boolean; mark: ComponentChildren; dim?: boolean }) {
  return (
    <a class={"sidebar-item" + (selected ? " selected" : "")} aria-current={selected ? "page" : undefined} title={title} style={dim ? { opacity: 0.6 } : undefined} {...link(to)}>
      {mark}
      <span class="title">{title}</span>
      {live && <span class="live-dot" aria-label={t`进行中`} />}
      {waiting > 0 && <span class="badge" aria-label={t`${waiting} 件待处理`}>{waiting}</span>}
    </a>
  );
}

/** A workspace's or chat's first letter in a small outlined square (Mac: `InitialMark`). */
export function InitialMark({ text }: { text: string }) {
  return (
    <span class="initial-mark" aria-hidden="true">
      {([...(text ?? "")][0] ?? "").toUpperCase()}
    </span>
  );
}

function TopBar() {
  const back = backTo.value;
  return (
    <header class="topbar">
      <div class="side">
        {back && (
          <a class="icon-btn" aria-label={t`返回`} {...link(back)}>
            <Icon name="chevron.left" size={20} />
          </a>
        )}
      </div>
      <div class="title">{pageTitle.value}</div>
      <div class="side">{!connected.value && <Icon name="wifi.slash" label={t`正在连接服务…`} class="fg-amber" />}</div>
    </header>
  );
}

function TabBar() {
  const r = route.value;
  const n = inbox.open.value.length;
  const tabs: { to: string; icon: string; label: string; on: boolean; badge?: number }[] = [
    { to: "/inbox", icon: "tray", label: t`收件箱`, on: r.page === "inbox", badge: n },
    { to: "/workspaces", icon: "person.2", label: t`团队`, on: r.page === "workspaces" || r.page === "workspace" },
    { to: "/chats", icon: "bubble.left.and.bubble.right", label: t`对话`, on: r.page === "chats" },
    { to: "/settings", icon: "gearshape", label: t`设置`, on: r.page === "settings" },
  ];
  return (
    <nav class="tabbar" aria-label={t`页签`}>
      {tabs.map((tab) => (
        <a class={"tab" + (tab.on ? " selected" : "")} aria-current={tab.on ? "page" : undefined} {...link(tab.to)}>
          <Icon name={tab.icon} size={22} />
          <span>{tab.label}</span>
          {!!tab.badge && <span class="badge">{tab.badge}</span>}
        </a>
      ))}
    </nav>
  );
}
