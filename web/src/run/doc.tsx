// A run's Markdown document (Mac: UI/DocumentViewer.swift), read through the service: one the run
// delivered, saved or mentioned (`GET /api/runs/{id}/doc?path=`), or one such a document links to
// (`/doc/asset?doc=&path=`), whose images come the same way. Shown rendered or as its source.

import { useEffect, useState } from "preact/hooks";
import { type Json, query } from "../lib/api.ts";
import { tilde } from "../lib/format.ts";
import { t } from "../lib/i18n.ts";
import { go } from "../lib/router.ts";
import { show } from "../lib/store.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { Button, Card, Spinner } from "../ui/kit.tsx";
import { Markdown } from "../session/Markdown.tsx";
import type { RunModel } from "../session/model.ts";
import { runLink } from "./nav.ts";

/** `href` from the folder of `doc`, as a path. */
export function resolvePath(doc: string, href: string): string {
  const clean = decodeURIComponent(href.split("#")[0]!.split("?")[0]!);
  if (clean.startsWith("/")) return clean;
  const parts = doc.split("/").slice(0, -1);
  for (const p of clean.split("/")) {
    if (p === "..") parts.pop();
    else if (p && p !== ".") parts.push(p);
  }
  return parts.join("/");
}

const isMarkdown = (p: string) => /\.(md|markdown|mdx)$/i.test(p);

export function DocPage({ run, path, from }: { run: RunModel; path: string; from?: string }) {
  const name = path.split("/").at(-1) ?? path;
  usePageTitle(name, runLink(run.id));
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [source, setSource] = useState(false);
  const [generation, setGeneration] = useState(0);
  const base = `/api/runs/${encodeURIComponent(run.id)}`;
  const url = from ? `${base}/doc/asset${query({ doc: from, path })}` : `${base}/doc${query({ path })}`;
  useEffect(() => {
    setError(null);
    fetch(url, { credentials: "same-origin" })
      .then(async (r) => (r.ok ? setText(await r.text()) : setError((await r.text()) || t`读不了这个文件`)))
      .catch(() => setError(t`读不了这个文件`));
  }, [url, generation]);
  useDrawn("doc", () => ({ path, from: from ?? null, length: text?.length ?? null, error }));
  const links = {
    image: (src: string) => `${base}/doc/asset${query({ doc: path, path: resolvePath(path, src) })}`,
    open: (href: string) => {
      if (/^(https?:|mailto:|#)/.test(href)) return false;
      const target = resolvePath(path, href);
      if (!isMarkdown(target)) return false;
      go(runLink(run.id, `doc/${encodeURIComponent(target)}/${encodeURIComponent(path)}`));
      return true;
    },
  };
  const copy = (s: string, done: string) => {
    void navigator.clipboard?.writeText(s).then(() => show(done), () => show(s));
  };
  return (
    <div class="page doc-page">
      <div class="section-header">
        <h1 class="display">{name}</h1>
      </div>
      <div class="doc-bar">
        <span class="mono caption secondary ellipsis grow" title={path}>{tilde(path)}</span>
        <div class="choices" role="radiogroup" aria-label={t`显示方式`}>
          <button type="button" role="radio" aria-checked={!source} class={"choice" + (!source ? " on" : "")} onClick={() => setSource(false)}>{t`预览`}</button>
          <button type="button" role="radio" aria-checked={source} class={"choice" + (source ? " on" : "")} onClick={() => setSource(true)}>{t`源文本`}</button>
        </div>
        <Button small onClick={() => setGeneration(generation + 1)}>{t`重新读取`}</Button>
        <Button small disabled={text === null} onClick={() => copy(text ?? "", t`已复制`)}>{t`复制内容`}</Button>
        <Button small onClick={() => copy(path, t`已复制`)}>{t`复制路径`}</Button>
      </div>
      <Card>
        {error ? (
          <div class="secondary">{error}</div>
        ) : text === null ? (
          <Spinner />
        ) : source || !isMarkdown(path) ? (
          <pre class="code-block mono">{text}</pre>
        ) : (
          <Markdown text={text} links={links} />
        )}
      </Card>
    </div>
  );
}

/** The run's documents by where they come from, for the 「文档」 menu. */
export function docGroups(docs: Json[]): [string, Json[]][] {
  const title: Record<string, string> = { deliverable: t`交付物`, artifact: t`子任务产物`, mentioned: t`Agent 写过或提到的文档` };
  return ["deliverable", "artifact", "mentioned"].map((s) => [title[s]!, docs.filter((d) => d.source === s)] as [string, Json[]]).filter(([, g]) => g.length > 0);
}
