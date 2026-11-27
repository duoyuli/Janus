// The run's progress board (Mac: UI/Run/SharedBoard.swift `ProgressBoard`, UI/Run/Timeline.swift):
// one timeline of how the run moved forward, as the service lays out its rows and marks
// (`timeline` of the view). Rows are tasks, bars their attempts, connectors handoffs from upstream
// tasks, decisions and open items sit on a lane above. A bar opens its task; a mark opens its event
// in the audit log. Above the lead's conversation it stays folded to one line until opened. Only the
// drawing is done here: the time scale that cuts idle stretches short, and where things go on it.

import { useState } from "preact/hooks";
import type { Json } from "../lib/api.ts";
import { dur } from "../lib/format.ts";
import { t } from "../lib/i18n.ts";
import { go, link } from "../lib/router.ts";
import type { Tone } from "../lib/status.ts";
import { Avatar, Card } from "../ui/kit.tsx";
import { Icon } from "../ui/Icon.tsx";
import type { RunModel } from "../session/model.ts";
import { who } from "../session/Items.tsx";
import { runLink, taskLink } from "./nav.ts";
import { clock, StatusGlyph, taskLabel } from "./parts.tsx";

type Mark = { id: string; seq: number; ts: number; kind: string; title: string; by: string; what: string; label: string; you: boolean };

const OPEN_SYMBOLS: Record<string, string> = {
  approval: "hand.raised.fill", input: "square.and.pencil", question: "questionmark.bubble.fill", session: "text.bubble.fill",
  lead_stall: "hourglass", loop_touched: "doc.badge.ellipsis", loop_failed: "exclamationmark.triangle", conflict: "arrow.triangle.merge",
  overstep: "arrow.right.to.line", trace: "doc.badge.ellipsis", stall: "hourglass", blocker: "exclamationmark.octagon.fill",
  lead_question: "questionmark.circle.fill", failed: "xmark.octagon.fill", blocked: "pause.circle.fill",
};

function symbol(m: Mark): string {
  if (m.kind === "result") return "checkmark.seal.fill";
  if (m.kind === "artifact") return "doc.fill";
  if (m.kind === "decision") return m.by === "user" ? "person.crop.circle.fill" : "diamond.fill";
  return OPEN_SYMBOLS[m.what] ?? "exclamationmark.triangle.fill";
}

function markTone(m: Mark): Tone {
  if (m.kind === "result") return "green";
  if (m.kind === "artifact") return "blue";
  if (m.kind === "decision") return "violet";
  return m.you ? "red" : "amber";
}

function kindName(m: Mark): string {
  if (m.kind === "result") return t`结论`;
  if (m.kind === "artifact") return t`产物`;
  if (m.kind === "decision") return t`决定`;
  return m.label;
}

/** Time on the x axis, with stretches where nothing happened cut to a fixed break. */
class TimeScale {
  static readonly breakW = 36;
  segs: { t0: number; t1: number; x0: number }[] = [];
  k = 0;

  constructor(spans: [number, number][], width: number, lead: number, trail: number, untilLast = false) {
    const iv = spans.map(([a, b]) => [a, Math.max(a, b)] as [number, number]).sort((a, b) => a[0] - b[0]);
    if (!iv.length) return;
    const lo = iv[0]![0];
    const hi = Math.max(...iv.map((x) => x[1]));
    const threshold = Math.max(180_000, (hi - lo) * 0.08);
    const merged: [number, number][] = [];
    for (const [a, b] of iv) {
      const last = merged.at(-1);
      if (last && a - last[1] <= threshold) last[1] = Math.max(last[1], b);
      else merged.push([a, b]);
    }
    const active0 = merged.reduce((s, [a, b]) => s + (b - a), 0);
    const minDur = Math.max(20_000, active0 * 0.06);
    merged.forEach((m, i) => {
      if (m[1] - m[0] < minDur && !(untilLast && i === merged.length - 1 && m[1] > m[0])) {
        const room = i + 1 < merged.length ? merged[i + 1]![0] - m[0] : Infinity;
        m[1] = m[0] + Math.min(minDur, room);
      }
    });
    const active = merged.reduce((s, [a, b]) => s + (b - a), 0);
    const avail = Math.max(width - lead - trail - (merged.length - 1) * TimeScale.breakW, 40);
    this.k = avail / Math.max(active, 1);
    let x = lead;
    for (const [a, b] of merged) {
      this.segs.push({ t0: a, t1: b, x0: x });
      x += (b - a) * this.k + TimeScale.breakW;
    }
  }

  x(ts: number): number {
    let s = this.segs[0];
    if (!s) return 0;
    for (const g of this.segs) if (g.t0 <= ts) s = g;
    return s.x0 + (Math.min(Math.max(ts, s.t0), s.t1) - s.t0) * this.k;
  }

  end(s: { t0: number; t1: number; x0: number }): number {
    return s.x0 + (s.t1 - s.t0) * this.k;
  }

  breaks(): { x: number; ms: number }[] {
    return this.segs.slice(1).map((s, i) => ({ x: this.end(this.segs[i]!), ms: s.t0 - this.segs[i]!.t1 }));
  }

  ticks(): { x: number; ts: number; seconds: boolean }[] {
    if (this.k <= 0) return [];
    const steps = [10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 14400].map((s) => s * 1000);
    const step = steps.find((s) => s * this.k >= 90) ?? steps.at(-1)!;
    const seconds = step < 60_000;
    const out: { x: number; ts: number; seconds: boolean }[] = [];
    this.segs.forEach((s, i) => {
      if (i > 0) out.push({ x: s.x0, ts: s.t0, seconds });
      for (let tt = Math.ceil(s.t0 / step) * step; tt <= s.t1; tt += step) {
        const px = this.x(tt);
        if (!out.length || px - out.at(-1)!.x >= 46) out.push({ x: px, ts: tt, seconds });
      }
    });
    return out;
  }
}

function idle(ms: number): string {
  const m = Math.round(ms / 60_000);
  return m < 60 ? t`${m} 分钟` : m % 60 === 0 ? t`${m / 60} 小时` : t`${Math.floor(m / 60)} 小时 ${m % 60} 分`;
}

/** The 「进展」 section above the lead's conversation. */
export function ProgressBoard({ run, st, now, live }: { run: RunModel; st: Json; now: number; live: boolean }) {
  const [open, setOpen] = useState(false);
  const tasks: Json[] = st.timeline?.tasks ?? [];
  const milestones: Mark[] = st.timeline?.milestones ?? [];
  if (!tasks.length && !milestones.length) return null;
  return (
    <section class="progress">
      <div class="row-inline">
        <h2 class="title3">{t`进展`}</h2>
        <button type="button" class="link callout" title={t`按时间看谁做了什么、交给了谁`} onClick={() => setOpen(!open)}>{open ? t`收起进展` : t`展开进展`}</button>
        <span class="grow" />
        <a class="link callout" {...link(runLink(run.id, "audit"))}>{t`在审计日志中查看全部`}</a>
      </div>
      {open && (
        <Card pad={12}>
          <div class="timeline-scroll">
            <RunTimeline run={run} tasks={tasks} milestones={milestones} now={now} live={live} />
          </div>
        </Card>
      )}
    </section>
  );
}

const LABEL_W = 220;
const ROW_H = 40;
const AXIS_H = 22;
const CHART_W = 640;

function RunTimeline({ run, tasks, milestones, now, live }: { run: RunModel; tasks: Json[]; milestones: Mark[]; now: number; live: boolean }) {
  const laneH = milestones.length ? 32 : 0;
  const queued = tasks.some((x) => x.waiting);
  const height = AXIS_H + laneH + tasks.length * ROW_H;
  const w = CHART_W;
  const spans: [number, number][] = [];
  for (const x of tasks) for (const b of x.bars) spans.push([b.start, b.end ?? now]);
  for (const x of tasks) for (const m of [x.result, ...(x.artifacts ?? [])].filter(Boolean)) spans.push([m.ts, m.ts]);
  for (const m of milestones) spans.push([m.ts, m.ts]);
  if (live || queued) spans.push([now, now]);
  const s = new TimeScale(spans, w, 12, queued ? 132 : 56, live);
  const rowTop = (i: number) => AXIS_H + laneH + i * ROW_H;
  const nx = s.x(now);
  const ticks = s.ticks().filter((tk) => !live || tk.x < nx - 52 || tk.x > nx + 34);
  const waitingX0 = nx + 8;
  const openMark = (m: Mark) => go(runLink(run.id, `audit/${m.seq}`));
  const index = new Map(tasks.map((x, i) => [x.id, i]));
  let lastMilestone = -Infinity;
  const placed = [...milestones].sort((a, b) => a.ts - b.ts).map((m) => {
    const px = Math.max(s.x(m.ts), lastMilestone + 20);
    lastMilestone = px;
    return [m, px] as const;
  });
  // Connectors from each upstream task's last bar to where the task it handed over to starts.
  const paths: string[] = [];
  tasks.forEach((x, i) => {
    const xs = x.bars[0] ? s.x(x.bars[0].start) : x.waiting ? waitingX0 : null;
    if (xs === null) return;
    const ys = rowTop(i) + ROW_H / 2;
    for (const d of x.deps ?? []) {
      const j = index.get(d);
      const up = j === undefined ? undefined : tasks[j]!.bars.at(-1);
      if (j === undefined || !up) continue;
      const xe = s.x(up.end ?? now);
      const ye = rowTop(j) + ROW_H / 2;
      const xm = xe + 6;
      const body = xs - 4 >= xm ? `M${xe},${ye}L${xm},${ye}L${xm},${ys}` : `M${xe},${ye}L${xm},${ye}L${xm},${rowTop(i) - 1}L${xs - 8},${rowTop(i) - 1}L${xs - 8},${ys}`;
      paths.push(`${body}L${xs - 1},${ys}M${xs - 5},${ys - 3.5}L${xs - 1},${ys}L${xs - 5},${ys + 3.5}`);
    }
  });
  return (
    <div class="timeline" style={{ height: `${height}px` }}>
      <div class="timeline-labels" style={{ width: `${LABEL_W}px` }}>
        <div style={{ height: `${AXIS_H}px` }} />
        {laneH > 0 && (
          <div class="caption secondary lane-label" style={{ height: `${laneH}px` }} title={t`◆ 决定 · 红色：等你处理 · 橙色：负责人在处理。悬停图标看是什么，点按打开`}>
            <Icon name="diamond" size={12} /> {t`决定与待处理`}
          </div>
        )}
        {tasks.map((x) => {
          const [label, tone] = taskLabel(x.status);
          const mw = who(run, x.agent);
          return (
            <a class="timeline-label" style={{ height: `${ROW_H}px` }} title={`${x.id} ${x.title}`} {...link(taskLink(run.id, x.id))}>
              <StatusGlyph tone={tone} label={label} />
              <span class="grow min0">
                <span class="callout ellipsis block">{x.title}</span>
                <span class="caption secondary ellipsis row-inline">
                  <Avatar role={mw.role} size={14} />
                  {`${mw.name} · ${x.id}`}
                </span>
              </span>
            </a>
          );
        })}
      </div>
      <div class="timeline-chart" style={{ width: `${w}px`, height: `${height}px` }}>
        {tasks.map((_, i) => <div class="tl-row-line" style={{ top: `${rowTop(i)}px` }} />)}
        {ticks.map((tk) => (
          <>
            <div class="tl-grid" style={{ left: `${tk.x}px`, top: `${AXIS_H}px`, height: `${height - AXIS_H}px` }} />
            <span class="tl-tick caption secondary mono" style={{ left: `${tk.x + 3}px` }}>{tk.seconds ? clock(tk.ts) : clock(tk.ts).slice(0, 5)}</span>
          </>
        ))}
        {s.breaks().map((b) => (
          <div class="tl-break" style={{ left: `${b.x + 4}px`, top: `${AXIS_H}px`, width: `${TimeScale.breakW - 8}px`, height: `${height - AXIS_H}px` }} title={t`这段时间没有进展：${idle(b.ms)}`} aria-label={t`摸鱼 ${idle(b.ms)}`}>…</div>
        ))}
        <svg class="tl-connectors" width={w} height={height} aria-hidden="true">
          {paths.map((d) => <path d={d} />)}
        </svg>
        {tasks.map((x, i) => {
          const y = rowTop(i) + (ROW_H - 22) / 2;
          const mw = who(run, x.agent);
          const last = x.bars.at(-1);
          return (
            <>
              {x.bars.map((b: Json) => {
                const end = b.end ?? now;
                const x0 = s.x(b.start);
                const width = Math.max(s.x(end), x0 + 4) - x0;
                const [label, tone] = taskLabel(b.status);
                const d = dur(end - b.start);
                const text = b.running ? t`进行中` : b.attempt === 1 ? d : width > 130 ? t`第 ${b.attempt} 次 · ${d}` : t`第 ${b.attempt} 次`;
                const cls = tone === "red" ? "failed" : b.running ? "running" : "done";
                return (
                  <a class={`tl-bar ${cls}`} style={{ left: `${x0}px`, top: `${y}px`, width: `${width}px`, ...(cls === "done" ? { "--bar": mw.role.color || "#5b6675" } : {}) }}
                    title={t`${x.id} ${x.title}\n尝试 ${b.attempt} · ${label} · ${clock(b.start)} – ${b.running ? t`进行中` : clock(end)}（${d}）\n点按查看子任务`}
                    {...link(taskLink(run.id, x.id))}>
                    {width > 64 ? text : ""}
                  </a>
                );
              })}
              {x.waiting && (
                <a class="tl-waiting caption secondary" style={{ left: `${waitingX0}px`, top: `${y}px`, width: `${Math.max(40, w - waitingX0 - 12)}px` }} title={t`${x.id} ${x.title}\n还没开始 · ${x.waiting}\n点按查看子任务`} {...link(taskLink(run.id, x.id))}>
                  {x.waiting}
                </a>
              )}
              {last && (x.result || (x.artifacts ?? []).length > 0) && (
                <span class="tl-outcome" style={{ left: `${Math.min(s.x(last.end ?? now) + 6, w - 52)}px`, top: `${y + 2}px` }}>
                  {x.result && (
                    <button type="button" class={`tl-mark fg-${markTone(x.result)}`} title={t`结论 · ${clock(x.result.ts)}\n${x.result.title}\n点按在审计日志中查看`} onClick={() => openMark(x.result)}>
                      <Icon name={symbol(x.result)} size={12} />
                    </button>
                  )}
                  {(x.artifacts ?? []).length > 0 && (
                    <button type="button" class="tl-mark fg-blue" title={t`产物 ${x.artifacts.length} 个\n${x.artifacts.map((a: Mark) => a.title).join("\n")}\n点按在审计日志中查看`} onClick={() => openMark(x.artifacts.at(-1))}>
                      <Icon name="doc.fill" size={12} />
                      {x.artifacts.length > 1 && <span class="caption">{x.artifacts.length}</span>}
                    </button>
                  )}
                </span>
              )}
            </>
          );
        })}
        {placed.map(([m, px]) => {
          const name = m.by === "user" ? t`你` : who(run, m.by).name;
          return (
            <button type="button" class={`tl-milestone fg-${markTone(m)}`} style={{ left: `${px - 10}px`, top: `${AXIS_H + (laneH - 20) / 2}px` }}
              title={t`${kindName(m)} · ${name} · ${clock(m.ts)}\n${m.title}\n点按在审计日志中查看`} aria-label={t`${kindName(m)}，${name}，${clock(m.ts)}，${m.title}`} onClick={() => openMark(m)}>
              <Icon name={symbol(m)} size={13} />
            </button>
          );
        })}
        {live && (
          <>
            <div class="tl-now" style={{ left: `${nx}px`, top: `${AXIS_H}px`, height: `${height - AXIS_H}px` }} />
            <span class="tl-now-label caption" style={{ left: `${nx - 12}px` }}>{t`现在`}</span>
          </>
        )}
      </div>
    </div>
  );
}
