// Light, dark, or the system's: this browser's own choice, like the Mac app's appearance setting.

import { effect, signal } from "@preact/signals";
import { load, save } from "./prefs.ts";

export type Appearance = "system" | "light" | "dark";

export const appearance = signal<Appearance>(load<Appearance>("appearance", "system"));

export function chooseAppearance(a: Appearance) {
  appearance.value = a;
  save("appearance", a);
}

effect(() => {
  const a = appearance.value;
  if (a === "system") document.documentElement.removeAttribute("data-theme");
  else document.documentElement.setAttribute("data-theme", a);
});

// The height a phone shows right now (`--vvh`): a phone's keyboard covers the bottom of the page
// without changing `100dvh`, so a page that fills the screen (a conversation) uses this instead,
// and its composer stays above the keyboard.
const vv = window.visualViewport;
if (vv) {
  const fit = () => document.documentElement.style.setProperty("--vvh", `${Math.round(vv.height)}px`);
  vv.addEventListener("resize", fit);
  fit();
}
