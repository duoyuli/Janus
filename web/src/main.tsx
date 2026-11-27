// The web client's entry: the frame, the page the path names, and the service's state read once
// and then followed on the global stream (docs/architecture/20261126-web.md).

import { render } from "preact";
import { unpaired } from "./lib/api.ts";
import "./lib/appearance.ts";
import { lang } from "./lib/i18n.ts";
import { route } from "./lib/router.ts";
import { boot, readAll } from "./lib/store.ts";
import { follow, stop } from "./lib/stream.ts";
import "./lib/testing.ts";
import { Shell } from "./app/Shell.tsx";
import { ChatsPage, NewChatPage } from "./pages/chats.tsx";
import { InboxPage } from "./pages/inbox.tsx";
import { LoopEditor } from "./pages/loops.tsx";
import { MemoryPage, NotePage } from "./pages/memory.tsx";
import { MissingPage, RunPage } from "./pages/misc.tsx";
import { SettingsPage } from "./pages/settings/index.tsx";
import { PairPage, UnpairedPage } from "./pages/pair.tsx";
import { BriefEditor, WorkspaceEditor } from "./pages/workspace-edit.tsx";
import { WorkspacePage, WorkspacesPage } from "./pages/workspace.tsx";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/pages.css";
import { effect } from "@preact/signals";
import icon from "../../docs/assets/icon.png";

function Page() {
  const r = route.value;
  switch (r.page) {
    case "inbox":
      return <InboxPage />;
    case "workspaces":
      return <WorkspacesPage />;
    case "workspace":
      return <WorkspacePage id={r.id} key={r.id} />;
    case "newWorkspace":
      return <WorkspaceEditor key="new" />;
    case "workspaceSettings":
      return <WorkspaceEditor id={r.id} key={r.id} />;
    case "brief":
      return <BriefEditor id={r.id} key={r.id} />;
    case "memory":
      return <MemoryPage id={r.id} key={r.id} />;
    case "note":
      return <NotePage id={r.id} note={r.note} key={r.note} />;
    case "newLoop":
      return <LoopEditor ws={r.id} key={`new-${r.id}`} />;
    case "editLoop":
      return <LoopEditor loopId={r.loop} key={r.loop} />;
    case "run":
      return <RunPage id={r.id} key={r.id} />;
    case "chats":
      return <ChatsPage />;
    case "newChat":
      return <NewChatPage />;
    case "settings":
      return <SettingsPage tab={r.tab} sub={r.sub} id={r.id} key={`${r.tab}/${r.sub ?? ""}/${r.id ?? ""}`} />;
    default:
      return <MissingPage />;
  }
}

function App() {
  const r = route.value;
  if (r.page === "pair") return <PairPage code={r.code} />;
  if (unpaired.value) return <UnpairedPage />;
  return (
    <Shell>
      <Page />
    </Shell>
  );
}

// Not paired (any request came back 401): stop following until the pairing page pairs it again.
effect(() => {
  if (unpaired.value) stop();
});

// What the service words follows the interface language: everything it wrote is read again in
// the new one (the setup, the lists, the Loops, the team memory shown, the inbox).
let first = true;
effect(() => {
  document.documentElement.lang = lang.value === "en" ? "en" : "zh-Hans";
  if (!first) void readAll();
  first = false;
});

// The app's icon (docs/assets/icon.png, drawn by frontend/scripts/make-icon.swift) for the tab and the home screen.
for (const rel of ["icon", "apple-touch-icon"]) {
  const l = document.createElement("link");
  l.rel = rel;
  l.href = icon;
  document.head.append(l);
}

render(<App />, document.getElementById("app")!);
if (route.value.page !== "pair") {
  void boot();
  follow();
}
