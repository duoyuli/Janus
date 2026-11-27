// The rows of a conversation as the service laid them out (`sessions[key].rows`,
// backend/src/run_view/turns.rs), with the person's own clicks applied: a folded turn leaves out
// what folds with it, an opened group of calls shows open, and the running turn's group split off
// at the step the agent is on shows whole once clicked. The same rule as the Mac app's
// `Conversation.rows`; nothing is regrouped here (docs/architecture/20261031-frontend.md#session_turns).

import type { Json } from "../lib/api.ts";

export type Gap = "none" | "round" | "block" | "half" | "line" | "loose";

export type Row =
  | { kind: "section"; id: number; gap: Gap; event: Json; injected: Json[] }
  | { kind: "item"; id: number; gap: Gap; event: Json }
  | { kind: "head"; id: number; gap: Gap; turn: number; agent: string; running: boolean; start: number; end: number; stats: string; summary: string; open: boolean | null }
  | { kind: "activity"; id: number; gap: Gap; events: Json[]; open: boolean | null; summary: string }
  | { kind: "text"; id: number; gap: Gap; events: Json[]; reply: boolean }
  | { kind: "edits"; id: number; gap: Gap; files: Json[] };

/** The rows `served` as shown now; `event` finds an event by its seq, `open` is the person's clicks by row id. */
export function rows(served: Json[], event: (seq: number) => Json | undefined, open: Record<number, boolean>): Row[] {
  const evs = (list: Json[] | undefined) => (list ?? []).map((s) => event(s)).filter(Boolean) as Json[];
  // Each turn's line as it stands: the person's click, else how it starts.
  const folded = new Set<number>();
  for (const r of served) {
    if (r.kind !== "head" || typeof r.open !== "boolean") continue;
    if (!(open[r.id] ?? r.open)) folded.add(r.turn);
  }
  const out: Row[] = [];
  for (const r of served) {
    const gap: Gap = r.gap ?? "none";
    const id: number = r.id ?? 0;
    if (r.fold === true && folded.has(r.turn)) continue;
    switch (r.kind) {
      case "section": {
        const e = event(r.seq);
        if (e) out.push({ kind: "section", id, gap, event: e, injected: evs(r.injected) });
        break;
      }
      case "item": {
        const e = event(r.seq);
        if (e) out.push({ kind: "item", id, gap, event: e });
        break;
      }
      case "head":
        out.push({
          kind: "head",
          id,
          gap,
          turn: r.turn ?? 0,
          agent: r.agent ?? "",
          running: !!r.running,
          start: r.start ?? 0,
          end: r.end ?? 0,
          stats: r.stats ?? "",
          summary: r.summary ?? "",
          open: typeof r.open === "boolean" ? (open[id] ?? r.open) : null,
        });
        break;
      case "activity": {
        // The step the agent is on, split off its group: the group shows whole once clicked.
        if (r.partOf !== undefined && open[r.partOf] !== undefined) break;
        let shown = evs(r.events);
        let state: boolean | null = typeof r.open === "boolean" ? r.open : null;
        const clicked = open[id];
        if (clicked !== undefined) {
          if (r.whole !== undefined) {
            shown = evs(r.whole);
            state = r.sole === true ? null : clicked;
          } else if (state !== null) {
            state = clicked;
          }
        }
        if (shown.length) out.push({ kind: "activity", id, gap, events: shown, open: state, summary: r.summary ?? "" });
        break;
      }
      case "text": {
        const shown = evs(r.events);
        if (shown.length) out.push({ kind: "text", id, gap, events: shown, reply: !!r.reply });
        break;
      }
      case "edits":
        out.push({ kind: "edits", id, gap, files: r.files ?? [] });
        break;
    }
  }
  return out;
}

/** The space above a row, in pixels (the Mac app's `ChatRhythm`). */
export function gapPx(g: Gap): number {
  switch (g) {
    case "round":
      return 20;
    case "block":
      return 12;
    case "half":
      return 6;
    case "line":
      return 4;
    case "loose":
      return 8;
    default:
      return 0;
  }
}
