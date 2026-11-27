// The web client's entry: the frame, the page the path names, and the service's state read once
// and then followed on the global stream (docs/architecture/20261126-web.md).

import { render } from "preact";
import { unpaired } from "./lib/api.ts";
import "./lib/appearance.ts";
import { lang } from "./lib/i18n.ts";
import { route } from "./lib/router.ts";
import { boot, loadInbox } from "./lib/store.ts";
import { follow } from "./lib/stream.ts";
import "./lib/testing.ts";
import { Shell } from "./app/Shell.tsx";
import { InboxPage } from "./pages/inbox.tsx";
import { ChatsPage, MissingPage, RunPage, SettingsPage } from "./pages/misc.tsx";
import { PairPage, UnpairedPage } from "./pages/pair.tsx";
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
    case "run":
      return <RunPage id={r.id} key={r.id} />;
    case "chats":
      return <ChatsPage />;
    case "settings":
      return <SettingsPage />;
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

// What the service words follows the interface language: the inbox is read again in the new one.
let first = true;
effect(() => {
  document.documentElement.lang = lang.value === "en" ? "en" : "zh-Hans";
  if (!first) void loadInbox();
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
