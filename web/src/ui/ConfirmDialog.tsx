// The dialog for `lib/confirm.ts`: the question, what follows, and two buttons; the one that goes
// ahead names its action and is red when it cannot be undone (Mac: NSAlert with a destructive
// button). Centered on a wide screen, a sheet from the bottom on a phone. Esc or a click outside
// cancels; focus starts on 取消 for a destructive question, as on the Mac, and stays in the dialog.

import { useEffect, useRef, useState } from "preact/hooks";
import { asking } from "../lib/confirm.ts";
import { t } from "../lib/i18n.ts";

export function ConfirmDialog() {
  const q = asking.value;
  const [dontAsk, setDontAsk] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const ok = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!q) return;
    setDontAsk(false);
    const before = document.activeElement as HTMLElement | null;
    ((q.destructive ?? true) ? cancel.current : ok.current)?.focus();
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        q.answer(false, false);
      } else if (e.key === "Tab") {
        // Keep focus in the dialog.
        const all = Array.from(box.current?.querySelectorAll<HTMLElement>("button, input") ?? []);
        if (!all.length) return;
        const i = all.indexOf(document.activeElement as HTMLElement);
        const next = e.shiftKey ? (i <= 0 ? all.length - 1 : i - 1) : i === all.length - 1 ? 0 : i + 1;
        e.preventDefault();
        all[next]?.focus();
      }
    };
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("keydown", key);
      before?.focus?.();
    };
  }, [q]);
  if (!q) return null;
  const destructive = q.destructive ?? true;
  return (
    <div class="confirm-backdrop" onPointerDown={(e) => e.target === e.currentTarget && q.answer(false, false)}>
      <div class="confirm-box" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" aria-describedby={q.text ? "confirm-text" : undefined} ref={box}>
        <div id="confirm-title" class="confirm-title">
          {q.title}
        </div>
        {q.text && (
          <div id="confirm-text" class="confirm-text">
            {q.text}
          </div>
        )}
        {q.dontAskAgain && (
          <label class="check-row confirm-dont-ask">
            <input type="checkbox" checked={dontAsk} onChange={(e) => setDontAsk((e.target as HTMLInputElement).checked)} />
            <span>{t`不再提醒`}</span>
          </label>
        )}
        <div class="confirm-buttons">
          <button type="button" class="btn btn-bordered" ref={cancel} onClick={() => q.answer(false, false)}>
            {t`取消`}
          </button>
          <button type="button" class={"btn " + (destructive ? "btn-destructive" : "btn-primary")} ref={ok} onClick={() => q.answer(true, dontAsk)}>
            {q.ok ?? t`继续`}
          </button>
        </div>
      </div>
    </div>
  );
}
