// The interface language: the same choice as the Mac app (follow the browser, 简体中文, English)
// and the same translations (`virtual:janus-strings`, the Mac app's en.lproj). Interface words
// are written `t\`…${x}…\`` as Swift's `L.text("…\(x)…")`, so both look up the same key; what
// the service words (inbox entries, run views) comes in the language `serverLang` names.
// See docs/architecture/20261126-web.md#i18n.

import { computed, signal } from "@preact/signals";
import english from "virtual:janus-strings";
import { type Arg, fill, keyOf } from "./strings.ts";
import { load, save } from "./prefs.ts";

export type Choice = "system" | "zh-Hans" | "en";
export type Lang = "zh" | "en";

/** What the person picked; `system` follows the browser's languages. */
export const choice = signal<Choice>(load<Choice>("language", "system"));

/** The browser's first Chinese or English language; Chinese when it lists neither. */
export function browserLang(languages: readonly string[]): Lang {
  for (const l of languages) {
    const s = l.toLowerCase();
    if (s.startsWith("en")) return "en";
    if (s.startsWith("zh")) return "zh";
  }
  return "zh";
}

/** The language the interface is drawn in now. */
export const lang = computed<Lang>(() => {
  const c = choice.value;
  if (c === "en") return "en";
  if (c === "zh-Hans") return "zh";
  return browserLang(typeof navigator === "undefined" ? [] : navigator.languages ?? [navigator.language]);
});

export function choose(c: Choice) {
  choice.value = c;
  save("language", c);
}

/** The interface text for these parts and values, in the language drawn now. */
export function t(parts: TemplateStringsArray, ...args: Arg[]): string {
  return tr(keyOf(parts, args), ...args);
}

/** The interface text of a key given whole (as the strings file has it), filled with `args`. */
export function tr(key: string, ...args: Arg[]): string {
  const format = lang.value === "en" ? (english[key] ?? key) : key;
  return fill(format, args);
}

/** `lang` as the service takes it in `?lang=`. */
export function serverLang(): Lang {
  return lang.value;
}
