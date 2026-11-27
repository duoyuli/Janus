// The shown page's title and where back goes: the browser tab says it, and on a phone the top bar
// shows it with a back button when the page sits under another one.

import { signal } from "@preact/signals";
import type { ComponentChildren } from "preact";
import { useEffect } from "preact/hooks";

export const pageTitle = signal("");
/** The page a phone's back button opens; none on the pages of the tab bar. */
export const backTo = signal<string | null>(null);
/** What the page puts at the right of a phone's top bar (its 「…」 menu); none by default. */
export const pageActions = signal<ComponentChildren>(null);

/** Put `node` at the right of a phone's top bar while the page is shown. */
export function usePageActions(node: ComponentChildren) {
  useEffect(() => {
    pageActions.value = node;
  });
  useEffect(() => () => {
    pageActions.value = null;
  }, []);
}

export function usePageTitle(title: string, back: string | null = null) {
  useEffect(() => {
    pageTitle.value = title;
    backTo.value = back;
    document.title = title ? `${title} · Janus` : "Janus";
  }, [title, back]);
}
