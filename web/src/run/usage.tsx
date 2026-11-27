// The run's usage on its card (Mac: UI/Run/Usage.swift, the run card's summary line): the line of
// numbers and the expanded ledger, as the service words them (`words` of `GET /api/runs/{id}/usage`).
// Only the wall-clock item counts on here while a live run goes, as the app does.

import { useEffect, useState } from "preact/hooks";
import type { Json } from "../lib/api.ts";
import { dur } from "../lib/format.ts";
import { t } from "../lib/i18n.ts";
import { link } from "../lib/router.ts";
import { Avatar } from "../ui/kit.tsx";
import { Icon } from "../ui/Icon.tsx";
import type { RunModel } from "../session/model.ts";
import { who } from "../session/Items.tsx";
import { agentLink, taskLink } from "./nav.ts";
import { TaskTag } from "./parts.tsx";

/** The run's wall-clock time at `now`: from its start while it ticks, never below the record. */
export function elapsedMs(usage: Json, now: number, start: number, ticking: boolean): number | null {
  const recorded = usage?.run?.durationMs ?? null;
  if (!ticking || start <= 0) return recorded;
  return Math.max(recorded ?? 0, Math.round(Math.max(0, now - start)));
}

/** The run card's line and its full reading: the progress, then the service's items. */
export function cardLine(usage: Json, progress: string | null, now: number, start: number, ticking: boolean): { line: string; voice: string } {
  const words = usage?.words ?? {};
  const items = (xs: string[] = []) => {
    const out = [...xs];
    const ms = elapsedMs(usage, now, start, ticking);
    if (ticking && start > 0 && words.elapsed < out.length && ms !== null) out[words.elapsed] = t`耗时 ${t`${dur(ms)}（总时长）`}`;
    return [...(progress ? [progress] : []), ...out];
  };
  return { line: items(words.card).join(" · "), voice: items(words.voice).join(t`，`) };
}

/** Redraw once a second while `on`. */
export function useTicking(on: boolean): number {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!on) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [on]);
  return on ? now : Date.now();
}

/** The expanded ledger: its rows, how many sessions, and the tasks' and members' detail. */
export function UsageSection({ run, st }: { run: RunModel; st: Json }) {
  const [detail, setDetail] = useState(false);
  const w = run.usage.value?.words ?? {};
  const title = (id: string) => (st.tasks ?? []).find((x: Json) => x.id === id)?.title ?? "";
  return (
    <div class="usage" aria-label={t`这次任务的用量明细`}>
      <hr />
      {(w.rows ?? []).map((r: Json) => (
        <div class="usage-row" title={r.help}>
          <span class="secondary callout usage-label">{r.label}</span>
          <span class="callout grow">{r.value}</span>
        </div>
      ))}
      <div class="row-inline">
        <span class="caption secondary grow">{w.sessions}</span>
        <button type="button" class="link callout" title={t`按子任务和成员拆开看用量`} aria-expanded={detail} onClick={() => setDetail(!detail)}>
          <Icon name={detail ? "chevron.down" : "chevron.right"} size={10} /> {detail ? t`收起明细` : t`子任务与成员明细`}
        </button>
      </div>
      {detail && (
        <div class="usage-detail">
          {!(w.tasks ?? []).length && !(w.members ?? []).length && <div class="caption secondary">{t`还没有子任务和会话。`}</div>}
          {(w.tasks ?? []).map((d: Json) => (
            <a class="usage-line" title={d.help} {...link(taskLink(run.id, d.id))}>
              <TaskTag id={d.id} />
              <span class="callout ellipsis">{title(d.id)}</span>
              <span class="grow" />
              <span class="caption secondary ellipsis">{d.text}</span>
            </a>
          ))}
          {(w.tasks ?? []).length > 0 && (w.members ?? []).length > 0 && <hr />}
          {(w.members ?? []).map((d: Json) => {
            const w = who(run, d.id);
            return (
              <a class="usage-line" title={d.help} {...link(agentLink(run.id, d.id))}>
                <Avatar role={w.role} size={14} />
                <span class="callout ellipsis">{w.name}</span>
                {d.id === "lead" && w.name !== t`负责人` && <span class="caption secondary">{t`负责人`}</span>}
                <span class="grow" />
                <span class="caption secondary ellipsis">{d.text}</span>
              </a>
            );
          })}
        </div>
      )}
    </div>
  );
}
