// One conversation of a run, the web counterpart of the Mac app's `ChatCard`
// (UI/Run/Session.swift, ChatScroll.swift, SessionParts.swift): its plan, its rows in turns as the
// service laid them out, what the session is writing right now, the messages on their way, the
// requests waiting above the composer, the composer with its suggestions, controls and usage.
// Everything shown comes from the run's view (`sessions[key]`, `lines`, `cards`); this only
// applies the person's clicks and holds their draft (docs/architecture/20261126-web.md#session).

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import { api, query, type Json } from "../lib/api.ts";
import { confirm } from "../lib/confirm.ts";
import { show } from "../lib/store.ts";
import { serverLang, t } from "../lib/i18n.ts";
import { sends, symbol, touch } from "../lib/sendkey.ts";
import { useDrawn } from "../lib/testing.ts";
import { AddImages, ImageStrip, MAX_IMAGES, readImages, Segmented } from "../ui/form.tsx";
import { Button, Pill } from "../ui/kit.tsx";
import { Icon } from "../ui/Icon.tsx";
import { ActivityRow, ChatItem, EditsCard, LiveRow, TextRow, TurnHead } from "./Items.tsx";
import { density, type RunModel, setDensity, target } from "./model.ts";
import { RequestCard } from "./Requests.tsx";
import { gapPx, type Row, rows as layRows } from "./rows.ts";
import "./session.css";

/** How many rows are drawn at first; scrolling to the top draws this many more. */
const PAGE = 80;

export function SessionPanel({ run, sessionKey, title }: { run: RunModel; sessionKey: string; title: string }) {
  const v = run.view.value;
  const info: Json = v?.sessions?.[sessionKey] ?? {};
  const mode: Json = info.mode ?? { state: "", tone: "gray", buttons: [], placeholder: "", keys: false, enabled: false };
  const live = !!v?.live;
  const shown = useMemo(
    () => layRows(info.rows ?? [], (s) => run.event(s), run.toggles.value[sessionKey] ?? {}),
    [info.rows, run.events.value, run.toggles.value, sessionKey],
  );
  const cards: Json[] = (info.requests ?? []).map((k: string) => (v?.cards ?? []).find((c: Json) => c.key === k)).filter(Boolean);
  const docked = live;
  const preview: Json | undefined = run.preview.value[sessionKey];
  useDrawn(`session:${sessionKey}`, () => ({
    key: sessionKey,
    state: mode.state,
    placeholder: mode.placeholder,
    buttons: (mode.buttons ?? []).map((b: Json) => b.label),
    rows: shown.map((r) => ({ id: r.id, kind: r.kind })),
    requests: cards.map((c) => c.key),
    density: density.value,
  }));
  const placeholder = mode.keys && !touch() ? t`${mode.placeholder} · ⌘V 贴截图 · ${symbol()} 发送` : mode.placeholder;
  return (
    <section class="session" aria-label={title}>
      <header class="session-head">
        <span class="session-title">{title}</span>
        <span class="grow" />
        {/* Only while its turn runs (`busy`): a turn that ended waits for the person, it is not quiet. */}
        {info.busy && <QuietNote last={lastTs(run, info)} />}
        <span class="density" title={t`详细：每次工具调用都显示；摘要：只看对话、请求和结果`}>
          <Segmented
            label={t`显示`}
            value={density.value}
            options={[
              ["detail", t`详细`],
              ["summary", t`摘要`],
            ]}
            onChange={(d) => {
              if (density.value === d) return;
              setDensity(d);
              // A new level applies to every turn: what was opened or folded by hand gives way to it.
              run.toggles.value = {};
              void run.loadView();
            }}
          />
        </span>
        <Pill text={mode.state} tone={mode.tone} />
      </header>
      <PlanStrip steps={info.plan ?? []} />
      <ChatList run={run} sessionKey={sessionKey} rows={shown} docked={docked} empty={!(info.events ?? []).length} preview={preview} />
      <PendingTray pending={info.pending ?? []} />
      <footer class="session-foot">
        {docked && cards.length > 0 && (
          <div class="docked">
            {cards.length > 1 && <div class="caption">{t`1 / ${cards.length} 个请求`}</div>}
            <RequestCard run={run} card={cards[0]} />
          </div>
        )}
        <Composer run={run} sessionKey={sessionKey} mode={mode} placeholder={placeholder} />
        <div class="session-tools">
          {live && (info.open || info.converse) && <Controls run={run} sessionKey={sessionKey} controls={info.controls ?? []} />}
          {info.usage && <ContextMeter usage={info.usage} />}
        </div>
      </footer>
    </section>
  );
}

function lastTs(run: RunModel, info: Json): number {
  const seq = (info.events ?? []).at(-1);
  return seq ? (run.event(seq)?.ts ?? 0) : 0;
}

/** 「N 秒没有新输出」: while an agent works on its own, how long since it did something visible. */
function QuietNote({ last }: { last: number }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 5000);
    return () => clearInterval(timer);
  }, []);
  const s = Math.floor((now - last) / 1000);
  if (!last || s < 20) return null;
  return (
    <span class="caption" title={t`Agent 在深度思考或写大段内容，暂时没有新输出`}>
      {t`${s < 120 ? t`${s} 秒` : t`${Math.floor(s / 60)} 分钟`}没有新输出`}
    </span>
  );
}

/** The agent's plan: a checklist above the conversation, folded to one line once every step is done. */
function PlanStrip({ steps }: { steps: Json[] }) {
  const done = steps.filter((s) => s.status === "completed").length;
  const [open, setOpen] = useState<boolean | null>(null);
  if (!steps.length) return null;
  const isOpen = open ?? done < steps.length;
  const now = steps.find((s) => s.status === "in_progress");
  return (
    <div class="plan">
      <button type="button" class="fold-row" aria-expanded={isOpen} onClick={() => setOpen(!isOpen)}>
        <span class={"chevron" + (isOpen ? " open" : "")} aria-hidden="true">
          <Icon name="chevron.right" size={10} />
        </span>
        <span class="caption strong">{t`计划`}</span>
        <span class="caption">
          {done}/{steps.length}
        </span>
        {!isOpen && now && <span class="caption ellipsis">{now.content}</span>}
      </button>
      {isOpen &&
        steps.map((s) => (
          <div class={"plan-step " + s.status}>
            <span aria-label={s.status === "completed" ? t`已完成` : s.status === "in_progress" ? t`进行中` : t`未开始`}>
              {s.status === "completed" ? "✓" : s.status === "in_progress" ? "◌" : "○"}
            </span>
            <span>{s.content}</span>
          </div>
        ))}
    </div>
  );
}

/** The conversation itself, newest at the bottom. It follows what arrives while the person is at
 * the bottom; scrolled up, it stays put and offers the way back. The newest rows are drawn; older
 * ones come as the person scrolls up to them. */
function ChatList({ run, sessionKey, rows, docked, empty, preview }: { run: RunModel; sessionKey: string; rows: Row[]; docked: boolean; empty: boolean; preview: Json | undefined }) {
  const box = useRef<HTMLDivElement>(null);
  const inner = useRef<HTMLDivElement>(null);
  const [count, setCount] = useState(PAGE);
  const [following, setFollowing] = useState(true);
  const [unseen, setUnseen] = useState(false);
  // The same as `following`, for the observer below, which outlives renders.
  const follow = useRef(true);
  // When the view last scrolled itself: the scroll events right after are not the person's.
  const ownScroll = useRef(0);
  const last = rows.at(-1)?.id ?? 0;
  const drawn = rows.slice(Math.max(0, rows.length - count));
  const stick = () => {
    const el = box.current;
    if (!el) return;
    ownScroll.current = performance.now();
    el.scrollTop = el.scrollHeight;
  };
  const toEnd = () => {
    follow.current = true;
    stick();
    setFollowing(true);
    setUnseen(false);
  };
  // Opening a conversation lands on its newest row; so does sending a message.
  useLayoutEffect(toEnd, [sessionKey, run.sent.value]);
  useLayoutEffect(() => {
    if (follow.current) stick();
    else setUnseen(true);
  }, [last, rows.length, preview?.text, preview?.thought]);
  // Rows grow after they are drawn (Markdown, images, a turn opening, the view coming back after a
  // send): while the person is at the bottom, the bottom stays in view.
  useEffect(() => {
    const el = inner.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => {
      if (follow.current) stick();
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const onScroll = () => {
    const el = box.current;
    if (!el) return;
    const atEnd = el.scrollHeight - el.scrollTop - el.clientHeight <= 32;
    // The view's own scrolling, and the content moving under it, is not the person leaving the end.
    if (follow.current && performance.now() - ownScroll.current < 250) {
      if (!atEnd) stick();
      return;
    }
    follow.current = atEnd;
    setFollowing(atEnd);
    if (atEnd) setUnseen(false);
    // Near the top: draw the older rows, keeping the place.
    if (el.scrollTop < 80 && count < rows.length) {
      const before = el.scrollHeight;
      setCount(count + PAGE);
      requestAnimationFrame(() => {
        if (box.current) box.current.scrollTop += box.current.scrollHeight - before;
      });
    }
  };
  return (
    <div class="chat-scroll-wrap">
      <div class="chat-scroll" ref={box} onScroll={onScroll}>
        <div ref={inner}>
          {empty && <div class="secondary">{t`开跑后，对话和工具调用会实时出现在这里`}</div>}
          {drawn.map((r, i) => (
            <div class="chat-row" data-row={r.id} data-kind={r.kind} style={{ paddingTop: `${i === 0 && drawn.length === rows.length ? 0 : gapPx(r.gap)}px` }}>
              <RowView run={run} sessionKey={sessionKey} row={r} docked={docked} />
            </div>
          ))}
          {preview && <LiveRow preview={preview} />}
        </div>
      </div>
      {!following && (
        <button type="button" class={"to-newest" + (unseen ? " unseen" : "")} onClick={toEnd} title={t`回到最新的消息，之后的新消息会继续显示在底部`}>
          ↓ {unseen ? t`有新消息` : t`回到最新`}
        </button>
      )}
    </div>
  );
}

function RowView({ run, sessionKey, row, docked }: { run: RunModel; sessionKey: string; row: Row; docked: boolean }) {
  switch (row.kind) {
    case "section":
      return (
        <div class="chat-section-block">
          <ChatItem run={run} e={row.event} docked={docked} />
          {row.injected.map((e) => (
            <ChatItem run={run} e={e} docked={docked} />
          ))}
        </div>
      );
    case "item":
      return <ChatItem run={run} e={row.event} docked={docked} />;
    case "head":
      return <TurnHead run={run} row={row} onToggle={() => run.toggle(sessionKey, row.id, !(row.open ?? false))} />;
    case "activity":
      return <ActivityRow run={run} row={row} onToggle={() => run.toggle(sessionKey, row.id, !(row.open ?? true))} />;
    case "text":
      return <TextRow events={row.events} reply={row.reply} />;
    case "edits":
      return <EditsCard run={run} files={row.files} />;
  }
}

/** Messages sent to this session that have not reached the agent yet. */
function PendingTray({ pending }: { pending: Json[] }) {
  if (!pending.length) return null;
  const tone = (d: string) => (d === "pending" || d === "interrupt" ? "blue" : d === "queued" ? "amber" : "gray");
  return (
    <div class="pending">
      <div class="caption strong">{t`待发送 ${pending.length}`}</div>
      {pending.map((m) => (
        <div class="pending-row">
          <Pill text={m.label} tone={tone(m.delivery)} />
          <span class="clamp2">{m.text}</span>
        </div>
      ))}
    </div>
  );
}

/** The controls the agent offers for its session (model, effort, mode …), switched while it runs. */
function Controls({ run, sessionKey, controls }: { run: RunModel; sessionKey: string; controls: Json[] }) {
  return (
    <div class="controls">
      {controls.map((c) => (
        <label class="control" title={t`切换这个会话的${c.name}，立即生效`}>
          <span class="caption">{c.label}</span>
          <select
            aria-label={c.name}
            value={c.current}
            onChange={(e) => {
              const value = (e.target as HTMLSelectElement).value;
              if (value !== c.current) void run.act({ type: "control", ...target(sessionKey), id: c.id, value, via: c.via });
            }}
          >
            {c.options.map((o: Json) => (
              <option value={o.value}>{o.name}</option>
            ))}
          </select>
        </label>
      ))}
    </div>
  );
}

/** This session's context: how full it is as a ring and a number, or the last turn's tokens. */
function ContextMeter({ usage }: { usage: Json }) {
  const p: number | undefined = usage.percent ?? undefined;
  const r = 5;
  const c = 2 * Math.PI * r;
  return (
    <span class="meter caption" title={usage.help} aria-label={usage.labels}>
      {p !== undefined && (
        <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
          <circle cx="6" cy="6" r={r} class="meter-track" />
          <circle cx="6" cy="6" r={r} class={"meter-fill" + (p >= 80 ? " high" : "")} stroke-dasharray={`${(c * p) / 100} ${c}`} transform="rotate(-90 6 6)" />
        </svg>
      )}
      {usage.meter}
    </span>
  );
}

/** The draft, its images and suggestions, and the session's buttons. */
function Composer({ run, sessionKey, mode, placeholder }: { run: RunModel; sessionKey: string; mode: Json; placeholder: string }) {
  const draft = run.drafts.value[sessionKey] ?? "";
  const images = run.images.value[sessionKey] ?? [];
  const [suggest, setSuggest] = useState<{ draft: string; rows: Json[]; ghost: string | null } | null>(null);
  const [index, setIndex] = useState(0);
  const [closedAt, setClosedAt] = useState<string | null>(null);
  const field = useRef<HTMLTextAreaElement>(null);
  const enabled = !!mode.enabled;

  // On a phone the field is one line that grows with its text (up to its CSS max-height).
  useLayoutEffect(() => {
    const el = field.current;
    if (!el || !matchMedia("(max-width: 640px)").matches) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight + 2}px`;
  }, [draft]);

  // Only a command (`/` at the start) or a file (`@` starting the last word) asks the service, a
  // moment after the person stops typing.
  useEffect(() => {
    if (!enabled || !(draft.startsWith("/") || /(^|\s)@\S*$/.test(draft))) {
      setSuggest(null);
      return;
    }
    const timer = setTimeout(() => {
      api
        .get(`${run.path}/suggest${query({ key: sessionKey, draft, lang: serverLang() })}`)
        .then((s) => {
          setSuggest({ draft, rows: s.rows ?? [], ghost: s.ghost ?? null });
          setIndex(0);
        })
        .catch(() => {});
    }, 120);
    return () => clearTimeout(timer);
  }, [draft, sessionKey, enabled]);

  const rows = suggest && suggest.draft === draft ? suggest.rows : [];
  const open = rows.length > 0 && closedAt !== draft;
  const ghost = suggest && suggest.draft === draft ? suggest.ghost : null;
  const set = (text: string) => run.setDraft(sessionKey, text);

  const send = async (act: string) => {
    const to = target(sessionKey);
    switch (act) {
      case "message": {
        const text = draft.trim();
        if (!text && !images.length) return;
        run.setDraft(sessionKey, "");
        run.setImages(sessionKey, []);
        run.sent.value++;
        const body: Json = { type: "message", ...to, text };
        if (images.length) body.images = images.map(({ name, mimeType, data }) => ({ name, mimeType, data }));
        // A message that did not go out comes back, unless another one was started meanwhile.
        if (!(await run.act(body)) && !(run.drafts.value[sessionKey] ?? "") && !(run.images.value[sessionKey] ?? []).length) {
          run.setDraft(sessionKey, text);
          run.setImages(sessionKey, images);
        }
        return;
      }
      case "finish":
        if (!(await confirm({ title: t`用当前产出完成这个子任务？`, text: t`子任务里已有的改动会照常记录成 diff。`, ok: t`完成`, destructive: false }))) return;
        void run.act({ type: "finish", ...to });
        return;
      case "resumeRun":
        void run.act({ type: "resumeRun" });
        return;
      default:
        void run.act({ type: act, ...to });
    }
  };

  const onKey = (e: KeyboardEvent) => {
    if (open) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        setIndex((index + (e.key === "ArrowDown" ? 1 : rows.length - 1)) % rows.length);
        return;
      }
      if (e.key === "Tab" || (e.key === "Enter" && !e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey)) {
        e.preventDefault();
        set(rows[Math.min(index, rows.length - 1)].completion);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setClosedAt(draft);
        return;
      }
    }
    if (sends(e)) {
      e.preventDefault();
      if (mode.buttons?.some((b: Json) => b.act === "message")) void send("message");
    }
  };

  const addFiles = async (files: Iterable<File>) => {
    const { images: read, refused } = await readImages(files);
    if (refused.length) show(t`图片太大，没有加上：${refused.join(", ")}`);
    if (read.length) run.setImages(sessionKey, [...(run.images.value[sessionKey] ?? []), ...read].slice(0, MAX_IMAGES));
  };

  return (
    <div class="composer">
      {enabled && open && (
        <div class="suggestions" role="listbox">
          {rows.map((r, i) => (
            <>
              {r.group && <div class="suggest-group caption strong">{r.group}</div>}
              <button type="button" role="option" aria-selected={i === index} class={"suggest-row" + (i === index ? " on" : "")} onClick={() => set(r.completion)}>
                <span class="mono">{r.label}</span>
                <span class="caption ellipsis">{r.detail}</span>
              </button>
            </>
          ))}
        </div>
      )}
      <ImageStrip images={images} onChange={(list) => run.setImages(sessionKey, list)} />
      {/* A wide window: the field, then a line with the image button and the buttons. A phone: one
          line, the image button, the field growing with its text, the buttons (session.css). */}
      <div class="composer-line">
      <div class="composer-attach">{enabled && <AddImages onPick={(files) => void addFiles(files)} />}</div>
      <div class="composer-box">
        <textarea
          ref={field}
          class="composer-field"
          rows={matchMedia("(max-width: 640px)").matches ? 1 : 2}
          placeholder={placeholder}
          disabled={!enabled}
          value={draft}
          onInput={(e) => {
            const text = (e.target as HTMLTextAreaElement).value;
            set(text);
            if (closedAt !== null && closedAt !== text) setClosedAt(null);
          }}
          onKeyDown={onKey}
          onPaste={(e) => {
            const files = [...(e.clipboardData?.files ?? [])];
            if (files.some((f) => f.type.startsWith("image/"))) {
              e.preventDefault();
              void addFiles(files);
            }
          }}
        />
        {ghost && <div class="ghost caption">{ghost}</div>}
      </div>
      <div class="composer-buttons">
        {(mode.buttons ?? []).map((b: Json) => (
          <Button
            kind={b.primary ? "primary" : "bordered"}
            danger={b.danger}
            disabled={b.primary && !draft.trim() && !images.length}
            title={b.primary && !draft.trim() && !images.length ? t`写点什么，或者贴张图` : ""}
            onClick={() => void send(b.act)}
          >
            {b.label}
          </Button>
        ))}
      </div>
      </div>
    </div>
  );
}
