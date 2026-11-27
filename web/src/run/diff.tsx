// Reviewing a diff the run saved (Mac: UI/DiffReview.swift): its files and hunks as the service
// parses them (`GET /api/runs/{id}/diff?path=`), a note on any line, and the notes sent to the lead
// or the task's agent as one message the service writes (`POST /api/runs/{id}/diff/message`).

import { useEffect, useState } from "preact/hooks";
import { api, type Json, query } from "../lib/api.ts";
import { t } from "../lib/i18n.ts";
import { show } from "../lib/store.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { Button, Card, IconButton, Spinner } from "../ui/kit.tsx";
import { Icon } from "../ui/Icon.tsx";
import type { RunModel } from "../session/model.ts";
import { who } from "../session/Items.tsx";
import { runLink } from "./nav.ts";

export function DiffPage({ run, path }: { run: RunModel; path: string }) {
  const st = run.view.value;
  const artifact = (st?.artifacts ?? []).find((a: Json) => a.path === path);
  const ofRun = artifact?.kind === "run_diff";
  const title = artifact ? (ofRun ? t`审阅整个任务的改动 · ${artifact.name}` : t`审阅 ${artifact.task} 的改动 · ${artifact.name}`) : t`审阅改动`;
  usePageTitle(title, runLink(run.id));
  const [files, setFiles] = useState<Json[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [editing, setEditing] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const [comments, setComments] = useState<Record<number, string>>({});
  const [summary, setSummary] = useState("");
  const [sent, setSent] = useState<string | null>(null);
  useEffect(() => {
    api.get(`/api/runs/${encodeURIComponent(run.id)}/diff${query({ path })}`).then(
      (j) => (j.files?.length ? setFiles(j.files) : setError(t`这个 diff 是空的`)),
      () => setError(t`读不了这个 diff`),
    );
  }, [run.id, path]);
  useDrawn("diff", () => ({ path, files: (files ?? []).map((f) => ({ path: f.path, added: f.added, removed: f.removed, hunks: f.hunks.length })), comments: Object.keys(comments).length }));
  const agent = (st?.tasks ?? []).find((x: Json) => x.id === artifact?.task)?.agent ?? "";
  const name = who(run, agent).name;
  const ready = Object.keys(comments).length > 0 || summary.trim() !== "";
  const can = run.canAct.value && !st?.readOnly;
  const send = async (to: string, label: string) => {
    try {
      const body = { path, summary, comments: Object.entries(comments).map(([line, text]) => ({ line: Number(line), text })) };
      const { text } = await api.post(`/api/runs/${encodeURIComponent(run.id)}/diff/message`, body);
      if (await run.act({ type: "message", task: "", agent: to, text })) {
        setComments({});
        setSummary("");
        setSent(t`已发给${label}`);
      }
    } catch (e) {
      show(e instanceof Error ? e.message : String(e));
    }
  };
  const shown = files ? (selected === null ? files : files.filter((f) => f.id === selected)) : [];
  return (
    <div class="page wide diff-page">
      <div class="section-header">
        <h1 class="display">{title}</h1>
      </div>
      {error ? (
        <div class="secondary">{error}</div>
      ) : !files ? (
        <Spinner />
      ) : (
        <div class="diff-layout">
          <Card pad={4} class="diff-files rows">
            <div class="caption secondary diff-files-head">{t`${files.length} 个文件`}</div>
            {files.map((f) => (
              <button type="button" class={"row" + (selected === f.id ? " selected" : "")} title={f.path} onClick={() => setSelected(selected === f.id ? null : f.id)}>
                <span class="grow ellipsis">{f.path.split("/").at(-1)}</span>
                <span class="caption secondary">{f.untracked ? t`新文件` : `+${f.added} −${f.removed}`}</span>
              </button>
            ))}
          </Card>
          <div class="diff-hunks">
            {shown.map((f) => (
              <div class="card diff-file">
                <div class="diff-file-head">
                  <span class="mono strong grow ellipsis">{f.path}</span>
                  {!f.untracked && <span class="caption secondary">{`+${f.added} −${f.removed}`}</span>}
                </div>
                {f.untracked && !f.hunks.length && <div class="callout secondary pad">{t`新文件，没有可显示的文本改动；内容在工作目录里查看。`}</div>}
                {f.hunks.map((h: Json) => (
                  <>
                    <div class="hunk-head mono caption secondary">{h.header}</div>
                    {h.lines.map((l: Json) => (
                      <div class="diff-row">
                        <div class={"diff-code mono " + l.kind}>
                          <span class="ln">{l.old ?? ""}</span>
                          <span class="ln">{l.new ?? ""}</span>
                          {can ? (
                            <button type="button" class="comment-btn" title={t`对这一行写审阅意见`} aria-label={t`对这一行写审阅意见`} onClick={() => { setEditing(l.id); setDraft(comments[l.id] ?? ""); }}>
                              <Icon name="plus.bubble.fill" size={14} />
                            </button>
                          ) : (
                            <span class="comment-btn" />
                          )}
                          <span class="code">{(l.kind === "added" ? "+" : l.kind === "removed" ? "−" : " ") + l.text}</span>
                        </div>
                        {editing === l.id ? (
                          <div class="comment-edit">
                            <textarea class="field" rows={2} placeholder={t`这一行要怎么改`} value={draft} onInput={(e) => setDraft(e.currentTarget.value)} />
                            <div class="flow end">
                              {comments[l.id] !== undefined && (
                                <Button onClick={() => { const c = { ...comments }; delete c[l.id]; setComments(c); setEditing(null); }}>{t`删除`}</Button>
                              )}
                              <Button onClick={() => setEditing(null)}>{t`取消`}</Button>
                              <Button kind="primary" onClick={() => { const c = { ...comments }; const x = draft.trim(); if (x) c[l.id] = x; else delete c[l.id]; setComments(c); setEditing(null); }}>{t`添加`}</Button>
                            </div>
                          </div>
                        ) : comments[l.id] !== undefined ? (
                          <div class="comment">
                            <Icon name="text.bubble" size={14} />
                            <span class="grow callout">{comments[l.id]}</span>
                            <IconButton icon="square.and.pencil" label={t`编辑这条意见`} size={12} onClick={() => { setEditing(l.id); setDraft(comments[l.id] ?? ""); }} />
                          </div>
                        ) : null}
                      </div>
                    ))}
                  </>
                ))}
              </div>
            ))}
          </div>
        </div>
      )}
      <div class="diff-footer card">
        <input class="field grow" placeholder={t`总体意见（可选）`} value={summary} onInput={(e) => setSummary(e.currentTarget.value)} />
        <span class="callout secondary">{t`${Object.keys(comments).length} 条意见`}</span>
        {sent && <span class="callout fg-green">{sent}</span>}
        {!ofRun && (
          <Button disabled={!can || !ready || !agent} title={t`直接发给做这个子任务的 Agent：在线就插进这一轮，不在线就下次会话带给它`} onClick={() => void send(agent, name)}>
            {t`发给${name}`}
          </Button>
        )}
        <Button kind="primary" disabled={!can || !ready} title={can ? t`负责人读完决定：让协作者修改、重做，或者自己处理` : t`这个任务已经结束，不能再发消息`} onClick={() => void send("lead", t`负责人`)}>
          {t`交给负责人`}
        </Button>
      </div>
    </div>
  );
}
