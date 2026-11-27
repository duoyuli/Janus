// The smaller pages of the first web version: a run's page while it is being built (a Loop's and
// a consolidation's have their own), and a path that names nothing.

import { useSignal } from "@preact/signals";
import { useEffect } from "preact/hooks";
import { api, type Json } from "../lib/api.ts";
import { serverLang, t } from "../lib/i18n.ts";
import { onNotice } from "../lib/stream.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { Note, Pill } from "../ui/kit.tsx";
import { LoopPage } from "./loops.tsx";
import { DreamPage } from "./memory.tsx";

/** A run's page: what it is and how it stands, until the full page (board, sessions, review) is
 * in. A Loop's session and a consolidation have pages of their own. The run's view is read as
 * the service writes it (`GET /api/runs/{id}/view?lang=`), and again when the run changes. */
export function RunPage({ id }: { id: string }) {
  const view = useSignal<Json>(null);
  useEffect(() => {
    const load = () =>
      api.get(`/api/runs/${encodeURIComponent(id)}/view?lang=${serverLang()}`).then(
        (v) => (view.value = v),
        () => {},
      );
    void load();
    return onNotice((name, n) => {
      if ((name === "run" || name === "event") && n.run === id) void load();
    });
  }, [id, serverLang()]);
  const v = view.value;
  const kind = v?.run?.kind ?? null;
  useDrawn("run", () => ({ id, kind, status: v?.run?.status ?? null }));
  if (!v) return <div class="page" />;
  const later = <Note text={t`这个页面在网页上还没有做完：先在 Mac 上的 Janus 打开它。`} />;
  const run = { ...v.run, id, title: v.title };
  if (kind === "loop") return <LoopPage run={run} session={later} />;
  if (kind === "dream") return <DreamPage run={run} session={later} />;
  return <PlainRun v={v} />;
}

function PlainRun({ v }: { v: Json }) {
  const ws = v.run?.workspace ?? "";
  usePageTitle(v.title ?? "", v.run?.kind === "chat" ? "/chats" : ws ? `/workspaces/${encodeURIComponent(ws)}` : "/inbox");
  return (
    <div class="page">
      <div class="section-header">
        <h1 class="display">{v.title}</h1>
        {v.capsule && <Pill text={v.capsule.text} tone={v.capsule.tone} />}
      </div>
      <Note text={t`这个页面在网页上还没有做完：先在 Mac 上的 Janus 打开它。`} />
    </div>
  );
}

export function MissingPage() {
  usePageTitle("");
  return (
    <div class="page">
      <Note text={t`没有这个页面。`} />
    </div>
  );
}
