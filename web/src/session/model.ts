// One open run, as the Mac app's `RunModel` holds it: its events (read once, then followed on the
// run's own stream), the view the service works out from them (`GET /api/runs/{id}/view`), the
// live previews the stream sends, and what the person has in hand: drafts, pasted images, the rows
// they opened or folded, the options they picked. Nothing here is worked out from events
// (docs/architecture/20261126-web.md#session).

import { signal } from "@preact/signals";
import { api, query, type Json } from "../lib/api.ts";
import { serverLang } from "../lib/i18n.ts";
import { load, save } from "../lib/prefs.ts";
import { show } from "../lib/store.ts";
import type { PickedImage } from "../ui/form.tsx";

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
  /** The run's record: its kind and its team's snapshot. Whether it is live is the view's. */
  readonly run = signal<Json | null>(null);
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
    const d = await api.get(this.path);
    this.run.value = d.run;
    this.events.value = d.events ?? [];
    await this.loadView();
    this.follow();
  }

  stop() {
    this.closed = true;
    this.source?.close();
    clearTimeout(this.timer);
  }

  /** The view at the display level chosen, in the interface's language. */
  async loadView() {
    const n = ++this.loading;
    const v = await api.get(`${this.path}/view${query({ lang: serverLang(), detail: density.value === "detail" ? 1 : undefined })}`);
    if (n === this.loading && !this.closed) this.view.value = v;
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
      if (list.length && e.seq <= list[list.length - 1].seq) return;
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

  /** A person's action (`POST /api/runs/{id}/actions`); false when the service refused it, and says why. */
  async act(body: Json): Promise<boolean> {
    try {
      await api.post(`${this.path}/actions`, body);
      this.scheduleView();
      return true;
    } catch (e) {
      show(e instanceof Error ? e.message : String(e));
      return false;
    }
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
