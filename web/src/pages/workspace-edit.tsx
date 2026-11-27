// A workspace's settings and its shared material, each on a page of its own (Mac: the sheets
// `WorkspaceEditor` and `BriefEditor` in UI/HomeView.swift). The web has no folder picker: the
// folder is typed and the service checks it as it is typed (`GET /api/inspect`), the same check
// the Mac app shows under its picker (docs/architecture/20261126-web.md#forms).

import { signal, useSignal } from "@preact/signals";
import { useEffect } from "preact/hooks";
import { api, type Json } from "../lib/api.ts";
import { serverLang, t } from "../lib/i18n.ts";
import { go } from "../lib/router.ts";
import { ask, connection, defaultTeam, deleteSetup, role, roleGroups, roles, saveSetup, show, workspace, workspaces } from "../lib/store.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { Field, FormPage, Radio, Stepper, Switch, TextArea, TextInput } from "../ui/form.tsx";
import { Avatar, Button, Menu, Note } from "../ui/kit.tsx";
import "./workspace-edit.css";

/** How many of one digital employee a workspace's team may run at once (the service's `MAX_SLOTS`). */
const MAX_SLOTS = 8;
/** The longest shared material (the service's `BRIEF_LIMIT`): it goes into every session's prompt. */
const BRIEF_LIMIT = 8000;

/** Lines to add to a workspace's shared material when its page opens next (a note, or a lead's suggestions), as unsaved. */
export const briefDraft = signal<{ workspace: string; lines: string[] } | null>(null);

/** The shared material with `lines` added at the end, one bullet each (Mac: `BriefSuggestions.adding`). */
export function adding(brief: string, lines: string[]): string {
  const current = brief.trim();
  const added = lines.map((l) => `- ${l}`).join("\n");
  return current ? `${current}\n${added}` : added;
}

/** A workspace is saved whole: everything as it is saved now, with the parts changed here. */
function whole(ws: Json, changes: Json): Json {
  return { id: ws.id, name: ws.name, path: ws.path, lead: ws.lead, members: ws.members, brief: ws.brief, ...changes };
}

export function WorkspaceEditor({ id }: { id?: string }) {
  const original: Json | undefined = id ? workspace(id) : undefined;
  const isNew = !id;
  const back = isNew ? "/workspaces" : `/workspaces/${encodeURIComponent(id!)}`;
  usePageTitle(isNew ? t`新建工作区` : t`工作区设置`, back);
  const name = useSignal<string | null>(null);
  const path = useSignal<string | null>(null);
  const lead = useSignal<string | null>(null);
  const on = useSignal<Set<string> | null>(null);
  const slots = useSignal<Record<string, number> | null>(null);
  const inspect = useSignal<Json>(null);
  const saving = useSignal(false);
  // The form starts from the workspace as saved, or (a new one) the team the service proposes.
  const start = {
    name: original?.name ?? "",
    path: original?.path ?? "",
    lead: original?.lead || (isNew ? (defaultTeam.value?.lead ?? "") : ""),
    on: new Set<string>(isNew ? (defaultTeam.value?.members ?? []) : (original?.members ?? []).map((m: Json) => m.role)),
    slots: Object.fromEntries((original?.members ?? []).map((m: Json) => [m.role, Math.min(MAX_SLOTS, Math.max(1, m.slots ?? 1))])) as Record<string, number>,
  };
  const v = { name: name.value ?? start.name, path: path.value ?? start.path, lead: lead.value ?? start.lead, on: on.value ?? start.on, slots: slots.value ?? start.slots };
  const slotSpec = (x: typeof v) => [...x.on].sort().map((r) => `${r}:${x.lead === r ? 1 : Math.min(MAX_SLOTS, Math.max(1, x.slots[r] ?? 1))}`).join(",");
  const dirty = v.name !== start.name || v.path !== start.path || v.lead !== start.lead || slotSpec(v) !== slotSpec(start);

  // What the folder is, a moment after the person stops typing.
  useEffect(() => {
    const p = v.path.trim();
    if (!p) {
      inspect.value = null;
      return;
    }
    inspect.value = null;
    const timer = setTimeout(() => {
      api.get(`/api/inspect?path=${encodeURIComponent(p)}&lang=${serverLang()}`).then(
        (i) => (inspect.value = i),
        () => {},
      );
    }, 250);
    return () => clearTimeout(timer);
  }, [v.path]);

  useDrawn("workspaceEditor", () => ({ id: id ?? null, path: v.path, inspect: inspect.value?.text ?? null, on: [...v.on].sort(), lead: v.lead }));

  if (id && !original) {
    return (
      <div class="page">
        <Note text={t`没有这个工作区。`} />
      </div>
    );
  }
  const toggle = (r: string, enable: boolean) => {
    const next = new Set(v.on);
    if (enable) next.add(r);
    else {
      next.delete(r);
      if (v.lead === r) lead.value = "";
    }
    on.value = next;
  };
  const useGroup = (g: string) => {
    const group = defaultTeam.value?.groups?.[g] ?? { members: [], leads: [] };
    on.value = new Set(group.members);
    lead.value = group.leads.includes(v.lead) ? v.lead : (group.leads[0] ?? "");
  };
  const save = async () => {
    saving.value = true;
    const members = roles.value
      .map((r) => r.id)
      .filter((r) => v.on.has(r))
      .map((r) => {
        const n = v.lead === r ? 1 : Math.min(MAX_SLOTS, Math.max(1, v.slots[r] ?? 1));
        return n > 1 ? { role: r, slots: n } : { role: r };
      });
    // The workspace is saved whole: keep its shared material as saved now.
    const body = { id: original?.id ?? "", name: v.name, path: v.path, lead: v.lead, members, brief: original?.brief ?? "" };
    try {
      const ws = await saveSetup("workspaces", body);
      show(t`已保存工作区 ${ws.name}`);
      go(`/workspaces/${encodeURIComponent(ws.id)}`, true);
    } catch (e) {
      show((e as Error).message);
    }
    saving.value = false;
  };
  const remove = async () => {
    if (!original || !ask(t`删除工作区「${original.name}」？`, t`任务记录会保留，在侧栏的「团队」里还能找到。`)) return;
    try {
      await deleteSetup("workspaces", original.id);
      go(workspaces.value[0] ? `/workspaces/${encodeURIComponent(workspaces.value[0].id)}` : "/workspaces", true);
    } catch (e) {
      show((e as Error).message);
    }
  };
  const groups = roleGroups.value;
  const named = groups.filter((g) => g.group !== "");
  const insp = inspect.value;
  const line = !v.path.trim() ? t`先填一个目录` : (insp?.text ?? t`检查中…`);
  return (
    <FormPage
      title={isNew ? t`新建工作区` : t`工作区设置 · ${original!.name}`}
      footer={
        <>
          {!isNew && <Button onClick={() => void remove()}>{t`删除工作区…`}</Button>}
          <span class="spacer" />
          <Button onClick={() => (!dirty || ask(t`放弃未保存的更改？`, t`关闭后，这次的修改不会保存。`)) && go(back)}>{t`取消`}</Button>
          <Button kind="primary" disabled={saving.value || !v.path.trim() || insp?.exists === false} onClick={() => void save()}>
            {saving.value ? t`正在保存…` : isNew ? t`创建工作区` : t`保存`}
          </Button>
        </>
      }
    >
      <Field label={t`项目目录`} note={line} tone={insp?.exists === false ? "red" : undefined}>
        <TextInput mono value={v.path} placeholder="/path/to/your/repo" onInput={(x) => (path.value = x)} />
      </Field>
      <div class="caption">{t`网页上不能浏览这台 Mac 的文件夹：填目录的完整路径（可以用 ~ 开头）。`}</div>
      <Field label={t`名称`}>
        <TextInput value={v.name} placeholder={t`名称（可选，默认用目录名）`} onInput={(x) => (name.value = x)} />
      </Field>
      <div class="team-head">
        <span class="field-label grow">{t`启用的数字员工`}</span>
        {named.length > 1 && <Menu icon="person.2" label={t`按分组选…`} items={named.map((g) => ({ label: g.title, onSelect: () => useGroup(g.group) }))} />}
      </div>
      {roles.value.length === 0 && <Note text={t`还没有数字员工，先去「设置 › 数字员工」建一个。`} />}
      <div class="team-list">
        {groups.map((g) => (
          <>
            {groups.length > 1 && <div class="caption team-group">{g.title}</div>}
            {g.roles.map((rid: string) => {
              const r = role(rid);
              if (!r) return null;
              const c = connection(r.connection);
              const enabled = v.on.has(rid);
              const count = Math.min(MAX_SLOTS, Math.max(1, v.slots[rid] ?? 1));
              return (
                <div class={"team-row" + (enabled ? "" : " off")}>
                  <Switch on={enabled} label={t`启用 ${r.name}`} disabled={!c} onChange={(x) => toggle(rid, x)} />
                  <Avatar role={r} size={24} />
                  <div class="grow">
                    <div class="team-name">{r.name}</div>
                    <div class="caption ellipsis">{c ? `${c.name} · ${r.model || t`默认模型`}` : t`未绑定 Agent`}</div>
                    <div class="caption ellipsis team-perm">{r.permLabel || "—"}</div>
                  </div>
                  {enabled && v.lead !== rid && (
                    <Stepper label={t`分身数：大于 1 时可以同时接多个子任务`} value={count} min={1} max={MAX_SLOTS} onChange={(n) => (slots.value = { ...v.slots, [rid]: n })}>
                      {t`${count} 个分身`}
                    </Stepper>
                  )}
                  {r.canLead && (
                    <Radio on={v.lead === rid} disabled={!enabled} label={t`${r.name} 担任负责人`} onSelect={() => (lead.value = rid)}>
                      {t`负责人`}
                    </Radio>
                  )}
                </div>
              );
            })}
          </>
        ))}
      </div>
    </FormPage>
  );
}

export function BriefEditor({ id }: { id: string }) {
  const ws = workspace(id);
  const back = `/workspaces/${encodeURIComponent(id)}`;
  usePageTitle(t`共享资料`, back);
  // What it opens with: the saved text, with lines a note or a lead suggested added (unsaved).
  const draft = useSignal<string | null>(null);
  useEffect(() => {
    const add = briefDraft.value;
    if (ws && add && add.workspace === id) {
      draft.value = adding(ws.brief ?? "", add.lines);
      briefDraft.value = null;
    }
  }, [id, !!ws]);
  const saving = useSignal(false);
  useDrawn("brief", () => ({ id, text: draft.value ?? ws?.brief ?? "", saved: ws?.brief ?? "" }));
  if (!ws) {
    return (
      <div class="page">
        <Note text={t`没有这个工作区。`} />
      </div>
    );
  }
  const text = draft.value ?? ws.brief ?? "";
  const dirty = text !== (ws.brief ?? "");
  const over = [...text].length > BRIEF_LIMIT;
  const save = async () => {
    saving.value = true;
    try {
      await saveSetup("workspaces", whole(ws, { brief: text }));
      show(t`已保存共享资料`);
      go(back, true);
    } catch (e) {
      show((e as Error).message);
    }
    saving.value = false;
  };
  return (
    <FormPage
      title={t`共享资料 · ${ws.name}`}
      sub={t`产品定位、目标用户、约定、重要文件位置……每次任务都交给团队，写过的不再重复调研或问你。`}
      footer={
        <>
          {dirty && <span class="caption fg-amber">{t`还没保存 · 保存后下个任务起生效`}</span>}
          <span class="spacer" />
          <span class={"caption" + (over ? " fg-red" : "")}>{`${[...text].length} / ${BRIEF_LIMIT}`}</span>
          <Button onClick={() => (!dirty || ask(t`放弃未保存的更改？`, t`关闭后，这次的修改不会保存。`)) && go(back)}>{t`取消`}</Button>
          <Button kind="primary" disabled={saving.value || !dirty || over} onClick={() => void save()}>
            {saving.value ? t`正在保存…` : t`保存`}
          </Button>
        </>
      }
    >
      <TextArea value={text} label={t`共享资料`} rows={16} invalid={over} onInput={(x) => (draft.value = x)} />
      <ul class="how caption">
        <li>{t`你写的，只有你能改；Agent 不能直接写进来。`}</li>
        <li>{t`每个任务开始时原样交给负责人和每个协作者，值班开工时也带上；和本次目标冲突时以目标为准。`}</li>
        <li>{t`任务开始时就固定下来，之后的修改从下一个任务起生效。`}</li>
      </ul>
    </FormPage>
  );
}
