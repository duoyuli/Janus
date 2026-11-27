// The card each request waiting on the person is answered with, as the service wrote it (the
// view's `cards`, backend/src/run_view/cards.rs), the counterpart of the Mac app's
// UI/Run/Requests.swift: who asks and where, what about, the options or the form. The person's
// picks, notes and form answers stay with the run until sent
// (docs/architecture/20261031-frontend.md#request_cards).

import type { ComponentChildren } from "preact";
import type { Json } from "../lib/api.ts";
import { joinList } from "../lib/format.ts";
import { t } from "../lib/i18n.ts";
import { Switch } from "../ui/form.tsx";
import { Avatar, Button, Card, Pill } from "../ui/kit.tsx";
import { CodeBlock, InlineDiff, inWorkspace, who } from "./Items.tsx";
import type { RunModel } from "./model.ts";

const clock = (ms: number) => {
  const d = new Date(ms);
  return [d.getHours(), d.getMinutes(), d.getSeconds()].map((n) => String(n).padStart(2, "0")).join(":");
};

function Shell({ run, card, children, controls }: { run: RunModel; card: Json; children?: ComponentChildren; controls: ComponentChildren }) {
  const w = who(run, card.agent);
  return (
    <Card class={`request-card tint-${card.pill.tone}`} pad={14}>
      <div class="request-head" data-key={card.key}>
        <Pill text={card.pill.text} tone={card.pill.tone} />
        <Avatar role={w.role} size={20} />
        <span class="request-title grow">{card.title}</span>
        <span class="caption">{clock(card.ts)}</span>
      </div>
      {children}
      {(card.facts ?? []).map(([k, v]: [string, string]) => (
        <div class="fact-row">
          <span class="secondary">{k}</span>
          <span>{v}</span>
        </div>
      ))}
      {controls}
    </Card>
  );
}

function Choice({ title, sub, on, multi, accent, onClick, disabled }: { title: string; sub?: string; on: boolean; multi?: boolean; accent?: boolean; onClick: () => void; disabled?: boolean }) {
  return (
    <button type="button" class={"answer" + (on ? " on" : "")} aria-pressed={on} onClick={onClick} disabled={disabled}>
      <span class="answer-mark" aria-hidden="true">
        {multi ? (on ? "☑" : "☐") : on ? "◉" : "○"}
      </span>
      <span class="answer-text">
        <span>{title}</span>
        {sub && <span class={"caption" + (accent ? " accent" : "")}>{sub}</span>}
      </span>
    </button>
  );
}

export function RequestCard({ run, card }: { run: RunModel; card: Json }) {
  switch (card.kind) {
    case "question":
    case "decision":
      return <QuestionAsk run={run} card={card} />;
    case "approval":
      return <ApprovalAsk run={run} card={card} />;
    case "link":
    case "form":
      return <InputAsk run={run} card={card} />;
    default:
      return <SessionAsk run={run} card={card} />;
  }
}

/** The request's task and agent, as the run's state has them. */
function request(run: RunModel, card: Json): Json {
  return (run.view.value?.inbox ?? []).find((r: Json) => r.seq === card.seq) ?? {};
}

function QuestionAsk({ run, card }: { run: RunModel; card: Json }) {
  const can = !!run.view.value?.live;
  const decision = card.kind === "decision";
  const options: string[] = card.options ?? [];
  const picked = run.picks.value[card.key];
  const note = run.notes.value[card.key] ?? "";
  const empty = !picked && (decision ? !note.trim() : true);
  const submit = () => {
    let n = note.trim();
    let answer = picked ?? "";
    // A free-form decision (no options) is answered with the text itself.
    if (decision && !answer && n) [answer, n] = [n, ""];
    if (!answer) return;
    void run.act({ type: "answer", key: card.key, task: request(run, card).task ?? "", answer, note: n });
  };
  return (
    <Shell
      run={run}
      card={card}
      controls={
        <>
          {options.length > 0 && (
            <div class="answers">
              {options.map((o) => (
                <Choice
                  title={o}
                  sub={o === card.recommend ? t`负责人建议` : ""}
                  accent
                  on={picked === o}
                  disabled={!can}
                  onClick={() => (run.picks.value = { ...run.picks.value, [card.key]: picked === o ? undefined : o })}
                />
              ))}
            </div>
          )}
          <textarea
            class="text-field"
            rows={2}
            disabled={!can}
            placeholder={decision ? (options.length ? t`补充说明（可选）` : t`你的回答`) : t`补充说明（可选），会和回答一起发给 Agent`}
            value={note}
            onInput={(e) => (run.notes.value = { ...run.notes.value, [card.key]: (e.target as HTMLTextAreaElement).value })}
          />
          <div class="row-buttons">
            <Button
              kind="primary"
              disabled={!can || empty}
              title={empty ? (!decision ? t`先选择一个选项` : options.length ? t`先选择一个选项，或写下你的回答` : t`先写下你的回答`) : ""}
              onClick={submit}
            >
              {decision ? t`提交` : t`提交回答`}
            </Button>
          </div>
          {card.footnote && <div class="caption">{card.footnote}</div>}
        </>
      }
    >
      {card.text && <div class="request-text">{card.text}</div>}
      {card.reason && <div class="secondary">{card.reason}</div>}
    </Shell>
  );
}

function ApprovalAsk({ run, card }: { run: RunModel; card: Json }) {
  const can = !!run.view.value?.live;
  const cwd = run.view.value?.run?.cwd ?? "";
  const diffs: Json[] = run.view.value?.diffs?.[card.seq] ?? [];
  return (
    <Shell
      run={run}
      card={card}
      controls={
        <>
          <div class="row-buttons">
            {(card.actions ?? []).map((o: Json) => (
              <Button kind={o.prominent ? "primary" : "bordered"} disabled={!can} title={o.help} onClick={() => void run.act(o.body)}>
                {o.label}
              </Button>
            ))}
          </div>
          {card.impact && <div class="caption">{card.impact}</div>}
          {card.always && <div class="caption">{card.always}</div>}
        </>
      }
    >
      {card.command && <CodeBlock text={card.command} />}
      {diffs.map((x) => (
        <InlineDiff path={inWorkspace(x.path, cwd)} lines={x.lines} />
      ))}
    </Shell>
  );
}

function InputAsk({ run, card }: { run: RunModel; card: Json }) {
  const can = !!run.view.value?.live;
  const filled = run.forms.value[card.key] ?? {};
  // What the person filled in, without the fields left empty.
  const content = Object.fromEntries(
    Object.entries(filled).filter(([, v]) => !(v === null || v === undefined || v === "" || (Array.isArray(v) && v.length === 0))),
  );
  const missing = (card.fields ?? []).filter((f: Json) => f.required && content[f.key] === undefined).map((f: Json) => f.title);
  const send = (action: string) => {
    if (action === "accept" && missing.length) return;
    const r = request(run, card);
    void run.act({ type: "input", key: card.key, task: r.task ?? "", agent: r.agent ?? "", action, content });
  };
  if (card.kind === "link") {
    return (
      <Shell
        run={run}
        card={card}
        controls={
          <div class="row-buttons">
            <Button disabled={!can} onClick={() => send("decline")}>
              {t`拒绝`}
            </Button>
            <Button kind="primary" disabled={!can} onClick={() => send("accept")}>
              {t`我已完成`}
            </Button>
          </div>
        }
      >
        <div>{card.text}</div>
        {card.url && (
          <a class="mono" href={card.url} target="_blank" rel="noreferrer noopener">
            {card.url}
          </a>
        )}
      </Shell>
    );
  }
  return (
    <Shell
      run={run}
      card={card}
      controls={
        <>
          <div class="row-buttons">
            <Button disabled={!can} title={t`告诉 Agent 取消这次请求`} onClick={() => send("cancel")}>
              {t`取消请求`}
            </Button>
            <Button disabled={!can} title={t`跳过，让 Agent 接着干`} onClick={() => send("decline")}>
              {t`跳过`}
            </Button>
            <Button kind="primary" disabled={!can || missing.length > 0} onClick={() => send("accept")}>
              {t`提交`}
            </Button>
          </div>
          <div class={"caption" + (missing.length ? " fg-amber" : "")}>{missing.length ? t`还有必填项：${joinList(missing)}` : card.footnote}</div>
        </>
      }
    >
      <div>{card.text}</div>
      <SchemaForm run={run} card={card} disabled={!can} />
    </Shell>
  );
}

/** A form's fields as the service read them from its JSON Schema. */
function SchemaForm({ run, card, disabled }: { run: RunModel; card: Json; disabled: boolean }) {
  const key = card.key;
  const values = run.forms.value[key] ?? {};
  const set = (k: string, v: Json) => (run.forms.value = { ...run.forms.value, [key]: { ...(run.forms.value[key] ?? {}), [k]: v } });
  const same = (a: Json, b: Json) => JSON.stringify(a) === JSON.stringify(b);
  return (
    <div class="schema-form">
      {(card.fields ?? []).map((f: Json) => {
        const cur = values[f.key];
        let field: ComponentChildren;
        switch (f.kind) {
          case "choice":
          case "multi": {
            const multi = f.kind === "multi";
            field = (
              <div class="answers">
                {f.choices.map((o: Json) => {
                  const on = multi ? (Array.isArray(cur) ? cur : []).some((x: Json) => same(x, o.value)) : same(cur, o.value);
                  return (
                    <Choice
                      title={o.label}
                      sub={o.desc}
                      on={on}
                      multi={multi}
                      disabled={disabled}
                      onClick={() => {
                        if (multi) {
                          const list: Json[] = Array.isArray(cur) ? cur : [];
                          set(f.key, on ? list.filter((x) => !same(x, o.value)) : [...list, o.value]);
                        } else set(f.key, on ? null : o.value);
                      }}
                    />
                  );
                })}
              </div>
            );
            break;
          }
          case "bool":
            field = <Switch on={!!cur} disabled={disabled} label={f.title} onChange={(on) => set(f.key, on)} />;
            break;
          case "number":
            field = (
              <input
                class="text-field narrow"
                inputMode="decimal"
                disabled={disabled}
                aria-label={f.title}
                value={cur ?? ""}
                onInput={(e) => {
                  const n = Number((e.target as HTMLInputElement).value);
                  set(f.key, (e.target as HTMLInputElement).value === "" || Number.isNaN(n) ? null : n);
                }}
              />
            );
            break;
          case "line":
            field = <input class="text-field" disabled={disabled} aria-label={f.title} value={cur ?? ""} onInput={(e) => set(f.key, (e.target as HTMLInputElement).value)} />;
            break;
          default:
            field = <textarea class="text-field" rows={3} disabled={disabled} aria-label={f.title} value={cur ?? ""} onInput={(e) => set(f.key, (e.target as HTMLTextAreaElement).value)} />;
        }
        return (
          <div class="form-field">
            <div class="strong">
              {f.title}
              {f.required && <span class="secondary"> *</span>}
            </div>
            {f.description && <div class="caption">{f.description}</div>}
            {field}
            {f.custom && (
              <input
                class="text-field"
                disabled={disabled}
                placeholder={f.custom.placeholder}
                value={values[f.custom.key] ?? ""}
                onInput={(e) => set(f.custom.key, (e.target as HTMLInputElement).value)}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

function SessionAsk({ run, card }: { run: RunModel; card: Json }) {
  const open = run.openSession;
  return (
    <Shell
      run={run}
      card={card}
      controls={
        open && card.session ? (
          <div class="row-buttons">
            <Button kind="primary" onClick={() => open(card.session)}>
              {request(run, card).task ? t`打开会话` : t`打开对话`}
            </Button>
          </div>
        ) : null
      }
    >
      {card.text && <div class="secondary">{card.text}</div>}
    </Shell>
  );
}

/** Every request of the run waiting on the person, oldest first, under 「待处理」 (the Mac app's
 * `InboxSection`); nothing when nothing waits. */
export function RequestList({ run }: { run: RunModel }) {
  const cards: Json[] = run.view.value?.cards ?? [];
  if (!cards.length) return null;
  return (
    <section class="request-list">
      <div class="request-list-head">
        <span class="h2">{t`待处理`}</span>
        <span class="count">{cards.length}</span>
      </div>
      {cards.map((c) => (
        <RequestCard run={run} card={c} />
      ))}
    </section>
  );
}
