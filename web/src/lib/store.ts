// What every page reads, as the service has it: the setup objects, the run lists, the Loops, the
// team memory shown and the inbox, each a signal a page draws from. Nothing here is the source of
// truth and nothing is worked out from events: the lists are read again when the global stream
// says they changed (`stream.ts`), in the interface language (`?lang=`), and again when that
// language changes. The same reads as the Mac app's `AppStore`
// (docs/architecture/20261126-web.md#store).

import { computed, signal } from "@preact/signals";
import { api, type Json } from "./api.ts";
import { confirm, confirmDeleteRun } from "./confirm.ts";
import { serverLang, tr } from "./i18n.ts";
import { RunPager } from "./pager.ts";

export const meta = signal<Json>(null);
export const connections = signal<Json[]>([]);
export const roles = signal<Json[]>([]);
export const workspaces = signal<Json[]>([]);
/** The team a new workspace starts with, and each group's (`team` of `GET /api/setup`). */
export const defaultTeam = signal<Json>(null);
/** What a first goal still needs (`onboarding`), and the digital employees by group (`roleGroups`). */
export const onboarding = signal<Json[]>([]);
export const roleGroups = signal<Json[]>([]);
/** The newest goals (`GET /api/runs`). */
export const runs = signal<Json[]>([]);
/** Every run that may need the person however old (`attention=1`), by kind. */
export const attention = signal<Json[]>([]);
export const chatAttention = signal<Json[]>([]);
export const loopAttention = signal<Json[]>([]);
/** Every workspace goal runs were made in, as their snapshots name it (deleted ones marked). */
export const runWorkspaces = signal<Json[]>([]);
/** The workspaces with something at work now. */
export const workingWorkspaces = signal<string[]>([]);
/** Every workspace's Loops, in the server's order (what needs the person first). */
export const loops = signal<Json[]>([]);
/** Each workspace's team memory, once its page has shown. */
export const memories = signal<Record<string, Json>>({});
export const chats = new RunPager("kind=chat");
/** Each workspace's task records, once its page has shown. */
const histories = new Map<string, RunPager>();

/** A line of feedback at the bottom of the window, gone after a while. */
export const toast = signal<string | null>(null);
let toastTimer: ReturnType<typeof setTimeout> | undefined;
export function show(text: string) {
  toast.value = text;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (toast.value = null), 5000);
}

/** Workspaces deleted since, whose runs stay. */
export const pastWorkspaces = computed(() => runWorkspaces.value.filter((w) => w.deleted));
/** Runs at work now, across kinds. */
export const working = computed(() => [...attention.value, ...chatAttention.value, ...loopAttention.value].filter((r) => r.active));

export function workspace(id: string): Json | undefined {
  return workspaces.value.find((w) => w.id === id);
}

export function role(id: string): Json | undefined {
  return roles.value.find((r) => r.id === id);
}

export function connection(id: string): Json | undefined {
  return connections.value.find((c) => c.id === id);
}

/** A workspace's Loops. */
export function loopsOf(ws: string): Json[] {
  return loops.value.filter((l) => l.workspace === ws);
}

/** The Loop whose session is run `id`. */
export function loopOfRun(id: string): Json | undefined {
  return loops.value.find((l) => l.run === id);
}

// --- Inbox -------------------------------------------------------------------------------------

export const inbox = {
  open: signal<Json[]>([]),
  later: signal<Json[]>([]),
  /** What waits in each workspace, one row per goal or Loop, as the server folds `open`. */
  byWorkspace: signal<Record<string, Json[]>>({}),
  /** Entries answered from the inbox, until the next read shows them gone. */
  answering: signal<Set<string>>(new Set()),
};
let inboxGeneration = 0;
let wakeTimer: ReturnType<typeof setTimeout> | undefined;

/** Read the inbox again (`GET /api/inbox`), and once more when the next entry put off comes back. */
export async function loadInbox() {
  const generation = ++inboxGeneration;
  let j: Json;
  try {
    j = await api.get(`/api/inbox?lang=${serverLang()}`);
  } catch {
    return;
  }
  if (generation !== inboxGeneration) return;
  setIfChanged(inbox.open, j.open ?? []);
  setIfChanged(inbox.later, j.later ?? []);
  setIfChanged(inbox.byWorkspace, j.byWorkspace ?? {});
  clearTimeout(wakeTimer);
  if (typeof j.nextWake === "number") {
    wakeTimer = setTimeout(() => void loadInbox(), Math.max(0, j.nextWake - Date.now()) + 500);
  }
}

/** Answer a request with one of its quick actions, as the run page would. */
export async function inboxAct(e: Json, a: Json) {
  inbox.answering.value = new Set([...inbox.answering.value, e.key]);
  try {
    await api.post(`/api/runs/${e.run}/actions`, { ...a.body, commandId: crypto.randomUUID() });
  } catch (err) {
    show(String((err as Error).message));
  }
  await loadInbox();
  const next = new Set(inbox.answering.value);
  next.delete(e.key);
  inbox.answering.value = next;
}

async function mark(body: Json) {
  try {
    await api.post("/api/inbox/marks", body);
  } catch (err) {
    show(String((err as Error).message));
  }
  await loadInbox();
}
export const snooze = (key: string, until: Date) => mark({ key, snoozedUntil: until.getTime() });
export const wake = (key: string) => mark({ key });
export const ignore = (key: string) => mark({ key, ignored: true });

// --- Loading -----------------------------------------------------------------------------------

function setIfChanged<T>(s: { value: T }, v: T) {
  // An unchanged read leaves the pages that draw it alone.
  if (JSON.stringify(s.value) !== JSON.stringify(v)) s.value = v;
}

export async function loadMeta() {
  try {
    meta.value = await api.get("/api/meta");
  } catch (e) {
    show(String((e as Error).message));
  }
}

export async function loadSetup() {
  try {
    const s = await api.get(`/api/setup?lang=${serverLang()}`);
    setIfChanged(connections, s.connections ?? []);
    setIfChanged(roles, s.roles ?? []);
    setIfChanged(workspaces, s.workspaces ?? []);
    setIfChanged(defaultTeam, s.team ?? null);
    setIfChanged(onboarding, s.onboarding ?? []);
    setIfChanged(roleGroups, s.roleGroups ?? []);
  } catch (e) {
    show(String((e as Error).message));
  }
}

async function list(path: string, into: { value: Json[] }) {
  try {
    setIfChanged(into, (await api.get(path)).runs ?? []);
  } catch {
    // Kept as shown; the next notice reads again.
  }
}

export async function loadRuns() {
  const l = serverLang();
  await Promise.all([
    list(`/api/runs?lang=${l}`, runs),
    chats.refresh(),
    ...[...histories.values()].map((p) => p.refresh()),
    list(`/api/runs?attention=1&limit=500&lang=${l}`, attention),
    list(`/api/runs?kind=chat&attention=1&limit=500&lang=${l}`, chatAttention),
    list(`/api/runs?kind=loop&attention=1&limit=500&lang=${l}`, loopAttention),
    (async () => {
      try {
        const r = await api.get("/api/runs/workspaces");
        setIfChanged(runWorkspaces, r.workspaces ?? []);
        setIfChanged(workingWorkspaces, r.working ?? []);
      } catch {
        // Kept as shown.
      }
    })(),
  ]);
}

/** Every Loop, read again whenever the global stream names one: a change of what it is doing can move it. */
export async function loadLoops() {
  try {
    setIfChanged(loops, (await api.get(`/api/loops?lang=${serverLang()}`)).loops ?? []);
  } catch {
    // Kept as shown.
  }
}

/** A workspace's team memory: its notes and its consolidation. */
export async function loadMemories(ws: string) {
  if (!ws) return;
  try {
    const m = await api.get(`/api/workspaces/${encodeURIComponent(ws)}/memories?lang=${serverLang()}`);
    if (JSON.stringify(memories.value[ws]) !== JSON.stringify(m)) memories.value = { ...memories.value, [ws]: m };
  } catch {
    // Kept as shown.
  }
}

/** A workspace's task records, read the first time its page shows; later reads come with `loadRuns`. */
export function history(ws: string): RunPager {
  let p = histories.get(ws);
  if (!p) {
    p = new RunPager(`workspace=${encodeURIComponent(ws)}`);
    histories.set(ws, p);
    void p.refresh();
  }
  return p;
}

export async function readAll() {
  await Promise.all([loadRuns(), loadSetup(), loadLoops(), loadInbox(), ...Object.keys(memories.value).map(loadMemories)]);
}

export async function boot() {
  await Promise.all([loadMeta(), loadRuns(), loadSetup(), loadLoops()]);
  await loadInbox();
}

// --- Acting on the setup -----------------------------------------------------------------------

/** Save a setup object whole (`POST /api/setup/{kind}`) and read the setup again; the saved object comes back. */
export async function saveSetup(kind: "workspaces" | "roles" | "connections", body: Json): Promise<Json> {
  const saved = await api.post(`/api/setup/${kind}`, body);
  await loadSetup();
  return saved;
}

export async function deleteSetup(kind: "workspaces" | "roles" | "connections", id: string) {
  await api.delete(`/api/setup/${kind}/${encodeURIComponent(id)}`);
  await loadSetup();
}

// --- Acting on runs ----------------------------------------------------------------------------

/** Start a goal in a workspace and return its id; first say what the person should know before it starts. */
export async function createRun(goal: string, ws: string, images: Json[]): Promise<string | null> {
  try {
    const warnings: string[] = (await api.get(`/api/runs/preflight?workspace=${encodeURIComponent(ws)}`)).warnings ?? [];
    if (warnings.length && !(await confirm({ title: tr("继续执行？"), text: warnings.join("\n\n"), ok: tr("继续"), destructive: false }))) return null;
    const r = await api.post("/api/runs", { goal, workspace: ws, images });
    await loadRuns();
    return r.id;
  } catch (e) {
    show(tr("创建失败：%@", (e as Error).message));
    return null;
  }
}

/** Start a chat with a digital employee and return its id; its session starts right away. */
export async function createChat(roleId: string): Promise<string | null> {
  try {
    const r = await api.post("/api/chats", { role: roleId });
    await loadRuns();
    return r.id;
  } catch (e) {
    show(tr("开始对话失败：%@", (e as Error).message));
    return null;
  }
}

/** Delete a run (a goal, or a chat) after asking; `false` when it stayed. */
export async function deleteRun(r: Json): Promise<boolean> {
  if (r.delete && r.delete.ok === false) {
    show(r.delete.why);
    return false;
  }
  const chat = r.kind === "chat";
  const name = r.title ? tr("「%@」", r.title) : chat ? tr("这个对话") : tr("这个任务");
  const ok = chat
    ? await confirm({ title: tr("删除对话？"), text: tr("%@的聊天记录、日志和专属工作目录（含%@留下的文件）都会删除，找不回来；进行中的会话会停止。", name, r.config?.team?.[0]?.name || tr("这名数字员工")), ok: tr("删除") })
    : await confirmDeleteRun(name);
  if (!ok) return false;
  try {
    await api.delete(`/api/runs/${encodeURIComponent(r.id)}`);
  } catch (e) {
    show(tr("删除失败：%@", (e as Error).message));
    return false;
  }
  await Promise.all([loadRuns(), loadInbox()]);
  show(chat ? tr("已删除对话") : tr("已删除任务"));
  return true;
}
