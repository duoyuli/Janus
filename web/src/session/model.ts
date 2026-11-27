// One open run, as the Mac app's `RunModel` holds it: its events (read once, then followed on the
// run's own stream), the view and the usage ledger the service works out from them
// (`GET /api/runs/{id}/view`, `/usage`) at the shown position (a replayed one asks with `until`),
// the live previews the stream sends, and what the person has in hand: drafts, pasted images, the
// rows they opened or folded, the options they picked. Nothing here is worked out from events
// (docs/architecture/20261126-web.md#session). The conversations and the goal's run page share it.

import { computed, signal } from "@preact/signals";
import { api, ApiError, query, type Json } from "../lib/api.ts";
import { serverLang, t } from "../lib/i18n.ts";
import { load, save } from "../lib/prefs.ts";
import { show } from "../lib/store.ts";
import type { PickedImage } from "../ui/form.tsx";
import { randomId } from "../lib/plain-http.ts";

/** How much of an agent's work a conversation shows; one choice for every conversation on this browser. */
export type Density = "summary" | "detail";
export const density = signal<Density>(load<Density>("chatDensity", "summary"));
export function setDensity(d: Density) {
  density.value = d;
  save("chatDensity", d);
}


/** A session's key, `task:<id>` or `agent:<id>`, as the body of an action addresses it. */
export function target(key: string): { task: string; agent: string } {
  const at = key.indexOf(":");
  const [kind, id] = [key.slice(0, at), key.slice(at + 1)];
  return kind === "task" ? { task: id, agent: "" } : { task: "", agent: id };
}

export class RunModel {
  readonly id: string;
  readonly events = signal<Json[]>([]);
  readonly view = signal<Json | null>(null);
  /** The run's record: its kind and its team's snapshot. */
  readonly run = signal<Json | null>(null);
  /** The service holds the run in memory: its sessions can be acted on. */
  readonly live = signal(false);
  /** The usage ledger with its words, at the shown position. */
  readonly usage = signal<Json | null>(null);
  /** How many events the replayed position shows; `null` while live. */
  readonly replay = signal<number | null>(null);
  /** The view at the replayed position. */
  readonly replayView = signal<Json | null>(null);
  /** What the run page shows: the replayed position's view, else the live one. */
  readonly shown = computed<Json | null>(() => (this.replay.value !== null ? this.replayView.value : this.view.value));
  readonly replaying = computed(() => this.replay.value !== null);
  /** Its sessions can be acted on: the service holds the run, and the page shows it live. */
  readonly canAct = computed(() => this.live.value && this.replay.value === null);
  /** What each conversation's session is writing and running now, by session key. */
  readonly preview = signal<Record<string, Json>>({});
  /** The rows the person opened (true) or folded (false), by session key and row id. */
  readonly toggles = signal<Record<string, Record<number, boolean>>>({});
  readonly drafts = signal<Record<string, string>>({});
  readonly images = signal<Record<string, PickedImage[]>>({});
  /** A question's picked option, a note under it, a form's answers, by request key. */
  readonly picks = signal<Record<string, string | undefined>>({});
  readonly notes = signal<Record<string, string>>({});
  readonly forms = signal<Record<string, Record<string, Json>>>({});
  /** Bumped when the person sends a message, which brings the conversation to its newest row. */
  readonly sent = signal(0);
  /** How the page shows another conversation of the run (a session's card opens it), when it can. */
  openSession: ((key: string) => void) | null = null;
  private source: EventSource | null = null;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private closed = false;
  private loading = 0;

  constructor(id: string) {
    this.id = id;
  }

  get path() {
    return `/api/runs/${encodeURIComponent(this.id)}`;
  }

  /** Read the run and its events, its view, then follow its stream. */
  async start() {
    this.source?.close();
    const d = await api.get(this.path);
    this.run.value = d.run;
    this.events.value = d.events ?? [];
    this.live.value = !!d.live;
    const r = this.replay.value;
    if (r !== null && r > this.events.value.length) this.replay.value = null;
    await this.loadView();
    this.follow();
  }

  /** The seq of the last event at the replayed position; `null` while live. */
  private replayedSeq(): number | null {
    const n = this.replay.value;
    if (n === null) return null;
    return this.events.value[Math.max(0, Math.min(n, this.events.value.length) - 1)]?.seq ?? 0;
  }

  /** Show the position after `n` events, or the live run (`null`). */
  setReplay(n: number | null) {
    this.replay.value = n === null ? null : Math.max(1, Math.min(this.events.value.length, n));
    void this.loadView().catch(() => {});
  }

  stop() {
    this.closed = true;
    this.source?.close();
    clearTimeout(this.timer);
  }

  /** The view at the display level chosen and the usage ledger, in the interface's language, at
   * the shown position; an answer for a position no longer shown is dropped. */
  async loadView() {
    const n = ++this.loading;
    const until = this.replayedSeq();
    const last = until ?? this.events.value.at(-1)?.seq ?? 0;
    const lang = serverLang();
    const [v, u] = await Promise.all([
      api.get(`${this.path}/view${query({ lang, until, detail: density.value === "detail" ? 1 : undefined })}`),
      last > 0 ? api.get(`${this.path}/usage${query({ lang, until: last })}`).catch(() => null) : Promise.resolve(null),
    ]);
    if (n !== this.loading || this.closed || this.replayedSeq() !== until) return;
    if (until !== null) this.replayView.value = v;
    else {
      this.view.value = v;
      // The service holds the run now but this page read it when it did not: read it again to follow it live.
      if (!this.live.value && v?.live) void this.start().catch(() => {});
    }
    if (u) this.usage.value = u;
  }

  /** Read the view again shortly: events arrive in bursts. */
  scheduleView() {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.loadView().catch(() => {}), 150);
  }

  private follow() {
    if (this.closed) return;
    const after = this.events.value.at(-1)?.seq ?? 0;
    const s = new EventSource(`${this.path}/stream${query({ after, lang: serverLang() })}`, { withCredentials: true });
    this.source = s;
    // The stream starts again from here and sends each preview anew.
    this.preview.value = {};
    s.onmessage = (ev) => {
      const e = JSON.parse(ev.data);
      const list = this.events.value;
      const last = list.at(-1)?.seq ?? 0;
      if (e.seq <= last) return;
      // A gap means something was missed: read the whole run again.
      if (e.seq !== last + 1) {
        s.close();
        void this.start().catch(() => {});
        return;
      }
      this.events.value = [...list, e];
      this.scheduleView();
    };
    s.addEventListener("live", (ev) => {
      const f = JSON.parse((ev as MessageEvent).data);
      const next = { ...this.preview.value };
      for (const [k, p] of Object.entries(f.previews ?? {})) {
        if (p === null) delete next[k];
        else next[k] = p;
      }
      this.preview.value = next;
    });
    s.addEventListener("lagged", () => {
      s.close();
      void this.start();
    });
    s.onerror = () => {
      s.close();
      if (this.source !== s || this.closed) return;
      setTimeout(() => this.follow(), 2000);
    };
  }

  /** A person's action (docs/interfaces/20261031-http_api.md#run_actions): de-duplicated by
   * `commandId`, guarded by `expectedSequence`; a 409 reads the run again once and tries again.
   * False when the service refused it, and says why. */
  async act(body: Json): Promise<boolean> {
    if (this.replay.value !== null) return false;
    const b = { ...body, commandId: randomId() };
    for (let attempt = 0; attempt < 2; attempt++) {
      b.expectedSequence = this.events.value.at(-1)?.seq ?? 0;
      try {
        await api.post(`${this.path}/actions`, b);
        // Resuming, or writing to a run the service no longer holds, brings it back: follow it again.
        if (!this.live.value && (b.type === "resumeRun" || b.type === "message")) {
          await this.start();
          show(b.type === "resumeRun" ? t`已从中断处继续` : t`正在接上会话`);
        } else this.scheduleView();
        return true;
      } catch (e) {
        if (e instanceof ApiError && e.status === 409 && attempt === 0) {
          await this.start().catch(() => {});
          continue;
        }
        show(e instanceof Error ? e.message : String(e));
        return false;
      }
    }
    return false;
  }

  event(seq: number): Json | undefined {
    const list = this.events.value;
    const guess = list[seq - 1];
    return guess?.seq === seq ? guess : list.find((e) => e.seq === seq);
  }

  toggle(key: string, id: number, open: boolean) {
    const all = this.toggles.value;
    this.toggles.value = { ...all, [key]: { ...(all[key] ?? {}), [id]: open } };
  }

  setDraft(key: string, text: string) {
    this.drafts.value = { ...this.drafts.value, [key]: text };
  }

  setImages(key: string, list: PickedImage[]) {
    this.images.value = { ...this.images.value, [key]: list };
  }
}
