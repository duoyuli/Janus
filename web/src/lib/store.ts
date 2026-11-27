// What every page reads, as the service has it: the setup objects, the run lists and the inbox,
// each a signal a page draws from. Nothing here is the source of truth and nothing is worked out
// from events: the lists are read again when the global stream says they changed (`stream.ts`).
// The same reads as the Mac app's `AppStore` (docs/architecture/20261126-web.md#store).

import { computed, signal } from "@preact/signals";
import { api, type Json } from "./api.ts";
import { serverLang } from "./i18n.ts";
import { RunPager } from "./pager.ts";

export const meta = signal<Json>(null);
export const connections = signal<Json[]>([]);
export const roles = signal<Json[]>([]);
export const workspaces = signal<Json[]>([]);
/** The newest goals (`GET /api/runs`). */
export const runs = signal<Json[]>([]);
/** Every run that may need the person however old (`attention=1`), by kind. */
export const attention = signal<Json[]>([]);
export const chatAttention = signal<Json[]>([]);
export const loopAttention = signal<Json[]>([]);
/** Every workspace goal runs were made in, as their snapshots name it (deleted ones too). */
export const runWorkspaces = signal<Json[]>([]);
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
export const pastWorkspaces = computed(() => runWorkspaces.value.filter((w) => !workspaces.value.some((x) => x.id === w.id)));
/** Runs at work now, across kinds. */
export const working = computed(() =>
  [...attention.value, ...chatAttention.value, ...loopAttention.value].filter((r) => ["queued", "planning", "running"].includes(r.status)),
);

export function workspace(id: string): Json | undefined {
  return workspaces.value.find((w) => w.id === id);
}

export function role(id: string): Json | undefined {
  return roles.value.find((r) => r.id === id);
}

// --- Inbox -------------------------------------------------------------------------------------

export const inbox = {
  open: signal<Json[]>([]),
  later: signal<Json[]>([]),
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
    const s = await api.get("/api/setup");
    setIfChanged(connections, s.connections ?? []);
    setIfChanged(roles, s.roles ?? []);
    setIfChanged(workspaces, s.workspaces ?? []);
  } catch (e) {
    show(String((e as Error).message));
  }
}

async function list(path: string, into: { value: Json[] }, field = "runs") {
  try {
    setIfChanged(into, (await api.get(path))[field] ?? []);
  } catch {
    // Kept as shown; the next notice reads again.
  }
}

export async function loadRuns() {
  await Promise.all([
    list("/api/runs", runs),
    chats.refresh(),
    ...[...histories.values()].map((p) => p.refresh()),
    list("/api/runs?attention=1&limit=500", attention),
    list("/api/runs?kind=chat&attention=1&limit=500", chatAttention),
    list("/api/runs?kind=loop&attention=1&limit=500", loopAttention),
    list("/api/runs/workspaces", runWorkspaces, "workspaces"),
  ]);
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
  await Promise.all([loadRuns(), loadSetup(), loadInbox()]);
}

export async function boot() {
  await Promise.all([loadMeta(), loadRuns(), loadSetup()]);
  await loadInbox();
}
