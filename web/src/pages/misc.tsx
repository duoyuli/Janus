// The smaller pages of the first web version: a run's page while it is being built (a Loop's and
// a consolidation's have their own), this browser's settings, and a path that names nothing.

import { useSignal } from "@preact/signals";
import { useEffect } from "preact/hooks";
import { api, type Json } from "../lib/api.ts";
import { chooseAppearance, appearance, type Appearance } from "../lib/appearance.ts";
import { choice, choose, serverLang, t, type Choice } from "../lib/i18n.ts";
import { meta } from "../lib/store.ts";
import { onNotice } from "../lib/stream.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { Card, Note, Pill, SectionHeader } from "../ui/kit.tsx";
import { PhoneAccess } from "./pair.tsx";
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

function Choices<T extends string>({ name, value, options, onChange }: { name: string; value: T; options: [T, string][]; onChange: (v: T) => void }) {
  return (
    <div class="choices" role="radiogroup" aria-label={name}>
      {options.map(([v, label]) => (
        <button type="button" role="radio" aria-checked={v === value} class={"choice" + (v === value ? " on" : "")} onClick={() => onChange(v)}>
          {label}
        </button>
      ))}
    </div>
  );
}

export function SettingsPage() {
  usePageTitle(t`设置`);
  useDrawn("settings", () => ({ language: choice.value, appearance: appearance.value }));
  const languages: [Choice, string][] = [
    ["system", t`跟随系统`],
    ["zh-Hans", "简体中文"],
    ["en", "English"],
  ];
  const appearances: [Appearance, string][] = [
    ["system", t`跟随系统`],
    ["light", t`浅色`],
    ["dark", t`深色`],
  ];
  return (
    <div class="page">
      <SectionHeader title={t`设置`} />
      <Card>
        <div class="setting">
          <span class="h2">{t`语言`}</span>
          <Choices name={t`语言`} value={choice.value} options={languages} onChange={choose} />
        </div>
        <div class="setting">
          <span class="h2">{t`外观`}</span>
          <Choices name={t`外观`} value={appearance.value} options={appearances} onChange={chooseAppearance} />
        </div>
      </Card>
      <PhoneAccess />
      {meta.value?.version && <div class="caption">{t`Janus 服务 ${meta.value.version}`}</div>}
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
