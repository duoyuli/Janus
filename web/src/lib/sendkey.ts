// Which key sends what is typed in a composer, as the Mac app's `SendKey` (Core/SendKey.swift):
// ⌘↩ (↩ starts a new line), or ↩ (⇧↩ starts a new line). ⌘↩ (Ctrl+↩ off a Mac) always sends. A
// phone sends with its button only: its ↩ starts a new line.

import { signal } from "@preact/signals";
import { load, save } from "./prefs.ts";

export type SendKey = "commandReturn" | "return";

export const sendKey = signal<SendKey>(load<SendKey>("sendKey", "commandReturn"));

export function chooseSendKey(k: SendKey) {
  sendKey.value = k;
  save("sendKey", k);
}

const mac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

/** A touch screen without a keyboard to speak of: no key hints, the button sends. */
export function touch(): boolean {
  return typeof matchMedia !== "undefined" && matchMedia("(pointer: coarse)").matches;
}

/** The sending shortcut as hints write it. */
export function symbol(): string {
  return sendKey.value === "return" ? "↩" : mac ? "⌘↩" : "Ctrl+↩";
}

/** Whether this key press sends. */
export function sends(e: KeyboardEvent): boolean {
  if (e.key !== "Enter" || e.isComposing) return false;
  if (e.metaKey || e.ctrlKey) return true;
  if (touch()) return false;
  return sendKey.value === "return" && !e.shiftKey && !e.altKey;
}
