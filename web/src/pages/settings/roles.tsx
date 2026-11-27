// Settings › 数字员工, its second part: the digital employees by group, and the editor that
// creates or changes one (Mac: UI/Settings/RolesView.swift). Their lines, groups and stats are the
// service's (`setup_view.rs`); the editor binds the draft and sends it.

import { useEffect, useState } from "preact/hooks";
import { api, type Json } from "../../lib/api.ts";
import { t } from "../../lib/i18n.ts";
import { go, link } from "../../lib/router.ts";
import { connections, loadSetup, roleGroups, roles, show } from "../../lib/store.ts";
import { Avatar, Button, Card, initials, Menu, Note, Pill, Spinner } from "../../ui/kit.tsx";
import { Icon } from "../../ui/Icon.tsx";
import { confirmed, Field, TextInput, Select, FormPage, SwitchRow, TextArea } from "../../ui/form.tsx";
import { roleStats, testConnection } from "./data.ts";

const RECRUITER = "recruiter";
/** The colors a new digital employee takes in turn (Mac: `AppStore.roleColors`). */
const COLORS = ["#c96442", "#7b6cc2", "#3f7d5b", "#b5852b", "#2f7f8c", "#b04d6d", "#5f6b7a", "#8a6a4f"];

/** Talk with 招聘顾问: a new chat with it, unless it can't run, when its editor opens. */
async function chatWithRecruiter() {
  const r = roles.value.find((x) => x.id === RECRUITER);
  if (!r) return;
  if (r.unavailable) {
    show(r.unavailable);
    go(`/settings/agents/role/${RECRUITER}`);
    return;
  }
  try {
    const chat = await api.post("/api/chats", { role: RECRUITER });
    go(`/runs/${encodeURIComponent(chat.id)}`);
  } catch (e) {
    show((e as Error).message);
  }
}

export function RolesSection() {
  const [renaming, setRenaming] = useState<string | null>(null);
  const [name, setName] = useState("");
  const list = roles.value;
  const ok = connections.value.filter((c) => c.phase === "ok");
  const groupTitle = (g: Json) => g.title as string;
  const bind = async (g: Json, c: Json) => {
    try {
      const bound = await api.post("/api/role-groups/bind", { group: g.group, connection: c.id });
      await loadSetup();
      show(t`${groupTitle(g)} 的 ${(bound ?? []).length} 位数字员工改用 ${c.name}`);
    } catch (e) {
      show((e as Error).message);
    }
  };
  const rename = async (from: string) => {
    try {
      await api.post("/api/role-groups/rename", { from, to: name });
      await loadSetup();
    } catch (e) {
      show((e as Error).message);
    }
    setRenaming(null);
  };
  return (
    <section class="settings-section">
      <div class="section-row">
        <div class="grow">
          <div class="h2 title3">{t`数字员工`}</div>
          <div class="secondary">{t`给每位数字员工配 Agent、模型和指令；资源库全员可用。`}</div>
        </div>
        {list.some((r) => r.id === RECRUITER) && (
          <Button title={t`和招聘顾问聊聊你要做的事，它为你设计一组数字员工，聊完在对话里确认创建`} onClick={() => void chatWithRecruiter()}>
            {t`找招聘顾问…`}
          </Button>
        )}
        <a class="btn btn-primary" {...link("/settings/agents/role/new")}>{t`新建数字员工`}</a>
      </div>
      {ok.length === 0 && connections.value.length > 0 && <Note text={t`还没有通过检查的 Agent。可以先编辑数字员工，Agent 通过检查后才能运行。`} tone="amber" />}
      {list.length === 0 ? (
        <Card pad={20}>
          <div class="h2 title3">{t`还没有数字员工`}</div>
          <div class="secondary">{t`先建一个负责人，再加几位协作者，团队就能开工。`}</div>
          <a class="btn btn-primary" {...link("/settings/agents/role/new")}>{t`新建数字员工`}</a>
        </Card>
      ) : (
        roleGroups.value.map((g) => {
          const members: Json[] = (g.roles ?? []).map((id: string) => list.find((r) => r.id === id)).filter(Boolean);
          const items = [
            ...connections.value.map((c) => ({ label: `${t`整组改用 Agent`} · ${c.name}`, onSelect: () => void bind(g, c) })),
            ...(g.group
              ? [
                  {
                    label: t`重命名…`,
                    onSelect: () => {
                      setName(g.group);
                      setRenaming(g.group);
                    },
                  },
                ]
              : []),
          ];
          return (
            <div class="role-group" data-group={g.group}>
              <div class="section-row">
                <strong>{groupTitle(g)}</strong>
                <span class="caption">{members.length}</span>
                <span class="grow" />
                <Menu icon="ellipsis" label={t`${groupTitle(g)} 的整组操作`} items={items} />
              </div>
              {renaming === g.group && (
                <div class="wrap-row">
                  <TextInput label={t`分组名`} value={name} onInput={setName} onEnter={() => void rename(g.group)} />
                  <Button onClick={() => setRenaming(null)}>{t`取消`}</Button>
                  <Button kind="primary" onClick={() => void rename(g.group)}>{t`重命名`}</Button>
                </div>
              )}
              <div class="card rows">
                {members.map((r) => (
                  <RoleRow r={r} />
                ))}
              </div>
            </div>
          );
        })
      )}
    </section>
  );
}

function RoleRow({ r }: { r: Json }) {
  const c = connections.value.find((x) => x.id === r.connection && r.connection);
  return (
    <a class="row role-row" data-role={r.id} title={t`编辑 ${r.name}`} {...link(`/settings/agents/role/${encodeURIComponent(r.id)}`)}>
      <Avatar role={{ ...r, init: r.displayInit }} size={30} />
      <span class="grow min0">
        <span class="name-line">
          <strong>{r.name}</strong>
          {r.lead && <Pill text={t`负责人`} tone="violet" />}
        </span>
        <span class="caption block ellipsis">{r.line}</span>
      </span>
      {c ? (
        <span class="role-agent">
          <span class="ellipsis">{c.name}</span>
          <span class="caption block ellipsis">{r.modelLine}</span>
        </span>
      ) : (
        <span class="role-agent fg-amber">{t`未选择 Agent`}</span>
      )}
      <Icon name="chevron.right" size={12} class="secondary" />
    </a>
  );
}

type Picture = { kind: "unchanged" } | { kind: "set"; data: string; url: string } | { kind: "remove" };

/** Create or change a digital employee, top to bottom the way it is read (Mac: `RoleEditor`).
 * One being changed opens once it is read. */
export function RoleEditor({ id }: { id: string }) {
  if (id !== "new" && !roles.value.some((r) => r.id === id)) return <Spinner />;
  return <RoleForm id={id} />;
}

function RoleForm({ id }: { id: string }) {
  const editing = id !== "new";
  const original: Json | undefined = editing ? roles.value.find((r) => r.id === id) : undefined;
  const start = (): Json => {
    if (original) return { ...original };
    const want = new URLSearchParams(location.search).get("connection");
    const ok = connections.value.find((c) => c.phase === "ok");
    return { id: "", name: t`新数字员工`, desc: "", group: "", connection: want ?? ok?.id ?? "", model: "", effort: "", session: {}, lead: false, color: COLORS[roles.value.length % COLORS.length], init: "", prompt: "", resources: [] };
  };
  const [r, setR] = useState<Json>(start);
  const [first] = useState<Json>(start);
  const [picture, setPicture] = useState<Picture>({ kind: "unchanged" });
  const [colors, setColors] = useState(false);
  const [saving, setSaving] = useState(false);
  useEffect(() => () => (picture.kind === "set" ? URL.revokeObjectURL(picture.url) : undefined), [picture]);
  const set = (k: string, v: unknown) => setR((d: Json) => ({ ...d, [k]: v }));
  const c = connections.value.find((x) => x.id === r.connection && r.connection);
  const opt = c?.options ?? {};
  const controls: Json[] = opt.controls ?? [];
  const perms = controls.filter((x) => x.permission);
  const rest = controls.filter((x) => !x.permission);
  const value = (x: Json) => (r.session?.[x.id] === undefined || r.session?.[x.id] === null ? x.default : r.session[x.id]);
  /** Choosing the default drops the key, so the role follows the default if it changes. */
  const setCtrl = (x: Json, v: unknown) => {
    const s = { ...(r.session ?? {}) };
    if (JSON.stringify(v) === JSON.stringify(x.default)) delete s[x.id];
    else s[x.id] = v;
    set("session", s);
  };
  const dirty = JSON.stringify(r) !== JSON.stringify(first) || picture.kind !== "unchanged";
  const back = () => go("/settings/agents");
  const cancel = () => {
    if (!dirty || confirmed(t`放弃未保存的更改？`, t`关闭后，这次的修改不会保存。`)) back();
  };
  const choose = (file: File | undefined) => {
    if (!file) return;
    if (!file.type.startsWith("image/")) {
      show(t`这个文件不是图片`);
      return;
    }
    const reader = new FileReader();
    reader.onload = () => setPicture({ kind: "set", data: String(reader.result), url: URL.createObjectURL(file) });
    reader.readAsDataURL(file);
  };
  const save = async () => {
    setSaving(true);
    try {
      const saved = await api.post("/api/setup/roles", r);
      const path = `/api/roles/${encodeURIComponent(saved.id)}/avatar`;
      if (picture.kind === "set") await api.put(path, { data: picture.data });
      if (picture.kind === "remove") await api.delete(path);
      await loadSetup();
      show(t`已保存数字员工 ${saved.name}`);
      back();
    } catch (e) {
      show((e as Error).message);
    }
    setSaving(false);
  };
  const remove = async () => {
    if (!confirmed(t`删除数字员工「${r.name}」？`)) return;
    try {
      await api.delete(`/api/setup/roles/${encodeURIComponent(r.id)}`);
      await loadSetup();
      back();
    } catch (e) {
      show((e as Error).message);
    }
  };
  const seed = async (field: string, adopt: boolean) => {
    try {
      const saved = await api.post(`/api/roles/${encodeURIComponent(r.id)}/seed`, { field, adopt });
      const updates = { ...(r.seedUpdates ?? {}) };
      delete updates[field];
      setR((d: Json) => ({ ...d, [field]: saved[field], seed: saved.seed, seedUpdates: updates }));
      show(adopt ? t`已采用新版` : t`已保留你的版本`);
    } catch (e) {
      show((e as Error).message);
    }
  };
  const hasPicture = picture.kind === "set" || (picture.kind === "unchanged" && !!original?.avatar);
  const stat: string | null = roleStats.value?.roles?.[r.id]?.line ?? null;
  const tile = picture.kind === "set" ? (
    <img class="avatar" src={picture.url} alt="" width={34} height={34} style={{ borderRadius: "10px", objectFit: "cover" }} />
  ) : (
    <Avatar role={{ id: r.id, name: r.name, init: r.id ? (original?.displayInit ?? initials(r.init, r.name)) : initials(r.init, r.name), color: r.color, avatar: picture.kind === "remove" ? "" : original?.avatar }} size={34} />
  );
  const seedUpdate = (field: string, note: string) =>
    r.id && r.seedUpdates?.[field] ? (
      <div class="soft-box tone-bg-blue">
        <div>{note}</div>
        <details>
          <summary>{t`查看新版`}</summary>
          <div class="secondary pre">{r.seedUpdates[field]}</div>
        </details>
        <div class="wrap-row">
          <Button kind="primary" onClick={() => void seed(field, true)}>{t`采用新版`}</Button>
          <Button onClick={() => void seed(field, false)}>{t`保留我的`}</Button>
        </div>
      </div>
    ) : null;
  const choiceLabel = (o: Json) => o.name || o.value;
  const named = r.name.trim() !== "";

  return (
    <div class="page settings-editor" onDragOver={(e) => e.preventDefault()} onDrop={(e) => (e.preventDefault(), choose(e.dataTransfer?.files?.[0]))}>
      <FormPage
        title={r.id ? t`编辑数字员工` : t`新建数字员工`}
        icon={
          <button type="button" class="plain-btn" title={t`选颜色，或拖张图片当头像`} aria-label={t`数字员工头像`} onClick={() => setColors(!colors)}>
            {tile}
          </button>
        }
        footer={
          <>
            {r.id && (
              <Button kind="plain" onClick={() => void remove()}>
                <span class="fg-red">{t`删除…`}</span>
              </Button>
            )}
            <span class="grow" />
            <Button onClick={cancel}>{t`取消`}</Button>
            <Button kind="primary" disabled={saving || !named} title={named ? "" : t`先填写名称`} onClick={() => void save()}>
              {saving ? t`正在保存…` : r.id ? t`保存` : t`创建`}
            </Button>
          </>
        }
      >
        <div class="wrap-row">
          <label class="link file-pick" title={t`选张图片当头像，会裁成正方形；也可以直接拖进来`}>
            {t`更换头像…`}
            <input type="file" accept="image/*" hidden onChange={(e) => choose((e.target as HTMLInputElement).files?.[0])} />
          </label>
          {hasPicture && (
            <button type="button" class="link" onClick={() => setPicture({ kind: "remove" })}>
              {t`移除头像`}
            </button>
          )}
          {stat && (
            <span class="caption" title={t`首次通过：第一次尝试就完成，且验证一次通过`}>
              {stat}
            </span>
          )}
        </div>
        {colors && (
          <div class="soft-box">
            <div class="caption">{t`没有头像图片时，显示这个颜色和文字。`}</div>
            <div class="wrap-row">
              {COLORS.map((col) => (
                <button
                  type="button"
                  class={"swatch" + (r.color === col ? " on" : "")}
                  aria-label={col}
                  aria-pressed={r.color === col}
                  style={{ background: col }}
                  onClick={() => {
                    set("color", col);
                    setColors(false);
                  }}
                />
              ))}
            </div>
          </div>
        )}
        {!r.id && roles.value.some((x) => x.id === RECRUITER) && (
          <div class="soft-box wrap-row">
            <Avatar role={roles.value.find((x) => x.id === RECRUITER)} size={26} />
            <span class="grow">{t`不知道怎么写职责和指令？和招聘顾问聊聊你要做的事，它帮你设计好，你确认后创建。`}</span>
            <Button onClick={() => (!dirty || confirmed(t`放弃未保存的更改？`, t`关闭后，这次的修改不会保存。`)) && void chatWithRecruiter()}>{t`找招聘顾问`}</Button>
          </div>
        )}
        <Field label={t`名称`}>
          <TextInput label={t`名称`} placeholder="Reviewer" value={r.name} onInput={(v) => setR((d: Json) => ({ ...d, name: v, ...(d.id ? {} : { init: "" }) }))} />
        </Field>
        <Field label={t`职责`} note={t`写它擅长什么、交付什么，负责人据此派活。`}>
          <TextInput label={t`职责`} placeholder={t`独立检查变更，寻找风险与反例`} value={r.desc ?? ""} onInput={(v) => set("desc", v)} />
        </Field>
        {seedUpdate("desc", t`默认职责有新版本。你改过这一项，所以升级时没有替换。`)}
        <Field group label={t`分组`} note={t`按团队或行业归类，只用来管理，负责人看不到。`}>
          <div class="wrap-row nowrap">
            <TextInput label={t`分组`} placeholder={t`不分组`} value={r.group ?? ""} onInput={(v) => set("group", v)} />
            <Select
              label={t`选一个已有的分组`}
              value=""
              disabled={!roleGroups.value.some((g) => g.group)}
              options={[["", "…"], ...roleGroups.value.filter((g) => g.group).map((g): [string, string] => [g.group, g.group])]}
              onChange={(v) => v && set("group", v)}
            />
          </div>
        </Field>
        <SwitchRow on={!!r.lead} disabled={!c} onChange={(v) => set("lead", v)} title={t`可以担任负责人`} sub={c ? t`用 /goal 推进目标，需要时编排协作者` : t`绑定 Agent 后才能担任负责人`} />
        <SwitchRow on={!!r.hires} onChange={(v) => set("hires", v)} title={t`可以在对话里提议数字员工`} sub={t`在对话里为你设计数字员工，你看过、勾选后才创建`} />

        <div class="h2">{t`指令`}</div>
        <TextArea label={t`指令`} rows={6} value={r.prompt ?? ""} placeholder={t`告诉这个数字员工怎么做事，例如：先读相关代码和已有测试；重点检查边界条件、错误处理和测试覆盖。`} onInput={(v) => set("prompt", v)} />
        <div class="caption">{t`作为系统提示词交给 Agent，整个会话都有效。`}</div>
        {seedUpdate("prompt", t`默认指令有新版本。你改过这一项，所以升级时没有替换。`)}

        <div class="h2">{t`由谁来做`}</div>
        <div class="field-row">
          <Field label="Agent">
            <Select
              label="Agent"
              value={r.connection ?? ""}
              options={[["", t`选择 Agent`], ...connections.value.map((x): [string, string] => [x.id, x.name])]}
              onChange={(v) => setR((d: Json) => ({ ...d, connection: v, model: "", effort: "", session: {} }))}
            />
          </Field>
          <Field group label={t`模型`}>
            {!c ? (
              <Select label={t`模型`} value="" disabled options={[["", t`先选择 Agent`]]} onChange={() => {}} />
            ) : (opt.model?.options ?? []).length ? (
              <Select label={t`模型`} value={r.model ?? ""} options={[["", t`默认（${opt.model.current ?? ""}）`], ...opt.model.options.map((o: Json): [string, string] => [o.value, choiceLabel(o)])]} onChange={(v) => set("model", v)} />
            ) : c.status === "testing" ? (
              <span class="secondary">{t`读取模型中…`}</span>
            ) : (
              <button type="button" class="link" onClick={() => void testConnection(c.id)}>
                {c.status === "error" ? t`检查没通过，再查一次` : t`检查 Agent 以读取模型`}
              </button>
            )}
          </Field>
          <Field label={t`推理强度`}>
            {(opt.effort?.options ?? []).length ? (
              <Select label={t`推理强度`} value={r.effort ?? ""} options={[["", t`默认（${opt.effort.current ?? ""}）`], ...opt.effort.options.map((o: Json): [string, string] => [o.value, choiceLabel(o)])]} onChange={(v) => set("effort", v)} />
            ) : (
              <Select label={t`推理强度`} value="" disabled options={[["", t`默认`]]} onChange={() => {}} />
            )}
          </Field>
        </div>

        <div class="section-row">
          <span class="h2 grow">{t`权限`}</span>
          {perms.length > 0 && <span class="caption">{t`${c?.name ?? ""} 的权限模式`}</span>}
        </div>
        {!c ? (
          <Note text={t`先选 Agent，权限选项由它自己声明。`} />
        ) : c.status === "testing" ? (
          <Note text={t`正在检查 ${c.name}，读取它的权限模式…`} />
        ) : !opt.controls ? (
          <div class="wrap-row">
            <Note text={t`还没有读取 ${c.name} 的权限模式。`} tone="amber" />
            <Button onClick={() => void testConnection(c.id)}>{t`检查 Agent`}</Button>
          </div>
        ) : (
          <>
            {perms.length === 0 && <Note text={t`${c.name} 没有权限模式：工具直接执行，不经审批，能改工作目录里的文件。`} />}
            {perms.map((x) =>
              x.type === "boolean" ? (
                <SwitchRow on={!!value(x)} onChange={(v) => setCtrl(x, v)} title={x.name} sub={x.desc} />
              ) : (
                <div class="perm-group">
                  <div>
                    <strong>{x.name}</strong> {x.desc && <span class="caption">{x.desc}</span>}
                  </div>
                  {(x.options ?? []).map((o: Json) => {
                    const on = JSON.stringify(o.value) === JSON.stringify(value(x));
                    return (
                      <button type="button" class={"perm-option" + (on ? " on" : "")} aria-pressed={on} onClick={() => setCtrl(x, o.value)}>
                        <Icon name={o.icon ?? "checkmark.shield"} class={o.widens ? "fg-red" : "secondary"} />
                        <span class="grow">
                          <span>
                            <strong>{choiceLabel(o)}</strong> {JSON.stringify(o.value) === JSON.stringify(x.default) && <span class="caption">{t`默认`}</span>}
                          </span>
                          {o.desc && <span class="caption block">{o.desc}</span>}
                        </span>
                        {on && <Icon name="checkmark" class="fg-blue" />}
                      </button>
                    );
                  })}
                </div>
              ),
            )}
            <div class="caption">{t`要不要先问你，由 Agent 自己判断；它发来的审批、表单和链接都进「待处理」。`}</div>
          </>
        )}
        {rest.length > 0 && (
          <>
            <div class="section-row">
              <span class="h2 grow">{t`会话选项`}</span>
              <span class="caption">{t`${c?.name ?? ""} 提供`}</span>
            </div>
            {rest.map((x) =>
              x.type === "boolean" ? (
                <SwitchRow on={!!value(x)} onChange={(v) => setCtrl(x, v)} title={x.name} sub={x.desc} />
              ) : (
                <Field label={x.name} note={x.desc}>
                  <Select
                    label={x.name}
                    value={String(value(x) ?? "")}
                    options={(x.options ?? []).map((o: Json): [string, string] => [String(o.value), choiceLabel(o) + (JSON.stringify(o.value) === JSON.stringify(x.default) ? t`（默认）` : "")])}
                    onChange={(v) => setCtrl(x, v)}
                  />
                </Field>
              ),
            )}
          </>
        )}
      </FormPage>
    </div>
  );
}
