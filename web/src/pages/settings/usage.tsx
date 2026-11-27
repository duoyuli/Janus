// Settings › 用量账单: a period's usage across every run as a receipt, every line worded by the
// service (`GET /api/usage/receipt`) in the person's time zone (Mac: UI/Settings/UsageBillView.swift
// and `UsageReceipt`). The last period picked comes back next time.

import { useEffect, useState } from "preact/hooks";
import { api, type Json } from "../../lib/api.ts";
import { serverLang, t } from "../../lib/i18n.ts";
import { load, save } from "../../lib/prefs.ts";
import { show } from "../../lib/store.ts";
import { useDrawn } from "../../lib/testing.ts";
import { Button, Card, Note, Spinner } from "../../ui/kit.tsx";
import { Segmented } from "../../ui/form.tsx";

type Period = "week" | "days30" | "thisMonth" | "lastMonth";

export function UsagePane() {
  const [period, setPeriod] = useState<Period>(load<Period>("usagePeriod", "days30"));
  const [r, setR] = useState<Json>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);
  const read = async () => {
    const offset = -new Date().getTimezoneOffset();
    try {
      setR((await api.get(`/api/usage/receipt?period=${period}&offset=${offset}&lang=${serverLang()}`)).receipt);
      setFailure(null);
    } catch (e) {
      setFailure((e as Error).message);
    }
  };
  useEffect(() => void read(), [period]);
  useDrawn("usage", () => ({ period, headline: r?.headline ?? null, groups: (r?.groups ?? []).map((g: Json) => g.name), totals: r?.totals ?? [] }));
  const sync = async () => {
    setSyncing(true);
    try {
      await api.post("/api/prices/sync");
      await read();
    } catch (e) {
      show(t`同步价格失败：${(e as Error).message}`);
    }
    setSyncing(false);
  };
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(r.text);
      show(t`已复制用量账单`);
    } catch (e) {
      show((e as Error).message);
    }
  };
  const periods: [Period, string][] = [
    ["week", t`近 7 天`],
    ["days30", t`近 30 天`],
    ["thisMonth", t`本月`],
    ["lastMonth", t`上月`],
  ];
  return (
    <div class="settings-pane">
      <h2 class="display title2">{t`用量账单`}</h2>
      <div class="secondary">{t`按时间段汇总各 Agent 上报的 Token。金额按 models.dev API 价估算（不是账单），价格每天同步。`}</div>
      <div class="wrap-row">
        <Segmented
          label={t`区间`}
          value={period}
          options={periods}
          onChange={(p) => {
            setPeriod(p);
            save("usagePeriod", p);
          }}
        />
        <span class="grow" />
        <Button disabled={syncing} title={t`立即从 models.dev 下载最新的模型价格`} onClick={() => void sync()}>
          {syncing ? t`正在同步价格…` : t`同步价格`}
        </Button>
        {r && <Button onClick={() => void copy()}>{t`复制账单`}</Button>}
      </div>
      {failure ? <Note text={t`读取用量失败：${failure}`} tone="red" /> : r ? <Receipt r={r} /> : <Spinner />}
    </div>
  );
}

function Row({ line }: { line: Json }) {
  return (
    <div class="receipt-row">
      <span class="secondary">{line.label}</span>
      <span class="num">{line.value}</span>
    </div>
  );
}

function Amount({ a, bold }: { a: Json; bold: boolean }) {
  return (
    <span class="num amount" title={a.help}>
      {a.estimate ? (
        <>
          <span class="caption">{a.tokens}</span> <span class={bold ? "strong" : "caption"}>{a.estimate}</span>
        </>
      ) : (
        <span class={bold ? "strong" : "caption"}>{a.tokens}</span>
      )}
    </span>
  );
}

function Receipt({ r }: { r: Json }) {
  return (
    <Card pad={22} class="receipt">
      <div class="display title3 center">{r.title}</div>
      {r.meta.map((l: Json) => (
        <Row line={l} />
      ))}
      <hr />
      <div class="center">
        <div class="caption caps">{r.headlineCaption}</div>
        <div class="headline num" title={r.headlineHelp}>
          {r.headline}
        </div>
        <div class="caption">{r.headlineDetail}</div>
      </div>
      <hr />
      {r.stats.map((l: Json) => (
        <Row line={l} />
      ))}
      {r.groups.length > 0 && (
        <>
          <hr />
          <div class="receipt-row caption caps">
            <span>{r.groupsTitle}</span>
            <span>{r.amountTitle}</span>
          </div>
          {r.groups.map((g: Json) => (
            <div class="receipt-group">
              <div class="receipt-row">
                <span>
                  <strong>{g.name}</strong> {g.cost && <span class="caption">{g.cost}</span>}
                </span>
                <Amount a={g.amount} bold />
              </div>
              {g.models.map((m: Json) => (
                <div class="receipt-row model">
                  <span class="mono caption ellipsis">• {m.model}</span>
                  <Amount a={m.amount} bold={false} />
                </div>
              ))}
            </div>
          ))}
        </>
      )}
      <hr />
      {r.totals.map((l: Json) => (
        <Row line={l} />
      ))}
      {r.notes.length > 0 && (
        <div class="caption notes">
          {r.notes.map((n: string) => (
            <div>{n}</div>
          ))}
        </div>
      )}
    </Card>
  );
}
