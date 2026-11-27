// The global inbox (Mac: UI/InboxView.swift): what every run waits on the person for, as the
// service words it (`GET /api/inbox`). Each entry is dealt with now (open the run, or a quick
// answer right here), later (put off, only when nothing waits on it) or ignored; what was put off
// and the runs at work are folded away. See docs/architecture/20261031-frontend.md#inbox.

import { useSignal } from "@preact/signals";
import { useEffect, useState } from "preact/hooks";
import type { Json } from "../lib/api.ts";
import { ago, dateTime } from "../lib/format.ts";
import { t } from "../lib/i18n.ts";
import { go, link } from "../lib/router.ts";
import type { Tone } from "../lib/status.ts";
import { ignore, inbox, inboxAct, snooze, wake, working, workspace } from "../lib/store.ts";
import { useDrawn } from "../lib/testing.ts";
import { usePageTitle } from "../lib/title.ts";
import { Icon } from "../ui/Icon.tsx";
import { Button, Card, Disclosure, IconButton, Menu, Pill, SectionHeader, Spinner } from "../ui/kit.tsx";
import "./inbox.css";

/** The glyph of an open item's kind (Mac: `TimelineMark.openKinds`). */
export const OPEN_KIND_SYMBOL: Record<string, string> = {
  approval: "hand.raised.fill",
  input: "square.and.pencil",
  question: "questionmark.bubble.fill",
  session: "text.bubble.fill",
  lead_stall: "hourglass",
  loop_touched: "doc.badge.ellipsis",
  loop_failed: "exclamationmark.triangle",
  conflict: "arrow.triangle.merge",
  overstep: "arrow.right.to.line",
  trace: "doc.badge.ellipsis",
  stall: "hourglass",
  blocker: "exclamationmark.octagon.fill",
  lead_question: "questionmark.circle.fill",
  failed: "xmark.octagon.fill",
  blocked: "pause.circle.fill",
};

/** What kind of thing an entry is, at a glance: red holds an agent up, the rest can wait. */
export function badge(e: Json): [string, Tone] {
  switch (e.kind) {
    case "delivered":
      return ["checkmark.seal.fill", "green"];
    case "merge_blocked":
      return ["arrow.triangle.merge", "amber"];
    case "held":
      return ["hand.raised.fill", "amber"];
    case "interrupted":
      return ["pause.circle.fill", "gray"];
    case "failed":
      return ["xmark.octagon.fill", "red"];
    case "dream":
      return ["brain.head.profile", "amber"];
    case "dream_failed":
      return ["exclamationmark.triangle.fill", "amber"];
    default:
      return [OPEN_KIND_SYMBOL[e.kind] ?? "exclamationmark.circle.fill", e.canWait ? "amber" : "red"];
  }
}

function tomorrowMorning(): Date {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + 1, 9, 0, 0);
}

export function InboxPage() {
  usePageTitle(t`收件箱`);
  const open = inbox.open.value;
  const later = inbox.later.value;
  const busy = working.value;
  const showLater = useSignal(false);
  const showWorking = useSignal(false);
  // Times read 「5 分钟前」: drawn again every minute.
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 60_000);
    return () => clearInterval(id);
  }, []);

  useDrawn("inbox", () => ({
    open: open.map((e) => ({ key: e.key, run: e.run, kind: e.kind, title: e.title, text: e.text, sub: e.sub, canWait: e.canWait, actions: e.actions.map((a: Json) => a.label) })),
    later: later.map((e) => ({ key: e.key, snoozedUntil: e.snoozedUntil })),
    working: busy.map((r) => r.id),
  }));

  return (
    <div class="page">
      <SectionHeader title={t`待处理`} sub={t`所有工作区`} count={open.length || undefined} />
      {open.length === 0 ? (
        <Card>
          <div class="h2">{t`清空了，没有待处理的事。`}</div>
          <div class="secondary">{busy.length === 0 ? t`也没有在干活的任务。` : t`${busy.length} 个任务在干活，需要你拍板时会出现在这里。`}</div>
        </Card>
      ) : (
        <Card pad={4} class="rows">
          {open.map((e) => (
            <Entry e={e} now={now} />
          ))}
        </Card>
      )}

      {later.length > 0 && (
        <Disclosure title={t`稍后处理（${later.length}）`} open={showLater.value} onToggle={(v) => (showLater.value = v)}>
          <Card pad={4} class="rows">
            {later.map((e) => (
              <div class="inbox-later">
                <Summary e={e} now={now} />
                <span class="caption">{t`到 ${dateTime(e.snoozedUntil ?? 0)}`}</span>
                <Button small onClick={() => void wake(e.key)}>
                  {t`现在处理`}
                </Button>
              </div>
            ))}
          </Card>
        </Disclosure>
      )}

      <Disclosure title={t`干活中（${busy.length}）`} open={showWorking.value} onToggle={(v) => (showWorking.value = v)}>
        <Card pad={4} class="rows">
          {busy.length === 0 && <div class="row secondary">{t`没有在干活的任务`}</div>}
          {busy.map((r) => (
            <a class="row" {...link(`/runs/${encodeURIComponent(r.id)}`)}>
              <div class="grow">
                <div class="ellipsis">{r.title}</div>
                <div class="caption ellipsis">{workspace(r.workspace ?? "")?.name ?? r.cwd}</div>
              </div>
              <Pill text={r.label.text} tone={r.label.tone} />
            </a>
          ))}
        </Card>
      </Disclosure>
    </div>
  );
}

/** One entry: what it is, then its actions. The one way forward is the only worded button; later
 * and ignore are glyphs in fixed places, so every row lines up. */
function Entry({ e, now }: { e: Json; now: number }) {
  const [symbol, tone] = badge(e);
  const busy = inbox.answering.value.has(e.key);
  const actions: Json[] = e.actions ?? [];
  const runPath = `/runs/${encodeURIComponent(e.run)}`;
  return (
    <div class="inbox-entry" data-key={e.key}>
      <div class="inbox-main">
        <span class={`kind-badge fg-${tone}`}>
          <Icon name={symbol} size={16} />
        </span>
        <Summary e={e} now={now} />
        <div class="inbox-glyphs">
          {e.canWait ? (
            <Menu
              icon="clock"
              label={t`稍后`}
              items={[
                { label: t`1 小时后`, onSelect: () => void snooze(e.key, new Date(Date.now() + 3600_000)) },
                { label: t`4 小时后`, onSelect: () => void snooze(e.key, new Date(Date.now() + 4 * 3600_000)) },
                { label: t`明天上午 9 点`, onSelect: () => void snooze(e.key, tomorrowMorning()) },
              ]}
            />
          ) : (
            <span class="glyph-space" aria-hidden="true" />
          )}
          <IconButton icon="eye.slash" label={t`忽略`} onClick={() => void ignore(e.key)} />
        </div>
        {actions.length === 0 ? (
          <Button kind="primary" onClick={() => go(runPath)}>
            {t`去处理`}
          </Button>
        ) : (
          <Button onClick={() => go(runPath)} title={t`打开这个任务，看完整的请求`}>
            {t`详情`}
          </Button>
        )}
      </div>
      {actions.length > 0 && (
        <div class="inbox-quick">
          {actions.map((a) => (
            <Button kind={a.prominent ? "primary" : "bordered"} small disabled={busy} title={a.help} onClick={() => void inboxAct(e, a)}>
              {a.label}
            </Button>
          ))}
          {busy && <Spinner />}
        </div>
      )}
    </div>
  );
}

function Summary({ e, now }: { e: Json; now: number }) {
  const line = [e.workspaceName, e.runTitle, e.sub, e.ts > 0 ? ago(e.ts, now) : ""].filter(Boolean).join(" · ");
  return (
    <div class="grow inbox-summary">
      <div class="inbox-title">{e.title}</div>
      {e.text && <div class="secondary clamp2">{e.text}</div>}
      <div class="caption ellipsis">{line}</div>
    </div>
  );
}
