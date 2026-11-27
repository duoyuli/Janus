// What the pages draw, for the end-to-end tests: `window.__janus.state()` returns each shown
// page's data as it is drawn, which the tests compare item by item with what the service returns,
// as the Mac app's `state` command does (docs/architecture/20261126-web.md#state).

import { useEffect, useRef } from "preact/hooks";
import { lang } from "./i18n.ts";
import { route } from "./router.ts";
import { connected } from "./stream.ts";

const pages = new Map<string, () => unknown>();

/** Report what this page draws under `name` while it is shown. `draw` is read when a test asks. */
export function useDrawn(name: string, draw: () => unknown) {
  const latest = useRef(draw);
  latest.current = draw;
  useEffect(() => {
    const read = () => latest.current();
    pages.set(name, read);
    return () => {
      if (pages.get(name) === read) pages.delete(name);
    };
  }, [name]);
}

window.__janus = {
  state: () => ({
    route: route.value,
    lang: lang.value,
    connected: connected.value,
    width: window.innerWidth,
    pages: Object.fromEntries([...pages].map(([k, f]) => [k, f()])),
  }),
};
