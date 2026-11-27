// The shown page's title and where back goes: the browser tab says it, and on a phone the top bar
// shows it with a back button when the page sits under another one.

import { signal } from "@preact/signals";
import { useEffect } from "preact/hooks";

export const pageTitle = signal("");
/** The page a phone's back button opens; none on the pages of the tab bar. */
export const backTo = signal<string | null>(null);

export function usePageTitle(title: string, back: string | null = null) {
  useEffect(() => {
    pageTitle.value = title;
    backTo.value = back;
    document.title = title ? `${title} · Janus` : "Janus";
  }, [title, back]);
}
