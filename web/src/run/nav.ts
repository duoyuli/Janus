// Where a run's pages are: `/runs/<id>` is its workbench, and under it the audit log
// (`audit`, `audit/<seq>` opens an event), the team (`team`), a subtask (`tasks/<id>`), an
// agent (`agents/<id>`), a diff (`diff/<path>`) and a document (`doc/<path>`). A path so a link,
// the back button and a reload all land on the same page (Mac: `RunPage`).

export type RunSub =
  | { kind: "board" }
  | { kind: "audit"; seq: number | null }
  | { kind: "team" }
  | { kind: "task"; id: string }
  | { kind: "agent"; id: string }
  | { kind: "diff"; path: string }
  | { kind: "doc"; path: string; from?: string };

/** The run page `sub` (the path after `/runs/<id>/`) names. */
export function runSub(sub: readonly string[]): RunSub {
  const [a, b] = sub;
  if (a === "audit") return { kind: "audit", seq: b && /^\d+$/.test(b) ? Number(b) : null };
  if (a === "team") return { kind: "team" };
  if (a === "tasks" && b) return { kind: "task", id: b };
  if (a === "agents" && b) return { kind: "agent", id: b };
  if (a === "diff" && b) return { kind: "diff", path: b };
  if (a === "doc" && b) return sub[2] ? { kind: "doc", path: b, from: sub[2] } : { kind: "doc", path: b };
  return { kind: "board" };
}

/** The path of run `id`'s page `sub` (`audit`, `tasks/T1`, …; none for the workbench). */
export function runLink(id: string, sub = ""): string {
  return `/runs/${encodeURIComponent(id)}${sub ? `/${sub}` : ""}`;
}

export const taskLink = (run: string, task: string) => runLink(run, `tasks/${encodeURIComponent(task)}`);
export const agentLink = (run: string, agent: string) => runLink(run, `agents/${encodeURIComponent(agent)}`);
export const diffLink = (run: string, path: string) => runLink(run, `diff/${encodeURIComponent(path)}`);
export const docLink = (run: string, path: string) => runLink(run, `doc/${encodeURIComponent(path)}`);
