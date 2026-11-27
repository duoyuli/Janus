// How times and text are written, as the Mac app's `L` (Core/Format.swift) writes them: the same
// date patterns from the shared translations, the same 「今天 20:16」/「Today at 20:16」. Only how
// a value is written, never what it is: that comes from the service.

import { lang, t, tr } from "./i18n.ts";

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const pad = (n: number) => String(n).padStart(2, "0");

/** A date in a pattern of `yyyy`, `MMM`, `M`, `d`, `HH`, `mm`, `ss` (the patterns the strings file translates). */
export function pattern(p: string, d: Date): string {
  return p.replace(/yyyy|MMM|M|d|HH|mm|ss/g, (tok) => {
    switch (tok) {
      case "yyyy":
        return String(d.getFullYear());
      case "MMM":
        return MONTHS[d.getMonth()]!;
      case "M":
        return String(d.getMonth() + 1);
      case "d":
        return String(d.getDate());
      case "HH":
        return pad(d.getHours());
      case "mm":
        return pad(d.getMinutes());
      default:
        return pad(d.getSeconds());
    }
  });
}

const sameDay = (a: Date, b: Date) => a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();

/** 「今天 20:16」, 「昨天 …」, 「明天 …」, else the date (with the year when it is not this year). */
export function dateTime(ms: number, now = new Date()): string {
  const d = new Date(ms);
  const time = pattern(tr("HH:mm"), d);
  const day = (offset: number) => new Date(now.getFullYear(), now.getMonth(), now.getDate() + offset);
  if (sameDay(d, now)) return t`今天 ${time}`;
  if (sameDay(d, day(-1))) return t`昨天 ${time}`;
  if (sameDay(d, day(1))) return t`明天 ${time}`;
  return pattern(tr(d.getFullYear() === now.getFullYear() ? "M月d日 HH:mm" : "yyyy年M月d日 HH:mm"), d);
}

/** How long ago: 「刚刚」, 「5 分钟前」, …, then the date. */
export function ago(ts: number, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - ts) / 1000));
  if (s < 10) return t`刚刚`;
  if (s < 60) return t`${s} 秒前`;
  if (s < 3600) return t`${Math.floor(s / 60)} 分钟前`;
  if (s < 86400) return t`${Math.floor(s / 3600)} 小时前`;
  return dateTime(ts, new Date(now));
}

/** A duration: 「不到 1 秒」, 「12 秒」, 「3 分 05 秒」, 「2 小时 4 分」. */
export function dur(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (ms < 1000) return t`不到 1 秒`;
  if (s < 60) return t`${s} 秒`;
  if (s < 3600) return t`${Math.floor(s / 60)} 分 ${pad(s % 60)} 秒`;
  return t`${Math.floor(s / 3600)} 小时 ${Math.floor((s % 3600) / 60)} 分`;
}

/** A home-relative path for display. */
export function tilde(p: string): string {
  return (p ?? "").replace(/^\/(?:Users|home)\/[^/]+\//, "~/");
}

/** The list separator of the language: 「、」 or ", ". */
export function joinList(items: string[]): string {
  return items.join(lang.value === "en" ? ", " : "、");
}
