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
