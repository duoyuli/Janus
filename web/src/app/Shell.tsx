// The window: on a wide screen the Mac app's frame, the sidebar (inbox, workspaces, chats,
// settings) beside the page; on a phone a top bar with the page's title and back button, and a
// tab bar at the bottom for the four places the sidebar leads to
// (docs/architecture/20261126-web.md#layout).

import type { ComponentChildren } from "preact";
import { t } from "../lib/i18n.ts";
import { link, route } from "../lib/router.ts";
import { chats, inbox, loopOfRun, pastWorkspaces, runs, toast, workingWorkspaces, workspaces } from "../lib/store.ts";
import { connected } from "../lib/stream.ts";
import { backTo, pageActions, pageTitle } from "../lib/title.ts";
import { ACTIVE } from "../lib/status.ts";
import { ConfirmDialog } from "../ui/ConfirmDialog.tsx";
import { Icon } from "../ui/Icon.tsx";
import { waitingIn } from "../pages/workspace.tsx";

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
      <ConfirmDialog />
    </div>
  );
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
        <SidebarHeading title={t`团队`} plus={t`新建工作区…`} to="/workspaces/new" />
        {workspaces.value.map((w) => {
          // Its page, or a run of it (a goal, or one of its Loops' sessions) is open.
          const shown = (r.page === "workspace" && r.id === w.id) || (r.page === "run" && (runs.value.some((x) => x.id === r.id && x.workspace === w.id) || loopOfRun(r.id)?.workspace === w.id));
          return <SidebarItem to={`/workspaces/${encodeURIComponent(w.id)}`} selected={shown} title={w.name} waiting={waitingIn(w.id)} live={workingWorkspaces.value.includes(w.id)} mark={<InitialMark text={w.name} />} />;
        })}
        {pastWorkspaces.value.map((w) => (
          <SidebarItem to={`/workspaces/${encodeURIComponent(w.id)}`} selected={r.page === "workspace" && r.id === w.id} title={t`${w.name}（已删除）`} help={t`已删除的工作区，留下 ${w.runs ?? 0} 个任务记录`} mark={<InitialMark text={w.name} />} dim />
        ))}
        <SidebarHeading title={t`对话`} plus={t`新对话：选一名数字员工单独对话（⌥⌘N）`} to="/chats/new" />
        {chats.runs.value.map((c) => {
          const waiting = inbox.open.value.filter((e) => e.run === c.id).length;
          const member = c.config?.team?.[0]?.name ?? "";
          return (
            <SidebarItem
              to={`/runs/${encodeURIComponent(c.id)}`}
              selected={r.page === "run" && r.id === c.id}
              title={c.title}
              help={[member, c.label?.text].filter(Boolean).join(" · ")}
              waiting={waiting}
              live={ACTIVE.has(c.status)}
              mark={<InitialMark text={member} />}
            />
          );
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

/** A section label of the sidebar, with a quiet plus where something can be started there. */
function SidebarHeading({ title, plus, to }: { title: string; plus: string; to: string }) {
  return (
    <div class="sidebar-heading">
      <span style={{ flex: 1 }}>{title}</span>
      <a class="icon-btn sidebar-plus" aria-label={plus} title={plus} {...link(to)}>
        <Icon name="plus" size={11} />
      </a>
    </div>
  );
}

function SidebarItem({ to, selected, title, help, waiting = 0, live = false, mark, dim }: { to: string; selected: boolean; title: string; help?: string; waiting?: number; live?: boolean; mark: ComponentChildren; dim?: boolean }) {
  return (
    <a class={"sidebar-item" + (selected ? " selected" : "")} aria-current={selected ? "page" : undefined} title={help || title} style={dim ? { opacity: 0.6 } : undefined} {...link(to)}>
      {mark}
      <span class="title">{title}</span>
      {waiting > 0 ? (
        <span class="badge" aria-label={t`${waiting} 件待处理`}>
          {waiting}
        </span>
      ) : (
        live && <span class="live-dot" aria-label={t`进行中`} />
      )}
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
      <div class="side">{!connected.value ? <Icon name="wifi.slash" label={t`正在连接服务…`} class="fg-amber" /> : pageActions.value}</div>
    </header>
  );
}

function TabBar() {
  const r = route.value;
  const n = inbox.open.value.length;
  const tabs: { to: string; icon: string; label: string; on: boolean; badge?: number }[] = [
    { to: "/inbox", icon: "tray", label: t`收件箱`, on: r.page === "inbox", badge: n },
    { to: "/workspaces", icon: "person.2", label: t`团队`, on: ["workspaces", "workspace", "newWorkspace", "workspaceSettings", "brief", "memory", "note", "newLoop", "editLoop"].includes(r.page) },
    { to: "/chats", icon: "bubble.left.and.bubble.right", label: t`对话`, on: r.page === "chats" || r.page === "newChat" },
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
