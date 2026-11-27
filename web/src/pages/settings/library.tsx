// Settings › 资源库: a tab each for skills and MCP servers, everything in it for every digital
// employee through the tab's router; skills from GitHub links, folders on the Mac or the editor,
// in folding groups by where they came from; MCP servers written as a form or pasted as JSON; the
// community's picks last (Mac: UI/Settings/LibraryView.swift, McpEditor.swift, CatalogShelf.swift,
// RouterGuide.swift). What each row says is the service's (`setup_view/library.rs`), and so are
// the MCP editor's command line and JSON (`mcp_draft.rs`) and the picks (`picks.rs`).

import { useEffect, useRef, useState } from "preact/hooks";
import { signal } from "@preact/signals";
import { api, type Json } from "../../lib/api.ts";
import { serverLang, t } from "../../lib/i18n.ts";
import { ago } from "../../lib/format.ts";
import { go, link } from "../../lib/router.ts";
import { show } from "../../lib/store.ts";
import { load, save } from "../../lib/prefs.ts";
import { useDrawn } from "../../lib/testing.ts";
import { Button, Card, Note, Pill, Spinner } from "../../ui/kit.tsx";
import { Icon } from "../../ui/Icon.tsx";
import { Check, confirmed, Field, TextInput, Segmented, FormPage, SwitchRow, TextArea } from "../../ui/form.tsx";
import { library, loadLibrary, picks, useFollow } from "./data.ts";

export type LibraryKind = "skills" | "servers";

/** The editor a pick that needs a key opens on, handed from the shelf to the editor page. */
const pendingEditor = signal<Json>(null);

const items = (kind: LibraryKind): Json[] => (library.value?.items ?? []).filter((i: Json) => i.kind === (kind === "skills" ? "skill" : "mcp"));

export function LibraryPane({ kind }: { kind: LibraryKind }) {
  const [query, setQuery] = useState("");
  useFollow(() => void loadLibrary(), ["library"]);
  const lib = library.value;
  useDrawn("library", () => ({
    kind,
    items: items(kind).map((i) => ({ id: i.id, name: i.name, warning: i.warning, origin: i.originText })),
    groups: (lib?.skillGroups ?? []).map((g: Json) => ({ type: g.type, items: g.items })),
    picks: (picks.value?.[kind] ?? []).map((p: Json) => ({ id: p.id, have: p.have })),
  }));
  return (
    <div class="settings-pane library-pane">
      <div class="section-row">
        <Segmented
          label={t`资源类型`}
          value={kind}
          options={[
            ["skills", t`Skill ${items("skills").length}`],
            ["servers", t`MCP ${items("servers").length}`],
          ]}
          onChange={(v) => go(`/settings/library/${v}`, true)}
        />
        <span class="grow" />
        <div class="search">
          <TextInput label={kind === "skills" ? t`搜索 Skill` : t`搜索 MCP 服务器`} placeholder={kind === "skills" ? t`搜索 Skill` : t`搜索 MCP 服务器`} value={query} onInput={setQuery} />
        </div>
      </div>
      <div class="caption">{t`资源库全员可用，按需取用。改动下个任务起生效。`}</div>
      {!lib ? <Spinner /> : kind === "skills" ? <SkillTab query={query} /> : <McpTab query={query} />}
      {lib && <Shelf kind={kind} query={query} />}
    </div>
  );
}

const matches = (q: string) => (i: Json) => !q || `${i.name} ${i.description ?? ""}`.toLowerCase().includes(q.toLowerCase());

// --- How a router works ----------------------------------------------------------------------------

function RouterGuide({ kind, names, now, searched }: { kind: LibraryKind; names: string[]; now: string; searched?: boolean }) {
  const [open, setOpen] = useState(load("routerGuide", true));
  const steps: [string, string][] =
    kind === "skills"
      ? [
          [t`收进路由`, t`资源库的 Skill 统一收进 janus-skills，Agent 的 Skill 列表里只多这一项。`],
          [t`只占一条描述`, t`janus-skills 的描述提醒 Agent 先读它，并列出常用 Skill。`],
          [t`先查目录`, searched ? t`Skill 多时目录只列名字，Agent 按关键词搜。` : t`Agent 读 janus-skills 的目录：每个 Skill 一行，写着完整描述和什么时候用。`],
          [t`用到再读`, t`找到合适的 Skill 再读 SKILL.md 照做；每次读取都会计数。`],
        ]
      : [
          [t`只多一个入口`, t`会话只拿到 janus-tools：它的说明列出各服务器和工具，Agent 用 find_tools 查、call_tool 调用，run_code 一步串起多次调用。`],
          [t`用到才启动`, t`第一次调用时才启动：命令型在隔离环境里跑，HTTP / SSE 由 Janus 连接。`],
          [t`同一个任务共用`, t`负责人和子任务共用同一个服务器，一个任务只运行一份。`],
          [t`闲置就停`, t`闲置 10 分钟自动停，下次用到再启动；任务结束时全停。`],
        ];
  return (
    <Card>
      <div class="section-row">
        <Icon name="sparkles" class="fg-blue" />
        <strong class="grow">{kind === "skills" ? t`Skill 路由怎样工作` : t`MCP 路由怎样工作`}</strong>
        <button
          type="button"
          class="link"
          onClick={() => {
            setOpen(!open);
            save("routerGuide", !open);
          }}
        >
          {open ? t`收起说明` : t`展开说明`}
        </button>
      </div>
      {open && (
        <ol class="guide-steps">
          {steps.map(([title, detail]) => (
            <li>
              <strong>{title}</strong>
              <span class="caption block">{detail}</span>
            </li>
          ))}
        </ol>
      )}
      {open && names.length > 0 && (
        <div class="wrap-row chips">
          {names.slice(0, 12).map((n) => (
            <span class="chip mono">{n}</span>
          ))}
          {names.length > 12 && <span class="caption">{t`还有 ${names.length - 12} 个`}</span>}
        </div>
      )}
      <div class="secondary">{now}</div>
    </Card>
  );
}

// --- Skills ---------------------------------------------------------------------------------------

/** Groups up to this size start open; bigger ones start folded. */
const OPEN_BY_DEFAULT = 6;

function SkillTab({ query }: { query: string }) {
  const lib = library.value;
  const skills = items("skills");
  const searched = lib?.skillRouter?.catalog === "search";
  const order: string[] = lib?.skillRouter?.order ?? [];
  const names = order.map((id) => skills.find((s) => s.id === id)?.name).filter(Boolean) as string[];
  const sources: Json[] = lib?.sources ?? [];
  const updates = sources.filter((s) => s.updateAvailable);
  const [opened, setOpened] = useState<Record<string, boolean>>({});
  const [link_, setLink] = useState("");
  const [adding, setAdding] = useState(false);
  const [checking, setChecking] = useState(false);
  const [folderPath, setFolderPath] = useState("");
  const [folder, setFolder] = useState<Json>(null);
  const [folderPicks, setFolderPicks] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [choosing, setChoosing] = useState<Set<string>>(new Set());
  const [picks_, setPicks] = useState<Record<string, Set<string>>>({});
  const [updating, setUpdating] = useState<Set<string>>(new Set());
  const groups: Json[] = (lib?.skillGroups ?? []).map((g: Json) => ({ ...g, list: (g.items ?? []).map((id: string) => skills.find((s) => s.id === id)).filter(Boolean) }));
  const shown = groups.filter((g) => !query || g.list.some(matches(query)));
  const key = (g: Json) => (g.type === "source" ? `source:${g.source}` : g.type === "folder" ? `folder:${g.path}` : "created");
  const isOpen = (g: Json) => (query ? true : (opened[key(g)] ?? g.list.length <= OPEN_BY_DEFAULT));

  const addSource = async () => {
    const url = link_.trim();
    if (!url || adding) return;
    setAdding(true);
    try {
      const s = await api.post("/api/library/sources", { url });
      setLink("");
      setPicks((p) => ({ ...p, [s.id]: new Set((s.offers ?? []).map((o: Json) => o.path)) }));
      setOpened((o) => ({ ...o, [`source:${s.id}`]: true }));
      setChoosing((c) => new Set(c).add(s.id));
      await loadLibrary();
      show(t`找到 ${(s.offers ?? []).length} 个 Skill，选择要导入的`);
    } catch (e) {
      show((e as Error).message);
    }
    setAdding(false);
  };
  const checkSources = async () => {
    setChecking(true);
    try {
      await api.post("/api/library/sources/check");
      await loadLibrary();
      const n = (library.value?.sources ?? []).filter((s: Json) => s.updateAvailable).length;
      show(n === 0 ? t`所有来源都是最新的` : t`${n} 个来源有更新`);
    } catch (e) {
      show((e as Error).message);
    }
    setChecking(false);
  };
  const updateSources = async (ids: string[]) => {
    setUpdating(new Set(ids));
    for (const id of ids) {
      try {
        await api.post(`/api/library/sources/${encodeURIComponent(id)}/update`);
      } catch (e) {
        show((e as Error).message);
      }
    }
    setUpdating(new Set());
    await loadLibrary();
  };
  const scan = async () => {
    if (!folderPath.trim() || busy) return;
    setBusy(true);
    try {
      const p = await api.post("/api/library/skills/scan", { path: folderPath.trim() });
      const imported = new Set((p.imported ?? []).map((i: Json) => i.path));
      setFolderPicks(new Set((p.offers ?? []).map((o: Json) => o.path).filter((x: string) => !imported.has(x))));
      setFolder(p);
    } catch (e) {
      show((e as Error).message);
    }
    setBusy(false);
  };
  const importFolder = async (paths: string[]) => {
    setBusy(true);
    try {
      const r = await api.post("/api/library/skills/import", { path: folder.path, paths });
      const imported: Json[] = r.imported ?? [];
      setFolder((r.warnings ?? []).length ? { ...folder, imported: [...(folder.imported ?? []), ...imported], warnings: r.warnings } : null);
      await loadLibrary();
      if (imported.length) show(t`已导入 ${imported.map((i) => i.name).join(t`、`)}`);
    } catch (e) {
      show((e as Error).message);
    }
    setBusy(false);
  };
  const importOffers = async (id: string, paths: string[]) => {
    try {
      const r = await api.post(`/api/library/sources/${encodeURIComponent(id)}/import`, { paths });
      setPicks((p) => ({ ...p, [id]: new Set() }));
      await loadLibrary();
      show(t`已导入 ${(r ?? []).map((i: Json) => i.name).join(t`、`)}`);
    } catch (e) {
      show((e as Error).message);
    }
  };
  const removeSource = async (s: Json) => {
    if (!confirmed(t`删除来源 ${s.url}？`, t`本机的仓库副本会删除。还有从它导入的资源时不能删除。`)) return;
    try {
      await api.delete(`/api/library/sources/${encodeURIComponent(s.id)}`);
      await loadLibrary();
    } catch (e) {
      show((e as Error).message);
    }
  };

  return (
    <>
      <RouterGuide
        kind="skills"
        names={names}
        searched={searched}
        now={skills.length === 0 ? t`还没有 Skill。加进来就全员可用。` : searched ? t`${skills.length} 个 Skill 全员可用，经 janus-skills 按关键词搜索。` : t`${skills.length} 个 Skill 全员可用，经 janus-skills 直接列出。`}
      />
      <div class="section-row">
        <strong>{t`已添加`}</strong>
        <Pill text={String(skills.length)} />
        <span class="caption grow">{t`按来源分组`}</span>
        {sources.length > 0 && (
          <button type="button" class="link" disabled={checking} title={t`服务每 6 小时检查一次`} onClick={() => void checkSources()}>
            {checking ? t`正在检查…` : t`检查 GitHub 更新`}
          </button>
        )}
      </div>
      {updates.length > 0 && (
        <div class="soft-box wrap-row">
          <Icon name="arrow.triangle.2.circlepath" class="fg-blue" />
          <span class="grow">{t`${updates.length} 个 GitHub 来源有更新`}</span>
          <button type="button" class="link" onClick={() => setOpened((o) => ({ ...o, ...Object.fromEntries(updates.map((s) => [`source:${s.id}`, true])) }))}>
            {t`查看`}
          </button>
          <Button kind="primary" onClick={() => void updateSources(updates.map((s) => s.id))}>{t`全部更新`}</Button>
        </div>
      )}
      <div class="link-field" title={t`Janus 只读取仓库、列出 Skill，不执行任何文件。第三方内容先看再导入。`}>
        <Icon name="link" class="secondary" />
        <TextInput label={t`拖入或粘贴 GitHub 链接：仓库，或其中的一个目录`} placeholder={t`拖入或粘贴 GitHub 链接：仓库，或其中的一个目录`} value={link_} disabled={adding} onInput={setLink} onEnter={() => void addSource()} />
        {adding ? (
          <span class="caption">
            <Spinner /> {t`正在拉取…`}
          </span>
        ) : (
          <Button disabled={!link_.trim()} onClick={() => void addSource()}>{t`查找 Skill`}</Button>
        )}
      </div>
      <div class="wrap-row nowrap">
        <TextInput label={t`选择一个 Skill 文件夹，或包含多个 Skill 的父文件夹`} placeholder={t`选择一个 Skill 文件夹，或包含多个 Skill 的父文件夹`} value={folderPath} onInput={setFolderPath} onEnter={() => void scan()} />
        <Button disabled={busy || !folderPath.trim()} onClick={() => void scan()}>{t`从文件夹…`}</Button>
        <a class="btn btn-bordered" {...link("/settings/library/skills/new")}>{t`新建`}</a>
      </div>
      {folder && (
        <div class="soft-box">
          <div class="section-row">
            <strong class="grow mono">{folder.path}</strong>
            <button type="button" class="link" onClick={() => setFolder(null)}>{t`取消`}</button>
          </div>
          <Offers preview={folder} picked={folderPicks} onPick={setFolderPicks} onImport={(p) => void importFolder(p)} />
        </div>
      )}
      {shown.length === 0 && <div class="secondary">{query ? t`没有匹配的 Skill` : t`还没有 Skill。拖个 GitHub 链接进来，或者从文件夹导入、新建、挑个推荐。`}</div>}
      {shown.map((g) => {
        const list = g.list.filter(matches(query));
        const open = isOpen(g);
        const toggle = () => setOpened((o) => ({ ...o, [key(g)]: !open }));
        if (g.type === "source") {
          const s = sources.find((x) => x.id === g.source) ?? {};
          const rest: Json[] = s.pending ?? [];
          const isChoosing = choosing.has(s.id);
          return (
            <Group icon="shippingbox" title={s.name} caption={s.caption} count={g.list.length} open={open} onToggle={toggle} trailing={updating.has(s.id) ? <Spinner /> : s.updateAvailable ? <><Pill text={t`有更新`} tone="blue" /><Button small onClick={() => void updateSources([s.id])}>{t`更新`}</Button></> : null}>
              {list.map((i: Json) => <Row i={i} />)}
              {rest.length > 0 && (
                <div class="offer-box">
                  <button type="button" class="link" onClick={() => setChoosing((c) => { const n = new Set(c); if (n.has(s.id)) n.delete(s.id); else n.add(s.id); return n; })}>
                    {isChoosing ? t`收起未导入的 Skill` : t`从这个仓库再导入（${rest.length} 个未导入）`}
                  </button>
                  {isChoosing && <Offers preview={{ ...s, offers: rest }} picked={picks_[s.id] ?? new Set()} onPick={(p) => setPicks((x) => ({ ...x, [s.id]: p }))} onImport={(p) => void importOffers(s.id, p)} />}
                </div>
              )}
              <div class="group-foot">
                {s.checkError && <Note text={t`检查更新失败：${s.checkError}`} tone="amber" />}
                <div class="wrap-row nowrap">
                  <Icon name="link" size={12} class="secondary" />
                  <span class="caption ellipsis grow">{s.url}</span>
                  <button type="button" class="link caption" disabled={g.list.length > 0} title={g.list.length ? t`还有从它导入的 Skill 时不能删除` : ""} onClick={() => void removeSource(s)}>
                    {t`删除来源…`}
                  </button>
                </div>
              </div>
            </Group>
          );
        }
        return (
          <Group icon={g.type === "folder" ? "folder" : "square.and.pencil"} title={g.type === "folder" ? g.path : t`在 Janus 中新建`} caption={g.type === "folder" ? t`本机文件夹 · 导入时的快照，展开可刷新` : t`可以在 Janus 里编辑`} count={g.list.length} open={open} onToggle={toggle}>
            {list.map((i: Json) => <Row i={i} />)}
          </Group>
        );
      })}
    </>
  );
}

function Group({ icon, title, caption, count, open, onToggle, trailing, children }: { icon: string; title: string; caption: string; count: number; open: boolean; onToggle: () => void; trailing?: preact.ComponentChildren; children: preact.ComponentChildren }) {
  return (
    <div class="card lib-group">
      <div class="lib-group-head">
        <button type="button" class="row-main" aria-expanded={open} title={open ? t`收起` : t`展开`} onClick={onToggle}>
          <span class={"chevron" + (open ? " open" : "")}>
            <Icon name="chevron.right" size={11} />
          </span>
          <span class="icon-tile">
            <Icon name={icon} size={14} />
          </span>
          <span class="grow min0">
            <strong class="ellipsis block">{title}</strong>
            <span class="caption ellipsis block">{caption}</span>
          </span>
        </button>
        {trailing}
        <Pill text={t`${count} 个`} />
      </div>
      {open && <div class="lib-group-body">{children}</div>}
    </div>
  );
}

/** Choosing among the skills a folder or a source offers; the ones imported already say so. */
function Offers({ preview, picked, onPick, onImport }: { preview: Json; picked: Set<string>; onPick: (p: Set<string>) => void; onImport: (paths: string[]) => void }) {
  const imported = new Map<string, string>((preview.imported ?? []).map((i: Json) => [i.path, i.name]));
  const offers: Json[] = preview.offers ?? [];
  const available = offers.map((o) => o.path as string).filter((p) => !imported.has(p));
  const chosen = available.filter((p) => picked.has(p));
  const text = (o: Json, extra: string) => (
    <span>
      <strong>{o.name}</strong> {extra && <span class="caption">{extra}</span>}
      <span class="caption block">{[o.description, o.path].filter(Boolean).join(" · ")}</span>
    </span>
  );
  return (
    <div class="offers">
      {offers.map((o) => {
        const name = imported.get(o.path);
        return name !== undefined ? (
          <div class="check-row">
            <Icon name="checkmark.circle" class="fg-green" />
            {text(o, name === o.name ? t`已导入` : t`已导入为 ${name}`)}
          </div>
        ) : (
          <Check
            on={picked.has(o.path)}
            onChange={(on) => {
              const n = new Set(picked);
              if (on) n.add(o.path);
              else n.delete(o.path);
              onPick(n);
            }}
          >
            {text(o, "")}
          </Check>
        );
      })}
      {(preview.warnings ?? []).map((w: string) => (
        <div class="caption fg-amber">⚠ {w}</div>
      ))}
      {available.length > 0 && (
        <div class="wrap-row">
          <button type="button" class="link" onClick={() => onPick(chosen.length === available.length ? new Set() : new Set(available))}>
            {chosen.length === available.length ? t`全不选` : t`全选`}
          </button>
          <span class="grow" />
          <Button kind="primary" disabled={!chosen.length} onClick={() => onImport(chosen)}>{t`导入所选（${chosen.length}）`}</Button>
        </div>
      )}
    </div>
  );
}

/** One resource: its name, a line, what keeps it from working; its details fold open. */
function Row({ i }: { i: Json }) {
  const [open, setOpen] = useState(false);
  const mcp = i.kind === "mcp";
  const warm = i.warm ?? {};
  const lines = [i.endpoint, i.description, i.originText, i.varsText, i.server?.transport === "stdio" ? t`同一任务共用一个进程，用到才启动` : "", mcp ? "" : usage(i)].filter(Boolean);
  const remove = async () => {
    if (!confirmed(t`删除 ${i.name}？`, t`进行中的任务不受影响；这个服务器会停下，它的值一起删除。`)) return;
    try {
      await api.delete(`/api/library/items/${encodeURIComponent(i.id)}`);
      await loadLibrary();
    } catch (e) {
      show((e as Error).message);
    }
  };
  const refresh = async () => {
    try {
      const r = await api.post(`/api/library/items/${encodeURIComponent(i.id)}/refresh`);
      await loadLibrary();
      show(r.revision === i.revision ? t`${i.name} 没有变化` : t`已刷新 ${i.name}，下个任务起生效`);
    } catch (e) {
      show((e as Error).message);
    }
  };
  const warmUp = async () => {
    try {
      await api.post(`/api/library/mcp/${encodeURIComponent(i.id)}/warm`);
      await loadLibrary();
    } catch (e) {
      show((e as Error).message);
    }
  };
  return (
    <div class="lib-row" data-item={i.name}>
      <button type="button" class="lib-row-head" aria-expanded={open} title={open ? t`收起` : t`展开详情`} onClick={() => setOpen(!open)}>
        {mcp && <Icon name={i.server?.transport === "stdio" ? "terminal" : "globe"} size={14} class="fg-blue" />}
        <span class="mono name">{i.name}</span>
        <span class="caption ellipsis grow">{i.description || i.endpoint || ""}</span>
        {i.warning && <span class="caption fg-amber ellipsis">{i.warning}</span>}
        {warm.state === "preparing" && <Pill text={t`准备中…`} tone="blue" />}
        {warm.state === "failed" && <span class="caption fg-red">{t`启动失败`}</span>}
        <span class={"chevron" + (open ? " open" : "")}>
          <Icon name="chevron.right" size={11} />
        </span>
      </button>
      {open && (
        <div class="lib-row-body">
          {lines.map((l) => (
            <div class="caption">{l}</div>
          ))}
          {(i.hintTexts ?? []).map((h: string) => (
            <div class="caption fg-amber">{h}</div>
          ))}
          {mcp && (
            <div class="wrap-row caption">
              {warm.state === "preparing" ? (
                <span>{i.server?.transport === "stdio" ? t`首次启动，正在下载依赖，大的包要等几分钟…` : t`正在连接，列出它的工具…`}</span>
              ) : warm.state === "ready" ? (
                <span class="fg-green">{t`已准备好 · ${warm.tools ?? 0} 个工具`}</span>
              ) : warm.state === "failed" ? (
                <span class="fg-red">{t`启动失败：${warm.error ?? ""}`}</span>
              ) : (
                <span>{t`还没有准备过：任务里第一次用到时再启动或连接`}</span>
              )}
              {warm.state !== "preparing" && (
                <button type="button" class="link" onClick={() => void warmUp()}>
                  {t`重新准备`}
                </button>
              )}
            </div>
          )}
          <div class="wrap-row">
            {(mcp || i.origin?.type === "created") && <a class="link" {...link(`/settings/library/${mcp ? "servers" : "skills"}/${encodeURIComponent(i.id)}`)}>{t`编辑`}</a>}
            {i.origin?.type === "folder" && (
              <button type="button" class="link" title={t`从原文件夹重新复制`} onClick={() => void refresh()}>
                {t`刷新`}
              </button>
            )}
            <button type="button" class="link" onClick={() => void remove()}>
              {t`删除`}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

/** How often a skill was read, and when last: the service's words, then the time. */
function usage(i: Json): string {
  const last = i.uses?.lastAt;
  return (i.usageText ?? "") + (typeof last === "number" ? ` · ${t`最近一次：${ago(last)}`}` : "");
}

// --- MCP servers ----------------------------------------------------------------------------------

function McpTab({ query }: { query: string }) {
  const servers = items("servers");
  const code = library.value?.mcpRouter?.code ?? true;
  const shown = servers.filter(matches(query));
  const setCode = async (on: boolean) => {
    try {
      await api.put("/api/library/router", { code: on });
      await loadLibrary();
    } catch (e) {
      show((e as Error).message);
    }
  };
  return (
    <>
      <RouterGuide kind="servers" names={servers.map((s) => s.name)} now={servers.length === 0 ? t`还没有 MCP 服务器。加进来就全员可用。` : t`${servers.length} 个服务器全员可用，经 janus-tools 一个入口取用。`} />
      <Card>
        <SwitchRow on={code} onChange={(v) => void setCode(v)} title={t`代码模式`} sub={t`Agent 可以用 run_code 写一段调用这些工具的 JavaScript，一步完成多次调用。`} />
      </Card>
      <div class="section-row">
        <strong>{t`已添加`}</strong>
        <Pill text={String(servers.length)} />
        <span class="caption grow">{t`全员可用，经 janus-tools 取用`}</span>
        <a class="btn btn-bordered" title={t`填表单，或切到 JSON 贴 README 里的配置`} {...link("/settings/library/servers/new")}>{t`添加服务器`}</a>
      </div>
      <div class="caption">{t`值只存在本机，不进数据库、任务记录和事件。`}</div>
      {shown.length === 0 ? (
        <div class="secondary">{query ? t`没有匹配的 MCP 服务器` : t`还没有 MCP 服务器`}</div>
      ) : (
        <div class="card rows">
          {shown.map((i) => (
            <Row i={i} />
          ))}
        </div>
      )}
    </>
  );
}

// --- The community's picks ---------------------------------------------------------------------

/** Tiles shown before「显示全部」. */
const FIRST = 6;

function Shelf({ kind, query }: { kind: LibraryKind; query: string }) {
  const [open, setOpen] = useState(load("catalogOpen", true));
  const [all, setAll] = useState(false);
  const [busy, setBusy] = useState<Set<string>>(new Set());
  const q = query.toLowerCase();
  const tiles: Json[] = (picks.value?.[kind] ?? []).filter((p: Json) => !q || String(p.search).includes(q));
  if (!tiles.length && q) return null;
  const shown = q || all ? tiles : tiles.slice(0, FIRST);
  const add = async (p: Json) => {
    setBusy((b) => new Set(b).add(p.id));
    try {
      const r = await api.post(`/api/library/picks/add?lang=${serverLang()}`, { kind: kind === "skills" ? "skill" : "mcp", id: p.id });
      if (r.editor) {
        pendingEditor.value = r.editor;
        go("/settings/library/servers/new");
      } else {
        await loadLibrary();
        show(r.message);
      }
    } catch (e) {
      await loadLibrary();
      show((e as Error).message);
    }
    setBusy((b) => {
      const n = new Set(b);
      n.delete(p.id);
      return n;
    });
  };
  return (
    <div class="shelf">
      <div class="section-row">
        <strong>{t`推荐`}</strong>
        <Pill text={String(tiles.length)} />
        <span class="caption grow">{kind === "skills" ? t`社区常用的 Skill，一键导入` : t`社区常用的 MCP 服务器，一键添加`}</span>
        <button
          type="button"
          class="link"
          onClick={() => {
            setOpen(!open);
            save("catalogOpen", !open);
          }}
        >
          {open ? t`收起` : t`展开`}
        </button>
      </div>
      {(open || q) && (
        <>
          <div class="tiles">
            {shown.map((p) => (
              <div class="card tile" data-pick={p.id}>
                <div class="tile-head">
                  <span class="icon-tile">
                    <Icon name={p.icon} size={14} />
                  </span>
                  <span class="grow min0">
                    <strong class="ellipsis block">{p.title}</strong>
                    <span class="caption ellipsis block">{p.byline}</span>
                  </span>
                  {busy.has(p.id) ? (
                    <Spinner />
                  ) : p.have ? (
                    <span class="caption fg-green">
                      <Icon name="checkmark" size={12} /> {t`已添加`}
                    </span>
                  ) : (
                    <Button small onClick={() => void add(p)}>{t`添加`}</Button>
                  )}
                </div>
                <div class="caption clamp2">{p.blurb}</div>
                {(p.tags ?? []).length > 0 && (
                  <div class="wrap-row">
                    {p.tags.map((tag: string) => (
                      <Pill text={tag} />
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
          {!q && tiles.length > FIRST && (
            <button type="button" class="link" onClick={() => setAll(!all)}>
              {all ? t`收起` : t`显示全部 ${tiles.length} 个`}
            </button>
          )}
        </>
      )}
    </div>
  );
}

// --- Editors --------------------------------------------------------------------------------------

/** A skill written in Janus: name, description and body when new; the whole SKILL.md when edited. */
export function SkillEditor({ id }: { id: string }) {
  const editing = id !== "new";
  const [d, setD] = useState<Json>(editing ? null : { name: "", description: "", content: "" });
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    if (!editing) return;
    api.get(`/api/library/items/${encodeURIComponent(id)}?lang=${serverLang()}`).then((full: Json) => setD({ id: full.id, name: full.name, content: full.content }), (e: Error) => show(e.message));
  }, [id]);
  if (!d) return <Spinner />;
  const set = (k: string, v: string) => setD((x: Json) => ({ ...x, [k]: v }));
  const back = () => go("/settings/library/skills");
  const save = async () => {
    setSaving(true);
    try {
      const body = { name: d.name, description: d.description, content: d.content };
      const item = d.id ? await api.put(`/api/library/skills/${encodeURIComponent(d.id)}`, body) : await api.post("/api/library/skills", body);
      await loadLibrary();
      show(t`已保存 Skill ${item.name}`);
      back();
    } catch (e) {
      show((e as Error).message);
    }
    setSaving(false);
  };
  return (
    <div class="page settings-editor">
      <FormPage
        title={d.id ? t`编辑 Skill` : t`新建 Skill`}
        footer={
          <>
            <span class="grow" />
            <Button onClick={back}>{t`取消`}</Button>
            <Button kind="primary" disabled={saving} onClick={() => void save()}>
              {saving ? t`正在保存…` : t`保存`}
            </Button>
          </>
        }
      >
        <TextInput label={t`名称：小写字母、数字和 -，例如 release-notes`} placeholder={t`名称：小写字母、数字和 -，例如 release-notes`} value={d.name} onInput={(v) => set("name", v)} />
        {!d.id && <TextInput label={t`什么时候用，例如：写发布说明时使用`} placeholder={t`什么时候用，例如：写发布说明时使用`} value={d.description} onInput={(v) => set("description", v)} />}
        <div class="caption">{d.id ? t`SKILL.md（名称以上面填写的为准）` : t`正文：Agent 使用这个 Skill 时读到的说明`}</div>
        <TextArea mono rows={14} label="SKILL.md" value={d.content} onInput={(v) => set("content", v)} />
      </FormPage>
    </div>
  );
}

/** Add or change an MCP server, as a form or as JSON in Claude Code's `.mcp.json` shape; the
 * service takes the command line apart, reads the JSON and writes the server out
 * (`POST /api/library/mcp/draft`). */
export function McpEditor({ id }: { id: string }) {
  const editing = id !== "new";
  const [e, setE] = useState<Json>(null);
  const [mode, setMode] = useState<"form" | "json">("form");
  const [line, setLine] = useState("");
  const [text, setText] = useState("");
  const [bad, setBad] = useState("");
  const [several, setSeveral] = useState<string[]>([]);
  const [note, setNote] = useState("");
  const [saving, setSaving] = useState(false);
  const sent = useRef(0);
  const item = editing ? (library.value?.items ?? []).find((i: Json) => i.id === id) : null;
  const take = (edit: Json) => {
    setE(edit);
    setLine(edit.line);
    setText(edit.json);
  };
  useEffect(() => {
    if (editing) {
      api.get(`/api/library/mcp/${encodeURIComponent(id)}/draft?lang=${serverLang()}`).then(take, (x: Error) => show(x.message));
    } else if (pendingEditor.value) {
      take(pendingEditor.value);
      setNote(pendingEditor.value.note ?? "");
      pendingEditor.value = null;
    } else {
      void edit({}, {}).then((r) => r && take(r));
    }
  }, [id]);
  /** One edit sent to the service; an answer to an older one is dropped. */
  async function edit(draft: Json, extra: Json): Promise<Json | null> {
    const mine = ++sent.current;
    try {
      const r = await api.post(`/api/library/mcp/draft?lang=${serverLang()}`, { draft, ...extra });
      return mine === sent.current ? r : null;
    } catch (x) {
      show((x as Error).message);
      return null;
    }
  }
  if (!e) return <Spinner />;
  const d: Json = e.draft;
  const setD = (patch: Json) => setE((x: Json) => ({ ...x, draft: { ...x.draft, ...patch } }));
  const back = () => go("/settings/library/servers");
  const switchTo = async (next: "form" | "json") => {
    if (next === mode) return;
    if (next === "form" && !(bad === "" && several.length === 0)) {
      if (!bad) setBad(t`这里有多个服务器，表单一次只能编辑一个`);
      return;
    }
    const r = await edit(d, {});
    if (!r) return;
    if (next === "json") {
      setText(r.json);
      setBad("");
    } else setLine(r.line);
    setMode(next);
  };
  const typeLine = async (v: string) => {
    setLine(v);
    const r = await edit(d, { line: v });
    if (r) setD({ command: r.draft.command, args: r.draft.args });
  };
  const typeText = async (v: string) => {
    setText(v);
    const r = await edit(d, { text: v });
    if (!r) return;
    if (r.error) {
      setBad(r.error);
      if (!r.several.length && !editing) setSeveral([]);
      return;
    }
    setBad("");
    setSeveral(r.several);
    if (!r.several.length) setE((x: Json) => ({ ...x, draft: r.draft }));
  };
  const save = async () => {
    setSaving(true);
    try {
      let saved: Json[] = [];
      let warnings: string[] = [];
      if (!several.length) {
        const r = await edit(d, {});
        if (!r) throw new Error(t`这不是 MCP 服务器的 JSON`);
        saved = [d.id ? await api.put(`/api/library/mcp/${encodeURIComponent(d.id)}`, r.body) : await api.post("/api/library/mcp", r.body)];
      } else {
        const r = await api.post("/api/library/mcp/import", { text });
        saved = r.items ?? [];
        warnings = r.warnings ?? [];
      }
      await loadLibrary();
      const names = saved.map((s) => s.name).join(t`、`);
      show(warnings.length ? t`已添加 ${names}；跳过：${warnings.join(t`；`)}` : t`已保存 ${names}`);
      back();
    } catch (x) {
      show((x as Error).message);
    }
    setSaving(false);
  };
  const remove = async () => {
    if (!confirmed(t`删除 ${d.name}？`, t`进行中的任务不受影响；这个服务器会停下，它的值一起删除。`)) return;
    try {
      await api.delete(`/api/library/items/${encodeURIComponent(d.id)}`);
      await loadLibrary();
      back();
    } catch (x) {
      show((x as Error).message);
    }
  };
  const vars: Json[] = d.vars ?? [];
  const setVar = (i: number, patch: Json) => setD({ vars: vars.map((v, j) => (j === i ? { ...v, ...patch } : v)) });
  const valuesHint = t`值只存在本机（权限 0600），会话启动时交给 Agent，不进数据库和日志。`;
  const pairs = (key: string) => (
    <div class="pairs">
      {vars.map((v, i) => (
        <div class="wrap-row nowrap">
          <TextInput mono label={key} placeholder={key} value={v.name} onInput={(x) => setVar(i, { name: x })} />
          <TextInput mono label={t`值`} placeholder={t`值`} value={v.value} onInput={(x) => setVar(i, { value: x })} />
          <button type="button" class="icon-btn" aria-label={t`移除这个变量和它的值`} title={t`移除这个变量和它的值`} onClick={() => setD({ vars: vars.filter((_, j) => j !== i) })}>
            <Icon name="xmark" size={12} />
          </button>
        </div>
      ))}
      <button type="button" class="link" onClick={() => setD({ vars: [...vars, { name: "", value: "" }] })}>
        {t`+ 添加`}
      </button>
    </div>
  );
  return (
    <div class="page settings-editor">
      <FormPage
        title={d.name || (item || editing ? t`MCP 服务器` : t`添加 MCP 服务器`)}
        sub={editing ? undefined : t`填表单，或切到 JSON 贴 README 里的配置`}
        icon={<span class="icon-tile"><Icon name={d.transport === "stdio" ? "terminal" : "globe"} /></span>}
        footer={
          <>
            {editing && (
              <Button kind="plain" onClick={() => void remove()}>
                <span class="fg-red">{t`从资源库删除…`}</span>
              </Button>
            )}
            <span class="grow" />
            <Button onClick={back}>{t`取消`}</Button>
            <Button kind="primary" disabled={saving || bad !== ""} onClick={() => void save()}>
              {saving ? t`正在保存…` : several.length ? t`添加 ${several.length} 个服务器` : t`保存`}
            </Button>
          </>
        }
      >
        <Segmented label={t`编辑方式`} value={mode} options={[["form", t`表单`], ["json", "JSON"]]} onChange={(v) => void switchTo(v)} />
        {note && <Note text={note} tone="blue" />}
        {mode === "json" ? (
          <>
            <Field label="JSON" tone={(bad !== "") ? "red" : undefined} note={bad || (editing ? t`与 Claude Code 的 .mcp.json 格式相同，值也在里面。` : t`兼容 Claude Code 的 .mcp.json 和 VS Code 的 servers，可以一次贴多个。`)}>
              <TextArea mono rows={12} label="JSON" value={text} onInput={(v) => void typeText(v)} />
            </Field>
            {several.length > 0 && <Note text={t`JSON 里有 ${several.length} 个服务器：${several.join(t`、`)}。保存时全部添加，重名自动加后缀。`} tone="blue" />}
          </>
        ) : (
          <>
            <Field label={t`名称`} note={t`字母、数字、- 和 _，Agent 用这个名字称呼它`}>
              <TextInput label={t`名称`} placeholder={t`例如 github`} value={d.name} onInput={(v) => setD({ name: v })} />
            </Field>
            <Field group label={t`运行方式`} note={d.transport === "stdio" ? "" : t`Janus 连接这个地址，带上下面的 header；Agent 只看到 janus-tools。`}>
              <Segmented label={t`运行方式`} value={d.transport} options={[["stdio", t`命令`], ["http", "HTTP"], ["sse", "SSE"]]} onChange={(v) => setD({ transport: v })} />
            </Field>
            {d.transport === "stdio" ? (
              <>
                <Field label={t`命令`} note={t`命令和它的参数，像在终端里那样写；含空格的参数加引号`}>
                  <TextInput mono label={t`命令`} placeholder="npx -y @modelcontextprotocol/server-github" value={line} onInput={(v) => void typeLine(v)} />
                </Field>
                <Field group label={t`环境变量`} note={valuesHint}>
                  {pairs("GITHUB_TOKEN")}
                </Field>
              </>
            ) : (
              <>
                <Field label="URL">
                  <TextInput mono label="URL" placeholder="https://example.com/mcp" value={d.url} onInput={(v) => setD({ url: v })} />
                </Field>
                <Field group label={t`请求头`} note={valuesHint}>
                  {pairs("Authorization")}
                </Field>
              </>
            )}
          </>
        )}
        {!several.length && (
          <Field label={t`说明`} note={t`可选：它做什么、什么时候用。经 MCP 路由时，Agent 在 janus-tools 的说明里读到它。`}>
            <TextInput label={t`说明`} placeholder={t`例如：店里的数据库，只读`} value={d.description} onInput={(v) => setD({ description: v })} />
          </Field>
        )}
      </FormPage>
    </div>
  );
}
