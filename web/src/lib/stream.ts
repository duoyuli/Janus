// Follows every run through the service's global stream (`GET /api/stream`) instead of polling,
// as the Mac app's `AppStore.followEvents` does (docs/interfaces/20261031-http_api.md#global_stream):
// a notice says what changed, and only that is read again. Notices that arrive together are read
// once, 300 ms later. When the stream opens, lags or ends, everything is read once; it reconnects.
// Pages that need more (a run's own page, a Loop) add their own handlers with `onNotice`.

import { signal } from "@preact/signals";
import { api, type ApiError, type Json, unpaired } from "./api.ts";
import { attention, chats, loadInbox, loadLoops, loadMemories, loadRuns, loadSetup, memories, readAll, runs } from "./store.ts";

/** The stream is open now. */
export const connected = signal(false);

type Handler = (name: string, data: Json) => void;
const handlers = new Set<Handler>();

/** Hear every notice of the global stream (`run`, `loop`, `memory`, `library`, …); returns the way to stop. */
export function onNotice(h: Handler): () => void {
  handlers.add(h);
  return () => handlers.delete(h);
}

let listsChanged = false;
let batch: ReturnType<typeof setTimeout> | undefined;

function note(lists: boolean) {
  listsChanged ||= lists;
  if (batch !== undefined) return;
  batch = setTimeout(() => {
    const lists = listsChanged;
    listsChanged = false;
    batch = undefined;
    if (lists) void loadRuns();
    void loadInbox();
  }, 300);
}

/** Whether a new event changes what the lists show: a run they do not have yet, or a chat's first message. */
function listChanges(type: string, run: string): boolean {
  if (type === "run.created") return true;
  const chat = chats.runs.value.find((c) => c.id === run);
  if (type === "user.message" && chat) return !chat.goal;
  return !attention.value.some((r) => r.id === run) && !chat && !runs.value.some((r) => r.id === run);
}

let source: EventSource | null = null;

export function follow() {
  source?.close();
  const s = new EventSource("/api/stream", { withCredentials: true });
  source = s;
  const on = (name: string, f: (data: Json) => void) =>
    s.addEventListener(name, (ev) => {
      let data: Json = {};
      try {
        data = JSON.parse((ev as MessageEvent).data || "{}");
      } catch {
        // A notice without a body still says what changed.
      }
      f(data);
      for (const h of handlers) h(name, data);
    });
  on("ready", () => {
    connected.value = true;
    void readAll();
  });
  on("lagged", () => void readAll());
  on("run", (n) => {
    note(true);
    // A run a Loop dispatched changed: that Loop's counts did too.
    if (n.loop) void loadLoops();
  });
  on("loop", () => void loadLoops());
  // A workspace's team memory changed: read it again if its page has shown.
  on("memory", (n) => {
    if (memories.value[n.workspace]) void loadMemories(n.workspace);
  });
  on("connection", () => void loadSetup());
  on("inbox", () => void loadInbox());
  on("event", (n) => note(listChanges(String(n.type ?? ""), String(n.run ?? ""))));
  for (const name of ["library", "adapters", "devices"]) on(name, () => {});
  s.onerror = () => {
    // The service restarted or is gone for a moment, or this device was unpaired (the service
    // ends its streams then): after a moment, ask who this device is. Unpaired, the window shows
    // how to pair and the stream stays closed; otherwise it opens again and `ready` reads all.
    connected.value = false;
    s.close();
    if (source !== s) return;
    setTimeout(() => {
      api.get("/api/devices/me").then(follow, (e: ApiError) => {
        if (!unpaired.value && source === s) follow();
        return e;
      });
    }, 2000);
  };
}

/** Stop following: this device is no longer paired. */
export function stop() {
  source?.close();
  source = null;
  connected.value = false;
}
