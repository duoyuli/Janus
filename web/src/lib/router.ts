// Where the window is: the path of the page, so a link, the back button and a reload all land on
// the same page. The service answers every path that is not `/api/…` with the app
// (docs/architecture/20261126-web.md#routes).

import { computed, signal } from "@preact/signals";

export const path = signal(location.pathname);

export function go(to: string, replace = false) {
  if (to === location.pathname + location.search) return;
  if (replace) history.replaceState(null, "", to);
  else history.pushState(null, "", to);
  path.value = location.pathname;
  window.scrollTo(0, 0);
}

addEventListener("popstate", () => {
  path.value = location.pathname;
});

export type Route =
  | { page: "inbox" }
  | { page: "workspaces" }
  | { page: "workspace"; id: string }
  | { page: "newWorkspace" }
  | { page: "workspaceSettings"; id: string }
  | { page: "brief"; id: string }
  | { page: "memory"; id: string }
  | { page: "note"; id: string; note: string }
  | { page: "newLoop"; id: string }
  | { page: "editLoop"; loop: string }
  | { page: "run"; id: string }
  | { page: "chats" }
  | { page: "newChat" }
  | { page: "settings"; tab: string; sub?: string; id?: string }
  | { page: "pair"; code: string }
  | { page: "missing" };

/** The page a path names. */
export function match(p: string): Route {
  const parts = p.split("/").filter(Boolean).map(decodeURIComponent);
  const [a, b, c, d] = parts;
  if (parts.length === 0 || (a === "inbox" && !b)) return { page: "inbox" };
  if (a === "workspaces" && !b) return { page: "workspaces" };
  if (a === "workspaces" && b === "new" && !c) return { page: "newWorkspace" };
  if (a === "workspaces" && b && !c) return { page: "workspace", id: b };
  if (a === "workspaces" && b && c === "settings" && !d) return { page: "workspaceSettings", id: b };
  if (a === "workspaces" && b && c === "brief" && !d) return { page: "brief", id: b };
  if (a === "workspaces" && b && c === "memory" && !d) return { page: "memory", id: b };
  if (a === "workspaces" && b && c === "memory" && d) return { page: "note", id: b, note: d };
  if (a === "workspaces" && b && c === "loops" && d === "new") return { page: "newLoop", id: b };
  if (a === "loops" && b && c === "edit" && !d) return { page: "editLoop", loop: b };
  if (a === "runs" && b) return { page: "run", id: b };
  if (a === "chats" && !b) return { page: "chats" };
  if (a === "chats" && b === "new" && !c) return { page: "newChat" };
  if (a === "settings") return { page: "settings", tab: b ?? "", sub: parts[2], id: parts[3] };
  if (a === "pair" && b) return { page: "pair", code: b };
  return { page: "missing" };
}

export const route = computed(() => match(path.value));

/** Links in the app go through `go`, so the page changes without reloading. */
export function link(to: string) {
  return {
    href: to,
    onClick: (e: MouseEvent) => {
      if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
      e.preventDefault();
      go(to);
    },
  };
}
