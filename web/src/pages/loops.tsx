// Workspace Loops (Mac: UI/LoopView.swift): the form to make or change one, and a Loop's page
// (what it is and does now, its session, what it dispatched). How a Loop reads (its schedule in
// words, its state, when it wakes) is the service's (`GET /api/loops`); the form only writes the
// cron expression of the shape picked, and the service reads it back and says when it comes round
// (docs/architecture/20261031-frontend.md#loops).

import { useSignal } from "@preact/signals";
import { useEffect } from "preact/hooks";
import { api, type Json } from "../lib/api.ts";
import { dateTime } from "../lib/format.ts";
import { serverLang, t } from "../lib/i18n.ts";
import { go, link } from "../lib/router.ts";
import { ask, loadLoops, loadRuns, loops, loopOfRun, role, show, workspace } from "../lib/store.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { Field, FormPage, Radio, Select, TextArea, TextInput } from "../ui/form.tsx";
import { Button, Card, Menu, Note, Pill } from "../ui/kit.tsx";
import { LoopBadge } from "./workspace.tsx";
import "./loops.css";

const MINUTE_CHOICES = [5, 10, 15, 20, 30];
const HOUR_CHOICES = [1, 2, 3, 4, 6, 8, 12];
const pad = (n: number) => String(n).padStart(2, "0");
const weekday = (d: number) => [t`周日`, t`周一`, t`周二`, t`周三`, t`周四`, t`周五`, t`周六`][d % 7]!;

type Shape = "minutes" | "hours" | "daily" | "weekdays" | "weekly" | "monthly" | "cron";
type Form = { shape: Shape; minutes: number; hours: number; hour: number; minute: number; weekday: number; monthDay: number; cron: string };

/** The cron expression of the shape the form says (Mac: `LoopSchedule.cron`). */
function cronOf(f: Form, shape: Shape = f.shape): string {
  switch (shape) {
    case "minutes":
      return `*/${f.minutes} * * * *`;
    case "hours":
      return f.hours === 1 ? "0 * * * *" : `0 */${f.hours} * * *`;
    case "weekdays":
      return `${f.minute} ${f.hour} * * 1-5`;
    case "weekly":
      return `${f.minute} ${f.hour} * * ${f.weekday}`;
    case "monthly":
      return `${f.minute} ${f.hour} ${f.monthDay} * *`;
    case "cron":
      return f.cron.trim();
    default:
      return `${f.minute} ${f.hour} * * *`;
  }
}

/** The form set from a Loop's schedule, as the service reads its expression (`scheduleShape`). */
function formOf(loop: Json | undefined): Form {
  const s = loop?.scheduleShape ?? { kind: "daily", hour: 9, minute: 0 };
  return {
    shape: s.kind,
    minutes: s.minutes ?? 30,
    hours: s.hours ?? 1,
    hour: s.hour ?? 9,
    minute: s.minute ?? 0,
    weekday: s.kind === "weekly" ? s.day : 1,
    monthDay: s.kind === "monthly" ? s.day : 1,
    cron: loop?.schedule?.cron ?? "",
  };
}

const EXAMPLES = () => [
  [t`分拣 GitHub issue`, t`用 gh 看 <owner/repo> 新开和有更新的 issue。能复现的 bug 和范围清楚的需求派成任务，key 用 github:<owner/repo>#<编号>；只是提问的，直接在 issue 下回答；需求不清楚的问我。`],
  [t`每天看 CI`, t`看 <owner/repo> 主分支最近的 CI。有失败就找出原因：能修的派成任务，key 用 ci:<运行编号>；偶发的先记下来，连续出现再派。`],
  [t`盯依赖更新`, t`看项目的依赖有没有新版本。补丁版本直接派成升级任务；大版本先读更新说明，有破坏性改动的问我。key 用 dep:<包名>@<版本>。`],
];

/** Make a Loop in workspace `ws`, or change Loop `loopId`. */
export function LoopEditor({ ws, loopId }: { ws?: string; loopId?: string }) {
  const original: Json | undefined = loopId ? loops.value.find((l) => l.id === loopId) : undefined;
  const isNew = !loopId;
  const wsId: string = ws ?? original?.workspace ?? "";
  const back = original ? `/runs/${encodeURIComponent(original.run)}` : `/workspaces/${encodeURIComponent(wsId)}`;
  usePageTitle(isNew ? t`新建值班` : t`编辑值班`, back);
  const name = useSignal<string | null>(null);
  const prompt = useSignal<string | null>(null);
  const scheduled = useSignal<boolean | null>(null);
  const form = useSignal<Form | null>(null);
  const upcoming = useSignal<number[]>([]);
  const problem = useSignal<string | null>(null);
  const tz = useSignal<string>("");
  const saving = useSignal(false);
  const f = form.value ?? formOf(original);
  const on = scheduled.value ?? (isNew || !!original?.schedule?.cron);
  const cron = cronOf(f);
  const v = { name: name.value ?? original?.name ?? "", prompt: prompt.value ?? original?.prompt ?? "" };

  // When the schedule comes round, as the service reads it, a moment after the form stops changing.
  useEffect(() => {
    if (!on) {
      upcoming.value = [];
      problem.value = null;
      return;
    }
    const timer = setTimeout(async () => {
      if (!cron) {
        upcoming.value = [];
        problem.value = t`写一条 5 段的 cron 表达式：分 时 日 月 星期`;
        return;
      }
      const zone = original?.schedule?.tz ? `&tz=${encodeURIComponent(original.schedule.tz)}` : "";
      try {
        const r = await api.get(`/api/loops/schedule?cron=${encodeURIComponent(cron)}${zone}&lang=${serverLang()}`);
        upcoming.value = r.next ?? [];
        problem.value = null;
        tz.value = r.tz ?? "";
      } catch (e) {
        upcoming.value = [];
        problem.value = (e as Error).message;
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [on, cron]);

  useDrawn("loopEditor", () => ({ id: loopId ?? null, workspace: wsId, scheduled: on, cron: on ? cron : null, upcoming: upcoming.value, problem: problem.value }));
  if (loopId && !original) return <div class="page" />;
  const space = workspace(wsId);
  const lead = space ? role(space.lead) : undefined;
  const set = (patch: Partial<Form>) => (form.value = { ...f, ...patch });
  const valid = v.prompt.trim() !== "" && (!on || (problem.value === null && upcoming.value.length > 0));
  const save = async (wakeNow: boolean) => {
    saving.value = true;
    // A new Loop keeps the service's own time zone (the Mac's); a changed one keeps its own.
    const when = on ? { cron, ...(original?.schedule?.tz ? { tz: original.schedule.tz } : {}) } : null;
    const body = { name: v.name, prompt: v.prompt, schedule: when, wakeNow };
    try {
      if (isNew) {
        const made = await api.post(`/api/workspaces/${encodeURIComponent(wsId)}/loops`, body);
        await Promise.all([loadLoops(), loadRuns()]);
        go(`/runs/${encodeURIComponent(made.run)}`, true);
      } else {
        await api.put(`/api/loops/${encodeURIComponent(loopId!)}`, body);
        await loadLoops();
        go(back, true);
      }
    } catch (e) {
      show(t`保存值班失败：${(e as Error).message}`);
    }
    saving.value = false;
  };
  const note = on ? t`按 Mac 上的时间（${tz.value}）。上次没干完就顺延；Mac 睡着错过的，醒来补一次。它也能自己约个时间再来，值班页会写明。` : t`只在你点「现在开工」或回答它时开工；它也能自己约一次，值班页会写明。`;
  const time = (
    <span class="loop-time">
      <Select label={t`时`} value={f.hour} options={[...Array(24).keys()].map((h) => [h, pad(h)] as [number, string])} onChange={(h) => set({ hour: h })} />
      <span>:</span>
      <Select
        label={t`分`}
        value={f.minute}
        options={[...new Set([...Array(12).keys()].map((i) => i * 5).concat(f.minute))].sort((a, b) => a - b).map((m) => [m, pad(m)] as [number, string])}
        onChange={(m) => set({ minute: m })}
      />
    </span>
  );
  return (
    <FormPage
      title={isNew ? t`新建值班` : t`编辑值班`}
      sub={
        <>
          {space && <div>{lead ? t`在「${space.name}」值班，由负责人「${lead.name}」来跑` : t`在「${space.name}」值班`}</div>}
          <div>{t`它自己不改文件，要改的事派成任务，结果到收件箱。`}</div>
        </>
      }
      footer={
        <>
          <span class="spacer" />
          <Button onClick={() => go(back)}>{t`取消`}</Button>
          {isNew ? (
            <>
              <Button disabled={!valid || saving.value} onClick={() => void save(false)}>
                {t`只创建`}
              </Button>
              <Button kind="primary" disabled={!valid || saving.value} onClick={() => void save(true)}>
                {t`创建并马上开工`}
              </Button>
            </>
          ) : (
            <Button kind="primary" disabled={!valid || saving.value} onClick={() => void save(false)}>
              {t`保存`}
            </Button>
          )}
        </>
      }
    >
      <Field label={t`名字`}>
        <TextInput value={v.name} placeholder={t`不填就用提示的第一行`} onInput={(x) => (name.value = x)} />
      </Field>
      <Field label={t`要它做什么`}>
        <TextArea value={v.prompt} rows={7} placeholder={t`每次开工都把这段话交给它。写清楚看哪里、什么情况派成任务、什么情况问你。`} onInput={(x) => (prompt.value = x)} />
      </Field>
      <div class="loop-examples">
        <span class="caption">{t`从示例开始：`}</span>
        {EXAMPLES().map(([n, text]) => (
          <Button small title={text} onClick={() => (prompt.value = text!)}>
            {n}
          </Button>
        ))}
      </div>
      <div class="field">
        <span class="field-label">{t`什么时候开工`}</span>
        <Radio on={on} onSelect={() => (scheduled.value = true)}>
          {t`按时间表`}
        </Radio>
        {on && (
          <div class="loop-when">
            <div class="loop-shape">
              <Select
                label={t`频率`}
                value={f.shape}
                options={[
                  ["minutes", t`每隔几分钟`],
                  ["hours", t`每隔几小时`],
                  ["daily", t`每天`],
                  ["weekdays", t`工作日`],
                  ["weekly", t`每周`],
                  ["monthly", t`每月`],
                  ["cron", t`写 cron 表达式`],
                ]}
                // Writing the expression starts from the one the form said.
                onChange={(s) => set({ shape: s as Shape, cron: s === "cron" && !f.cron ? cronOf(f) : f.cron })}
              />
              {f.shape === "minutes" && <Select label={t`分钟`} value={f.minutes} options={MINUTE_CHOICES.map((m) => [m, t`每 ${m} 分钟`] as [number, string])} onChange={(m) => set({ minutes: m })} />}
              {f.shape === "hours" && <Select label={t`小时`} value={f.hours} options={HOUR_CHOICES.map((h) => [h, h === 1 ? t`每小时` : t`每 ${h} 小时`] as [number, string])} onChange={(h) => set({ hours: h })} />}
              {f.shape === "weekly" && <Select label={t`星期`} value={f.weekday} options={[1, 2, 3, 4, 5, 6, 0].map((d) => [d, weekday(d)] as [number, string])} onChange={(d) => set({ weekday: d })} />}
              {f.shape === "monthly" && <Select label={t`日`} value={f.monthDay} options={[...Array(31).keys()].map((i) => [i + 1, t`${i + 1} 日`] as [number, string])} onChange={(d) => set({ monthDay: d })} />}
              {["daily", "weekdays", "weekly", "monthly"].includes(f.shape) && time}
              {f.shape === "cron" && <TextInput mono value={f.cron} placeholder={t`分 时 日 月 星期`} label={t`分 时 日 月 星期`} onInput={(x) => set({ cron: x })} />}
            </div>
            {problem.value ? <div class="caption fg-red">{problem.value}</div> : upcoming.value.length > 0 && <div class="caption">{t`接下来：${upcoming.value.map((x) => dateTime(x)).join(" · ")}`}</div>}
            <div class="caption">{note}</div>
          </div>
        )}
        <Radio on={!on} onSelect={() => (scheduled.value = false)}>
          {t`只在你叫它时`}
        </Radio>
        {!on && <div class="caption loop-when">{note}</div>}
      </div>
    </FormPage>
  );
}

/** A Loop's page: what it is and does now, its session, and what it dispatched. `session` is the
 * conversation card of the run page, given by the page that shows this one. */
export function LoopPage({ run, session, asks }: { run: Json; session?: preact.ComponentChildren; asks?: preact.ComponentChildren }) {
  const loop = loopOfRun(run.id);
  usePageTitle(loop?.title ?? run.title ?? "", loop ? `/workspaces/${encodeURIComponent(loop.workspace)}` : "/inbox");
  const dispatched = useSignal<Json[]>([]);
  const next = useSignal<string | null>(null);
  const counts = JSON.stringify(loop?.status?.dispatched ?? {});
  // Its counts change with every dispatch and every change of a dispatched run's status.
  useEffect(() => {
    if (!loop) return;
    api.get(`/api/runs?loop=${encodeURIComponent(loop.id)}&limit=100&lang=${serverLang()}`).then(
      (r) => {
        dispatched.value = r.runs ?? [];
        next.value = r.next ?? null;
      },
      () => {},
    );
  }, [loop?.id, counts]);
  useDrawn("loop", () =>
    loop
      ? { id: loop.id, title: loop.title, pill: loop.pill.text, schedule: loop.scheduleText, wakes: loop.status.wakes, dispatchedCount: loop.dispatchedCount, dispatched: dispatched.value.map((r) => r.id) }
      : null,
  );
  if (!loop) return <div class="page" />;
  const act = async (action: "wake" | "pause" | "resume") => {
    try {
      await api.post(`/api/loops/${encodeURIComponent(loop.id)}/${action}`, {});
      await loadLoops();
    } catch (e) {
      show((e as Error).message);
    }
  };
  const remove = async () => {
    if (!ask(t`删除值班「${loop.title}」？`, t`它的会话和记录一起删掉，找不回来；派出的任务保留。`)) return;
    try {
      await api.delete(`/api/loops/${encodeURIComponent(loop.id)}`);
      await Promise.all([loadLoops(), loadRuns()]);
      go(`/workspaces/${encodeURIComponent(loop.workspace)}`, true);
    } catch (e) {
      show((e as Error).message);
    }
  };
  const more = async () => {
    try {
      const r = await api.get(`/api/runs?loop=${encodeURIComponent(loop.id)}&limit=100&lang=${serverLang()}&cursor=${encodeURIComponent(next.value ?? "")}`);
      dispatched.value = [...dispatched.value, ...(r.runs ?? [])];
      next.value = r.next ?? null;
    } catch (e) {
      show((e as Error).message);
    }
  };
  // Its lead: the workspace's (a Loop is run by the lead of its workspace).
  const lead = role(workspace(loop.workspace)?.lead ?? "")?.name ?? "";
  const pill = loop.wakesAt ? t`摸鱼中 · ${dateTime(loop.wakesAt)} 开工` : loop.pill.text;
  const asked = loop.asked ? (loop.asked.why ? t`它约了 ${dateTime(loop.asked.at)} 再看一次：${loop.asked.why}` : t`它约了 ${dateTime(loop.asked.at)} 再看一次`) : null;
  return (
    <div class="page wide loop-page">
      <div class="loop-head">
        <LoopBadge size={34} />
        <div class="grow">
          <div class="loop-title-row">
            <h1 class="display loop-name">{loop.title}</h1>
            <Pill text={pill} tone={loop.pill.tone} />
          </div>
          <div class="caption ellipsis">{[loop.scheduleText, lead, t`开工 ${loop.status.wakes ?? 0} 次`, t`派出 ${loop.dispatchedCount} 个任务`].filter(Boolean).join(" · ")}</div>
          {asked && <div class="caption">{asked}</div>}
        </div>
        <div class="loop-actions">
          <Button disabled={loop.status.phase === "awake"} title={t`不等时间表，叫它现在就干一次活`} onClick={() => void act("wake")}>
            {t`现在开工`}
          </Button>
          {loop.state === "paused" ? (
            <Button onClick={() => void act("resume")}>{t`继续`}</Button>
          ) : (
            <Button title={t`停下它的时间表；暂停时仍可以叫它开工`} onClick={() => void act("pause")}>
              {t`暂停`}
            </Button>
          )}
          <Menu
            icon="ellipsis.circle"
            label={t`更多`}
            items={[
              { label: t`编辑…`, onSelect: () => go(`/loops/${encodeURIComponent(loop.id)}/edit`) },
              { label: t`删除值班…`, onSelect: () => void remove() },
            ]}
          />
        </div>
      </div>
      {loop.status.watchesFiles === false && <Note text={t`不是 Git 仓库：值班直接改你的目录，Janus 看不出它改了什么，也就没法提醒你。`} tone="amber" />}
      {run.status === "failed" && run.error && <Note text={t`会话已结束：${run.error} · 下次开工时再接上`} tone="amber" />}
      <div class="loop-body">
        <div class="loop-session">{session}</div>
        <aside class="loop-side">
          {asks}
          <span class="h2">{t`派出的任务`}</span>
          {dispatched.value.length === 0 ? (
            <div class="secondary">{t`还没有派出过任务`}</div>
          ) : (
            <Card pad={4} class="rows">
              {dispatched.value.map((r) => (
                <a class="row dispatched-row" title={r.sourceUrl ?? ""} {...link(`/runs/${encodeURIComponent(r.id)}`)}>
                  <span class="dispatched-lines">
                    <span class="clamp2">{r.title}</span>
                    <span class="dispatched-meta">
                      <span class="caption ellipsis grow">{r.dispatchKey}</span>
                      <Pill text={r.label.text} tone={r.label.tone} />
                    </span>
                  </span>
                </a>
              ))}
              {next.value && (
                <Button kind="plain" small onClick={() => void more()}>
                  {t`更早的任务…`}
                </Button>
              )}
            </Card>
          )}
        </aside>
      </div>
    </div>
  );
}
