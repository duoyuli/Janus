// Settings › 数字员工, its first part: the Agents the digital employees run on, each card with its
// login, check and runtime line, and the editor that adds or changes one (Mac:
// UI/Settings/WorkersSettingsView.swift). Every line on a card is the service's
// (`setup_view.rs`): the page lays them out.

import { useEffect, useState } from "preact/hooks";
import { api, type Json } from "../../lib/api.ts";
import { t, tr } from "../../lib/i18n.ts";
import { go, link } from "../../lib/router.ts";
import { connections, loadSetup, meta, roles, show } from "../../lib/store.ts";
import type { Tone } from "../../lib/status.ts";
import { Avatar, Button, Card, Menu, Note, Pill, Spinner } from "../../ui/kit.tsx";
import { Icon } from "../../ui/Icon.tsx";
import { confirmed, Field, TextInput, Segmented, Select, FormPage, TextArea } from "../../ui/form.tsx";
import { acknowledge, adapterAction, adapters, autoUpdateSeen, login, testConnection } from "./data.ts";

/** A catalog word of the agent kinds (`/api/meta` `kinds`), in the interface language. */
const catalog = (s: string) => (s ? tr(s) : s);

export function kindOf(id: string): Json | undefined {
  return (meta.value?.kinds ?? []).find((k: Json) => k.id === id);
}

/** The phase a card's status says, as the Mac app's `ConnectionRow.status`. */
function phase(c: Json): [string, Tone] {
  switch (c.phase) {
    case "logging_in":
      return [t`等待登录`, "amber"];
    case "logged_out":
      return [t`未登录`, "amber"];
    case "testing":
      return [t`检查中…`, "blue"];
    case "ok":
      return [t`可用`, "green"];
    case "error":
      return [t`失败`, "red"];
    default:
      return [t`未检查`, "gray"];
  }
}

const startedLogin = () => t`已开始登录：按卡片上的提示操作，完成后自动检查。`;

export function AgentsSection() {
  const list = connections.value;
  const add = (kind: string) => go(`/settings/agents/agent/new?kind=${kind}`);
  return (
    <section class="settings-section">
      <div class="section-row">
        <div class="grow">
          <div class="h2 title3">Agent</div>
          <div class="secondary">{t`每个 Agent 在独立的进程和配置目录里运行，不影响你本机的 Agent。`}</div>
        </div>
        <Button onClick={() => add(list.length === 0 ? "codex" : "claude")}>{t`添加 Agent`}</Button>
      </div>
      {list.length === 0 ? (
        <Card pad={20}>
          <div class="h2 title3">{t`还没有接入 Agent`}</div>
          <div class="secondary">{t`把本机在用的 Agent 搬过来，配置和凭据一起带上。`}</div>
          <div class="wrap-row">
            {["codex", "claude"].map((id) => {
              const k = kindOf(id);
              if (!k) return null;
              return (
                <button type="button" class="kind-tile" title={t`复制本机 ${k.name} 的配置和凭据`} onClick={() => go(`/settings/agents/agent/new?kind=${id}&auth=import`)}>
                  <Avatar role={{ init: k.init, color: k.color }} size={28} />
                  <span>
                    <strong>{k.name}</strong>
                    <span class="caption block">{t`复制本机配置`}</span>
                  </span>
                </button>
              );
            })}
          </div>
          <a class="link" {...link("/settings/agents/agent/new?kind=codex")}>{t`用 API Key 或其他方式添加`}</a>
        </Card>
      ) : (
        list.map((c) => {
          const a = adapters.value.find((x) => x.id === c.base);
          return (
            <div class="card connection-card" data-connection={c.id}>
              <ConnectionRow c={c} />
              {a && <AdapterLine a={a} />}
            </div>
          );
        })
      )}
    </section>
  );
}

function ConnectionRow({ c }: { c: Json }) {
  const k = kindOf(c.kind);
  const [label, tone] = phase(c);
  const using = roles.value.filter((r) => r.connection === c.id);
  const loginLabel: string | null = c.loginLabel ?? null;
  const edit = `/settings/agents/agent/${encodeURIComponent(c.id)}`;
  const newRole = `/settings/agents/role/new?connection=${encodeURIComponent(c.id)}`;
  const remove = async () => {
    const used = using.map((r) => r.name);
    const msg = used.length ? t`这些数字员工正在使用它，删除后会变成未绑定，需要重新选择 Agent：${used.join(t`、`)}` : "";
    if (!confirmed(t`删除「${c.name}」和它的独立目录（包括登录凭据）？`, msg)) return;
    try {
      await api.delete(`/api/setup/connections/${encodeURIComponent(c.id)}`);
      show(t`已删除 ${c.name}`);
    } catch (e) {
      show((e as Error).message);
    }
    await loadSetup();
  };
  const items = [
    ...(loginLabel ? [{ label: `${c.loggedIn ? t`重新登录` : t`登录`} ${loginLabel}`, onSelect: () => void login(c.id, startedLogin()) }] : []),
    { label: t`检查配置`, onSelect: () => void testConnection(c.id) },
    { label: t`编辑…`, onSelect: () => go(edit) },
    { label: t`在这个 Agent 上新建数字员工…`, onSelect: () => go(newRole) },
    { label: t`删除…`, onSelect: () => void remove() },
  ];
  return (
    <div class="connection-row">
      <div class="row-line">
        <a class="row-main" title={t`编辑 ${c.name}`} {...link(edit)}>
          <Avatar role={{ init: k?.init ?? "?", color: k?.color ?? "#64748b" }} size={30} />
          <span class="grow min0">
            <span class="name-line">
              <strong>{c.name}</strong>
              <span class="mono caption">{c.id}</span>
            </span>
            <span class="caption block ellipsis">{c.summary}</span>
          </span>
        </a>
        <span class="row-side">
          {using.length === 0 ? (
            <a class="link" {...link(newRole)}>{t`新建数字员工`}</a>
          ) : (
            <span class="avatars" title={t`${using.length} 个数字员工`}>
              {using.slice(0, 4).map((r) => (
                <Avatar role={r} size={20} />
              ))}
              <span class="caption">{t`${using.length} 个数字员工`}</span>
            </span>
          )}
          <Pill text={label} tone={tone} />
          <Menu icon="ellipsis" label={t`更多操作`} items={items} />
        </span>
      </div>
      <ConnectionExtra c={c} loginLabel={loginLabel} />
    </div>
  );
}

function ConnectionExtra({ c, loginLabel }: { c: Json; loginLabel: string | null }) {
  if (loginLabel && c.login?.state === "running") {
    return (
      <div class="extra">
        <div>{t`按提示完成 ${loginLabel} 登录，完成后自动检查。`}</div>
        {c.loginLog && <pre class="code">{c.loginLog}</pre>}
        {(c.loginUrls ?? []).map((u: string) => (
          <a class="link" href={u} target="_blank" rel="noreferrer">
            {u}
          </a>
        ))}
      </div>
    );
  }
  if (loginLabel && !c.loggedIn) {
    return (
      <div class="extra wrap-row">
        <Note text={t`还没有登录 ${loginLabel}。`} tone="amber" />
        <Button onClick={() => void login(c.id, startedLogin())}>{t`登录`}</Button>
      </div>
    );
  }
  if (c.status === "testing") return <CheckList c={c} />;
  if (c.status === "error") {
    return (
      <div class="extra wrap-row">
        <Note text={c.error ?? ""} tone="red" />
        <Button onClick={() => void testConnection(c.id)}>{t`重新检查`}</Button>
        <a class="btn btn-bordered" {...link(`/settings/agents/agent/${encodeURIComponent(c.id)}`)}>{t`修改配置`}</a>
      </div>
    );
  }
  if (c.status === "untested") {
    return (
      <div class="extra wrap-row">
        <span class="secondary">{t`还没有检查配置，读到模型后才能给数字员工使用。`}</span>
        <button type="button" class="link" onClick={() => void testConnection(c.id)}>{t`检查`}</button>
      </div>
    );
  }
  return null;
}

/** The steps of an agent's check as the service lists them (`checks`). */
export function CheckList({ c }: { c: Json }) {
  const steps: Json[] = c.checks ?? [];
  if (!steps.length) return null;
  const icon = (s: string) =>
    s === "ok" ? <Icon name="checkmark.circle" class="fg-green" /> : s === "failed" ? <Icon name="xmark.circle" class="fg-red" /> : s === "running" ? <Spinner /> : <Icon name="circle" class="secondary" />;
  return (
    <div class="checks extra">
      {steps.map((s) => (
        <div class="check-step" data-state={s.state}>
          {icon(s.state)}
          <span class="step-name">{s.step}</span>
          <span class="caption">{s.detail}</span>
        </div>
      ))}
    </div>
  );
}

/** A card's runtime line, as the service words it (`line`); an automatic update the person saw
 * once is not announced again. */
function AdapterLine({ a }: { a: Json }) {
  const line = a.line ?? {};
  const [why, setWhy] = useState(false);
  useEffect(() => () => acknowledge(a), [a.line?.autoUpdated?.key]);
  const seen = line.state === "autoUpdated" && autoUpdateSeen.value.has(line.autoUpdated?.key);
  const word: string | null = seen || line.state === "idle" ? null : (line.word ?? null);
  const menu: Json[] = line.menu ?? [];
  return (
    <div class="adapter-line" title={line.hover}>
      <Icon name="shippingbox" size={13} class="secondary" />
      {a.current ? (
        <span class="caption">
          {t`运行组件`} <span class="mono">{a.current}</span> {word} {a.pinned && t`不会自动更新`}
        </span>
      ) : (
        word && <span class="caption">{word}</span>
      )}
      {line.failure && (
        <>
          <span class="caption fg-red">{line.failure}</span>
          <button type="button" class="link caption" onClick={() => setWhy(!why)}>
            {t`查看原因`}
          </button>
        </>
      )}
      <span class="grow" />
      {menu.length > 0 ? (
        <select
          class="version-menu"
          aria-label={t`切回历史版本`}
          disabled={line.running}
          value=""
          onChange={(e) => {
            const m = menu[Number((e.target as HTMLSelectElement).value)];
            if (m) void adapterAction(a.id, m.action, m.version);
          }}
        >
          <option value="" disabled>
            {t`切回历史版本`}
          </option>
          {menu.map((m, i) => (
            <option value={String(i)}>{m.label}</option>
          ))}
        </select>
      ) : (
        line.install && (
          <button type="button" class="link caption" disabled={line.running} onClick={() => void adapterAction(a.id, "install")}>
            {t`安装`}
          </button>
        )
      )}
      {why && (
        <div class="adapter-why">
          <div>{line.detail}</div>
          {(line.notes ?? []).length > 0 && <pre class="code">{line.notes.join("\n")}</pre>}
          <div class="wrap-row">
            <Button small disabled={line.running} onClick={() => (setWhy(false), void adapterAction(a.id, "install"))}>
              {line.state === "installFailed" ? t`安装` : t`重试更新`}
            </Button>
            <a class="link" href={`/api/adapters/${encodeURIComponent(a.id)}/log`} target="_blank" rel="noreferrer">
              {t`查看日志`}
            </a>
          </div>
        </div>
      )}
    </div>
  );
}

// --- The editor ----------------------------------------------------------------------------------

/** The draft a new agent starts from (Mac: `AgentsSection.newDraft`). */
function newDraft(kind: string, auth?: string | null): Json {
  const k = kindOf(kind);
  const a = auth ?? k?.auths?.[0]?.id ?? "";
  return { kind, auth: a, name: k?.name ?? "", api: "openai-completions", prompt: "", envText: "" };
}

/** Add an Agent or change one: its kind, how it signs in, its credentials and environment, its
 * runtime and its own prompt (Mac: `ConnectionEditor`). */
export function ConnectionEditor({ id }: { id: string }) {
  const editing = id !== "new";
  const params = new URLSearchParams(location.search);
  const prev: Json | undefined = editing ? connections.value.find((c) => c.id === id) : undefined;
  const start = (): Json => (prev ? { ...prev } : newDraft(params.get("kind") ?? "codex", params.get("auth")));
  const [draft, setDraft] = useState<Json>(start);
  const [saved] = useState<Json>(start);
  const [secrets, setSecrets] = useState<Record<string, string>>({});
  const [envDrafts, setEnvDrafts] = useState<Record<string, string>>({});
  const [idTouched, setIdTouched] = useState(false);
  const [preview, setPreview] = useState<Json>(null);
  const [showSource, setShowSource] = useState(false);
  const [advanced, setAdvanced] = useState(false);
  const [saving, setSaving] = useState(false);
  const [unread, setUnread] = useState(false);
  const [query, setQuery] = useState("");
  const set = (k: string, v: unknown) => setDraft((d: Json) => ({ ...d, [k]: v }));
  const k = kindOf(draft.kind) ?? {};
  const auths: Json[] = k.auths ?? [];
  const auth = auths.find((a) => a.id === draft.auth) ?? auths[0];
  const envAuth = (kind: string, a: string) => !!kindOf(kind)?.auths?.find((x: Json) => x.id === a)?.env;

  // The private values of its environment, read once: Janus is local first.
  useEffect(() => {
    if (!editing || !Object.values(prev?.env ?? {}).includes(null)) return;
    api.get(`/api/connections/${encodeURIComponent(id)}/env`).then(
      (r: Json) => setDraft((d: Json) => (d.envText === saved.envText ? { ...d, envText: r.envText } : d)),
      () => setUnread(true),
    );
  }, []);
  // What copying this Mac's config would bring.
  useEffect(() => {
    if (draft.auth !== "import") return;
    setPreview(null);
    const timer = setTimeout(() => {
      api.get(`/api/local-config?kind=${encodeURIComponent(draft.kind)}&source=${encodeURIComponent(draft.source ?? "")}`).then(setPreview, (e: Error) => setPreview({ error: e.message }));
    }, 250);
    return () => clearTimeout(timer);
  }, [draft.kind, draft.source, draft.auth]);
  // The folder's name, suggested from the kind and name until the person types their own.
  useEffect(() => {
    if (editing || idTouched) return;
    const timer = setTimeout(() => {
      api.get(`/api/agent-id?kind=${encodeURIComponent(draft.kind)}&name=${encodeURIComponent(draft.name ?? "")}`).then((r: Json) => setDraft((d: Json) => ({ ...d, newId: r.id })), () => {});
    }, 200);
    return () => clearTimeout(timer);
  }, [draft.kind, draft.name]);

  const dirty = JSON.stringify({ ...draft, newId: undefined }) !== JSON.stringify({ ...saved, newId: undefined }) || (idTouched && !draft.id) || Object.values(secrets).some(Boolean);
  const back = () => go("/settings/agents");
  const cancel = () => {
    if (!dirty || confirmed(t`放弃未保存的更改？`, t`关闭后，这次的修改不会保存。`)) back();
  };
  /** Pasted credentials stay with the provider and auth form they were typed for. */
  const envFor = (kind: string, a: string): string => {
    const current = envAuth(draft.kind, draft.auth) ? "env" : draft.kind;
    const next = envAuth(kind, a) ? "env" : kind;
    if (current === next) return draft.envText ?? "";
    setEnvDrafts((m) => ({ ...m, [current]: draft.envText ?? "" }));
    return envDrafts[next] ?? "";
  };
  const pickType = (v: string) => {
    if (v === draft.kind) return;
    const nk = kindOf(v);
    const keepName = draft.name && draft.name !== kindOf(draft.kind)?.name;
    const a = nk?.auths?.[0]?.id ?? "";
    setDraft({ kind: v, auth: a, name: keepName ? draft.name : (nk?.name ?? ""), api: "openai-completions", prompt: draft.prompt, envText: envFor(v, a), ...(idTouched ? { newId: draft.newId } : {}) });
    setPreview(null);
    setShowSource(false);
  };
  const insertTemplate = async () => {
    try {
      const r = await api.post("/api/agent-env/template", { text: draft.envText ?? "", kind: draft.kind, auth: draft.auth });
      set("envText", r.text);
    } catch (e) {
      show((e as Error).message);
    }
  };
  const save = async () => {
    setSaving(true);
    const body = { ...draft };
    for (const [key, v] of Object.entries(secrets)) if (v) body[key] = v;
    try {
      const c = await api.post("/api/setup/connections", body);
      show(t`已保存 ${c.name}，正在检查配置`);
      await loadSetup();
      const after = connections.value.find((x) => x.id === c.id);
      if (after?.loginLabel && !after.loggedIn) await login(c.id, startedLogin());
      else await testConnection(c.id);
      back();
    } catch (e) {
      show((e as Error).message);
    }
    setSaving(false);
  };

  const env = (template: string) => {
    const primary = envAuth(draft.kind, draft.auth);
    return (
      <Field
        label={t`环境变量`}
        note={unread ? t`只写变量名的行保留原值；删掉整行就移除。` : t`每行一个 KEY=value，可以直接贴 export 行；写 KEY= 表示清空。`}
        accessory={
          template && (
            <button type="button" class="link" title={t`把模板中还没有填写的变量加到末尾，已填写的不变`} onClick={() => void insertTemplate()}>
              {t`填入模板`}
            </button>
          )
        }
      >
        <TextArea
          mono
          rows={primary ? 8 : 4}
          label={t`环境变量`}
          value={draft.envText ?? ""}
          placeholder={primary ? "export ANTHROPIC_BASE_URL=https://api.example.com\nexport ANTHROPIC_AUTH_TOKEN=…\nexport ANTHROPIC_MODEL=…" : ""}
          onInput={(v) => set("envText", v)}
        />
      </Field>
    );
  };
  const input = (f: Json) => {
    const key: string = f.key;
    const name = catalog(f.label);
    const label = name + (f.required ? "" : t`（可选）`);
    if (f.type === "select") return <Field label={label}><Select label={name} value={draft[key] ?? ""} options={(f.options ?? []).map((o: Json) => [o[0], o[1]])} onChange={(v) => set(key, v)} /></Field>;
    if (f.type === "secret") {
      const kept = (prev?.secretsSet ?? []).includes(key);
      return (
        <Field label={label} note={catalog(f.help ?? "")}>
          <TextInput secret label={name} value={secrets[key] ?? ""} placeholder={kept ? t`已保存，留空表示不修改` : (f.placeholder ?? "")} onInput={(v) => setSecrets((s) => ({ ...s, [key]: v }))} />
        </Field>
      );
    }
    return <Field label={label} note={catalog(f.help ?? "")}><TextInput label={name} value={draft[key] ?? ""} placeholder={f.placeholder ?? ""} onInput={(v) => set(key, v)} /></Field>;
  };
  const kinds: Json[] = (meta.value?.kinds ?? []).filter((x: Json) => {
    const q = query.toLowerCase();
    return !q || `${catalog(x.name)} ${x.id} ${catalog(x.desc)}`.toLowerCase().includes(q);
  });
  const idValue = editing ? draft.id : (draft.newId ?? "");

  return (
    <div class="page settings-editor">
      <FormPage
        title={editing ? t`编辑 ${prev?.name ?? k.name}` : t`添加 Agent`}
        sub={catalog(k.desc ?? "")}
        icon={<Avatar role={{ init: k.init, color: k.color }} size={34} />}
        footer={
          <>
            <span class="grow" />
            <Button onClick={cancel}>{t`取消`}</Button>
            <Button kind="primary" disabled={saving} title={t`保存后自动检查：运行时、凭据、启动、会话、一次真实回复，进度看卡片。`} onClick={() => void save()}>
              {saving ? t`正在保存…` : editing ? t`保存` : t`创建`}
            </Button>
          </>
        }
      >
        <div class="kind-rail">
          <TextInput label={t`搜索 Agent 类型`} placeholder={t`搜索 Agent 类型`} value={query} onInput={setQuery} />
          <div class="wrap-row">
            {kinds.map((x) => (
              <button type="button" class={"kind-chip" + (x.id === draft.kind ? " on" : "")} aria-pressed={x.id === draft.kind} disabled={editing} onClick={() => pickType(x.id)}>
                <Avatar role={{ init: x.init, color: x.color }} size={20} />
                {catalog(x.name)}
              </button>
            ))}
          </div>
        </div>
        <div class="field-row">
          <Field label={t`名称`}>
            <TextInput label={t`名称`} value={draft.name ?? ""} placeholder={catalog(k.name ?? "")} onInput={(v) => set("name", v)} />
          </Field>
          <Field label="ID" note={`${meta.value?.agentsDir ?? ""}/${idValue}`}>
            <TextInput
              mono
              label="ID"
              value={idValue}
              placeholder="codex"
              disabled={editing}
              onInput={(v) => {
                set("newId", v);
                setIdTouched(v !== "");
              }}
            />
          </Field>
        </div>
        {auth && (
          <>
            <Field group label={t`认证`} note={catalog(auth.desc ?? "")}>
              <Segmented
                label={t`认证`}
                value={auth.id}
                options={auths.map((a) => [a.id, catalog(a.label)])}
                onChange={(next) => setDraft((d: Json) => ({ ...d, envText: envFor(d.kind, next), auth: next }))}
              />
            </Field>
            {auth.id === "import" ? (
              <>
                {showSource && (auth.fields ?? []).map(input)}
                <ImportPreview preview={preview} kind={k} showSource={showSource} onSource={() => setShowSource(true)} />
              </>
            ) : auth.env ? (
              env(auth.template ?? "")
            ) : (
              (auth.fields ?? []).map(input)
            )}
            {auth.login && prev && (
              <div class="wrap-row">
                <span class="secondary">{prev.loggedIn ? t`已登录 ${auth.label}。` : t`还没有登录。`}</span>
                <button type="button" class="link" onClick={() => void login(prev.id, startedLogin())}>
                  {prev.loggedIn ? t`重新登录` : t`现在登录`}
                </button>
              </div>
            )}
          </>
        )}
        {prev && (prev.checks ?? []).length > 0 && (
          <div class="soft-box">
            <div class="section-row">
              <strong class="grow">{t`配置检查`}</strong>
              {prev.status !== "testing" ? (
                <button type="button" class="link" onClick={() => void testConnection(prev.id)}>
                  {t`重新检查`}
                </button>
              ) : (
                <span class="caption">{t`检查中…`}</span>
              )}
            </div>
            <CheckList c={prev} />
          </div>
        )}
        <details class="advanced" open={advanced || !!draft.prompt || !!draft.bin} onToggle={(e) => setAdvanced((e.target as HTMLDetailsElement).open)}>
          <summary>{t`高级设置`}</summary>
          <div class="sheet-body inner">
            {k.runtime && (
              <Field label={t`运行时路径`}>
                <TextInput mono label={t`运行时路径`} value={draft.bin ?? ""} placeholder={k.runtime.detected || k.runtime.placeholder} onInput={(v) => set("bin", v)} />
              </Field>
            )}
            <Field label={t`自定义提示词`} note={t`放在数字员工指令前面一起注入（Claude Code 走系统提示，Codex 走开发者指令）。`}>
              <TextArea label={t`自定义提示词`} rows={3} value={draft.prompt ?? ""} onInput={(v) => set("prompt", v)} />
            </Field>
            {!envAuth(draft.kind, draft.auth) && env("")}
          </div>
        </details>
      </FormPage>
    </div>
  );
}

function ImportPreview({ preview, kind, showSource, onSource }: { preview: Json; kind: Json; showSource: boolean; onSource: () => void }) {
  if (!preview) return <div class="secondary">{t`正在检查本机配置…`}</div>;
  if (preview.error) return <Note text={preview.error} tone="red" />;
  if (!(preview.files ?? []).length && !(preview.env ?? []).length) {
    return (
      <div class="wrap-row">
        <Note text={t`在 ${preview.source} 没有找到 ${kind.name} 的配置。`} tone="amber" />
        {!showSource && <Button onClick={onSource}>{t`更改来源`}</Button>}
      </div>
    );
  }
  return (
    <div class="soft-box">
      <div class="wrap-row">
        <strong>{t`从 ${preview.source} 复制`}</strong>
        {!showSource && (
          <button type="button" class="link" onClick={onSource}>
            {t`更改`}
          </button>
        )}
      </div>
      {(preview.files ?? []).map((f: Json) => (
        <div class="caption">
          <span class="mono">{f.file}</span> {f.note}
        </div>
      ))}
      {(preview.env ?? []).map((e: Json) => (
        <div class="caption">
          <span class="mono">${e.name}</span> {t`shell 环境变量`}
        </div>
      ))}
    </div>
  );
}

