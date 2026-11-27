// Asking before something that cannot be undone (Mac: `confirm` in UI/Components.swift): the
// question waits here and the frame draws it as a dialog in the page (`ui/ConfirmDialog.tsx`),
// a sheet from the bottom on a phone; the answer comes back as a promise. With `dontAskAgain`,
// the dialog offers 「不再提醒」, kept in this browser like the Mac keeps it on the Mac.

import { signal } from "@preact/signals";
import { tr } from "./i18n.ts";
import { load, save } from "./prefs.ts";

export type Question = {
  title: string;
  text?: string;
  /** The button that goes ahead, naming what it does (「删除」, 「停止」). */
  ok?: string;
  /** Goes ahead with something that cannot be undone: the button is red. Default true. */
  destructive?: boolean;
  /** A key under which 「不再提醒」 is remembered; once set, the answer is yes without asking. */
  dontAskAgain?: string;
};

export type Asking = Question & { answer: (yes: boolean, dontAskAgain: boolean) => void };

/** The question on screen, if any. */
export const asking = signal<Asking | null>(null);

/** Ask, and resolve with the person's answer. A second question waits for the first to be answered. */
export async function confirm(q: Question): Promise<boolean> {
  if (q.dontAskAgain && load(q.dontAskAgain, false)) return true;
  while (asking.value) await new Promise<void>((done) => setTimeout(done, 50));
  return new Promise<boolean>((resolve) => {
    asking.value = {
      ...q,
      answer: (yes, dontAskAgain) => {
        asking.value = null;
        if (yes && dontAskAgain && q.dontAskAgain) save(q.dontAskAgain, true);
        resolve(yes);
      },
    };
  });
}

/** The one warning before a goal and everything Janus kept for it go away (Mac: `confirmDeleteRun`). */
export function confirmDeleteRun(name: string): Promise<boolean> {
  return confirm({
    title: tr("删除任务？"),
    text: tr("%@的记录、子任务、产物和日志都会删除，找不回来。没提交的改动先存成补丁；你的目录和已合并的结果不受影响。", name),
    ok: tr("删除"),
    dontAskAgain: "deleteRunNoConfirm",
  });
}

/** Before a sheet closes with edits that were not saved (Mac: `confirmDiscard`). */
export function confirmDiscard(): Promise<boolean> {
  return confirm({ title: tr("放弃未保存的更改？"), text: tr("关闭后，这次的修改不会保存。"), ok: tr("放弃更改") });
}

/** Close a sheet: right away when nothing changed, else after the person agrees to discard. */
export async function closeSheet(dirty: boolean, close: () => void) {
  if (!dirty || (await confirmDiscard())) close();
}
