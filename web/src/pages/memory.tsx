// A workspace's team memory (Mac: UI/MemoryView.swift): its notes, one line each (kind and
// title, the line the index gives and who wrote it under it), its consolidation, and the page to
// read and change one note. The words are the service's (`GET /api/workspaces/{id}/memories`,
// docs/architecture/20261031-frontend.md#team_memory).

import type { ComponentChildren } from "preact";
import { useSignal } from "@preact/signals";
import { useEffect } from "preact/hooks";
import { api, type Json } from "../lib/api.ts";
import { dateTime } from "../lib/format.ts";
import { t } from "../lib/i18n.ts";
import { go, link } from "../lib/router.ts";
import { closeSheet, confirm } from "../lib/confirm.ts";
import { loadMemories, loadRuns, memories, role, show, workspace } from "../lib/store.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { Field, FormPage, Select, TextArea, TextInput } from "../ui/form.tsx";
import { Avatar, Button, Card, IconButton, Note, Pill } from "../ui/kit.tsx";
import { briefDraft } from "./workspace-edit.tsx";
import "./memory.css";
import "./loops.css";

/** Who wrote a note last and where, as the service says it, and when. */
export function source(m: Json): string {
  return `${m.source} · ${dateTime(m.updatedAt ?? 0)}`;
}

const HOW = () => [
  t`Agent 写的：负责人、协作者和值班在任务里记下之后还用得上的事（怎么跑测试、踩过的坑、为什么这样做），每条记着谁在哪个任务里写的。`,
  t`不会塞进每个任务的说明里：Agent 只知道这里有多少条，需要时按关键词检索，拿到最相关的几条。它是笔记不是规则：用之前先核对，和共享资料冲突时以共享资料为准。`,
  t`攒够 5 个交付的任务、且距上次超过一天，负责人自动整理一次：合并重复的、改正过时的、删掉没用的；也可以点右上角现在整理。`,
  t`你可以改、删，或把一条加进共享资料，让它变成团队必须遵守的规则。`,
];

export function MemoryPage({ id }: { id: string }) {
  usePageTitle(t`团队记忆`, `/workspaces/${encodeURIComponent(id)}`);
  const all = useSignal(false);
  const how = useSignal(false);
  useEffect(() => void loadMemories(id), [id]);
  const m = memories.value[id];
  const notes: Json[] = m?.memories ?? [];
  const c = m?.consolidation ?? {};
  const rows = all.value ? notes : notes.slice(0, 5);
  useDrawn("memory", () => ({
    workspace: id,
    status: m?.status ?? null,
    notes: notes.map((n) => ({ id: n.id, kind: n.kindLabel, title: n.title, hook: n.hook, source: source(n) })),
    running: c.running ?? null,
    last: c.last?.run ?? null,
  }));
  const consolidate = async () => {
    try {
      const r = await api.post(`/api/workspaces/${encodeURIComponent(id)}/memories/consolidate`, {});
      await Promise.all([loadMemories(id), loadRuns()]);
      go(`/runs/${encodeURIComponent(r.run)}`);
    } catch (e) {
      show((e as Error).message);
    }
  };
  return (
    <div class="page">
      <div class="section-header">
        <h1 class="display">{t`团队记忆`}</h1>
        {notes.length > 0 && <span class="count">{notes.length}</span>}
        <IconButton icon="questionmark.circle" label={t`怎样运作`} onClick={() => (how.value = !how.value)} />
        <span class="grow" />
        {c.running ? (
          <a {...link(`/runs/${encodeURIComponent(c.running)}`)} title={t`打开这次整理`}>
            <Pill text={t`整理中`} tone="blue" />
          </a>
        ) : (
          notes.length > 0 && (
            <Button small title={t`现在整理：由负责人合并重复的、改正过时的、删掉没用的笔记`} onClick={() => void consolidate()}>
              {t`现在整理`}
            </Button>
          )
        )}
      </div>
      <div class="secondary">{workspace(id)?.name}</div>
      {how.value && (
        <Card>
          <ul class="how">
            {HOW().map((l) => (
              <li>{l}</li>
            ))}
          </ul>
        </Card>
      )}
      {notes.length === 0 ? (
        <Note text={t`还没有笔记。Agent 在任务里记下的会出现在这里。`} />
      ) : (
        <Card pad={4} class="rows">
          {rows.map((n) => (
            <a class="row note-row" title={`${n.hook}\n${source(n)}`} {...link(`/workspaces/${encodeURIComponent(id)}/memory/${encodeURIComponent(n.id)}`)}>
              <Pill text={n.kindLabel} tone={n.kind === "pitfall" ? "amber" : "gray"} />
              <span class="grow note-lines">
                <span class="ellipsis">{n.title}</span>
                <span class="caption ellipsis">{n.hook}</span>
              </span>
            </a>
          ))}
        </Card>
      )}
      <div class="memory-foot">
        {notes.length > 5 && (
          <Button kind="plain" small onClick={() => (all.value = !all.value)}>
            {all.value ? t`收起` : t`展开全部（${notes.length}）`}
          </Button>
        )}
        <span class="grow" />
        {!c.running && c.last?.run && (
          <a class="caption" {...link(`/runs/${encodeURIComponent(c.last.run)}`)}>
            {t`上次整理：${dateTime(c.last.at ?? 0)}`}
          </a>
        )}
      </div>
    </div>
  );
}

/** One note, to read and change; to delete, or to add to the shared material, which every session then follows as the person's own. */
export function NotePage({ id, note }: { id: string; note: string }) {
  const back = `/workspaces/${encodeURIComponent(id)}/memory`;
  usePageTitle(t`团队记忆`, back);
  useEffect(() => void loadMemories(id), [id]);
  const m = memories.value[id];
  const original: Json | undefined = (m?.memories ?? []).find((n: Json) => n.id === note);
  const kind = useSignal<string | null>(null);
  const title = useSignal<string | null>(null);
  const hook = useSignal<string | null>(null);
  const body = useSignal<string | null>(null);
  const saving = useSignal(false);
  useDrawn("note", () => ({ id: note, found: !!original, source: original ? source(original) : null }));
  if (!m) return <div class="page" />;
  if (!original) {
    return (
      <div class="page">
        <Note text={t`这条笔记已经不在了。`} />
      </div>
    );
  }
  const v = {
    kind: kind.value ?? original.kind,
    title: title.value ?? original.title,
    hook: hook.value ?? original.hook,
    body: body.value ?? original.body,
  };
  const dirty = v.kind !== original.kind || v.title !== original.title || v.hook !== original.hook || v.body !== original.body;
  const valid = [v.title, v.hook, v.body].every((x: string) => x.trim() !== "");
  const save = async (): Promise<boolean> => {
    saving.value = true;
    try {
      await api.put(`/api/memories/${encodeURIComponent(note)}`, v);
      await loadMemories(id);
      return true;
    } catch (e) {
      show(t`保存笔记失败：${(e as Error).message}`);
      return false;
    } finally {
      saving.value = false;
    }
  };
  const remove = async () => {
    if (!(await confirm({ title: t`删除笔记「${original.title}」？`, text: t`之后的任务不会再读到它。`, ok: t`删除` }))) return;
    try {
      await api.delete(`/api/memories/${encodeURIComponent(note)}`);
      await loadMemories(id);
      go(back, true);
    } catch (e) {
      show((e as Error).message);
    }
  };
  // The line the service writes for the note as saved: changes here are saved first.
  const addToBrief = async () => {
    if (dirty && !(valid && (await save()))) return;
    const line = (memories.value[id]?.memories ?? []).find((n: Json) => n.id === note)?.briefLine ?? original.briefLine;
    briefDraft.value = { workspace: id, lines: [line] };
    go(`/workspaces/${encodeURIComponent(id)}/brief`);
  };
  return (
    <FormPage
      title={t`团队记忆`}
      sub={source(original)}
      footer={
        <>
          <Button onClick={() => void remove()}>{t`删除…`}</Button>
          <Button title={t`把这条笔记作为一行加进共享资料，保存后每个任务都按它来`} disabled={!workspace(id)} onClick={() => void addToBrief()}>
            {t`加入共享资料…`}
          </Button>
          <span class="spacer" />
          <Button onClick={() => void closeSheet(dirty, () => go(back))}>{t`取消`}</Button>
          <Button kind="primary" disabled={saving.value || !dirty || !valid} onClick={() => void save().then((ok) => ok && go(back))}>
            {saving.value ? t`正在保存…` : t`保存`}
          </Button>
        </>
      }
    >
      <div class="note-head">
        <Select label={t`类型`} value={v.kind} options={(m.kinds ?? []).map((k: Json) => [k.id, k.label] as [string, string])} onChange={(k) => (kind.value = k)} />
        <TextInput value={v.title} placeholder={t`标题`} label={t`标题`} onInput={(x) => (title.value = x)} />
      </div>
      <TextInput value={v.hook} placeholder={t`一句话摘要：之后的任务检索到这条时先看到这一行`} label={t`一句话摘要：之后的任务检索到这条时先看到这一行`} onInput={(x) => (hook.value = x)} />
      <Field note={t`这是 Agent 记的笔记，之后的任务读到后会先核对再用。要团队必须遵守，加入共享资料。`}>
        <TextArea value={v.body} label={t`正文`} rows={10} onInput={(x) => (body.value = x)} />
      </Field>
    </FormPage>
  );
}

/** A consolidation's page: the workspace lead's one turn over the notes, with what it changed.
 * `session` is the conversation card of the run page, given by the page that shows this one. */
export function DreamPage({ run, session }: { run: Json; session?: ComponentChildren }) {
  const wsId: string = run.workspace ?? "";
  const ws = workspace(wsId);
  usePageTitle(t`整理团队记忆`, ws ? `/workspaces/${encodeURIComponent(wsId)}/memory` : "/inbox");
  // What it changed, as its last event says (`memory.consolidated`).
  const done = useSignal<Json>(null);
  useEffect(() => {
    api.get(`/api/runs/${encodeURIComponent(run.id)}`).then(
      (j) => (done.value = [...(j.events ?? [])].reverse().find((e: Json) => e.type === "memory.consolidated")?.data ?? null),
      () => {},
    );
  }, [run.id, run.status]);
  const lead = ws ? role(ws.lead) : undefined;
  useDrawn("dream", () => ({ id: run.id, status: run.status, done: done.value }));
  return (
    <div class="page wide">
      <div class="loop-head">
        <Avatar role={lead} size={34} />
        <div class="grow">
          <h1 class="display loop-name">{t`整理团队记忆`}</h1>
          <div class="caption">{[lead?.name, ws?.name].filter(Boolean).join(" · ")}</div>
        </div>
        <Button disabled={!ws} onClick={() => go(`/workspaces/${encodeURIComponent(wsId)}`)}>
          {t`打开工作区`}
        </Button>
      </div>
      {done.value ? (
        <Note text={t`整理完了：改了 ${done.value.saved ?? 0} 条，删了 ${done.value.forgotten ?? 0} 条。${done.value.summary ?? ""}`} tone="green" />
      ) : (
        (run.status === "failed" || run.status === "interrupted") && (
          <Note text={t`这次整理没有完成：${run.error || t`会话中断了`}。已经改的笔记保留，可以在工作区页重新整理。`} tone="amber" />
        )
      )}
      {session}
    </div>
  );
}
