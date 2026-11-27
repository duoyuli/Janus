// The smaller pages of the first web version: a goal's page while it is being built, and a path
// that names nothing.

import type { Json } from "../lib/api.ts";
import { t } from "../lib/i18n.ts";
import { usePageTitle } from "../lib/title.ts";
import { Note, Pill } from "../ui/kit.tsx";

/** A goal's page from its view: what it is and how it stands, until the full page (board, sessions, review) is in. */
export function PlainRun({ v }: { v: Json }) {
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
