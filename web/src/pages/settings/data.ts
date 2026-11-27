// What the settings pages read beyond the setup objects of `store.ts`: the ACP adapters, how each
// digital employee's subtasks went, the resource library and its picks. Each is read when a page
// that shows it opens and again when the global stream says it changed (`adapters`, `library`,
// `connection`), the way the Mac app's `AppStore` reads them.

import { signal } from "@preact/signals";
import { useEffect } from "preact/hooks";
import { api, type Json } from "../../lib/api.ts";
import { serverLang } from "../../lib/i18n.ts";
import { show } from "../../lib/store.ts";
import { onNotice } from "../../lib/stream.ts";
import { load, save } from "../../lib/prefs.ts";

export const adapters = signal<Json[]>([]);
export const roleStats = signal<Json>(null);
export const library = signal<Json>(null);
export const picks = signal<Json>(null);
/** The automatic updates the person already saw on the settings page (`line.autoUpdated.key`). */
export const autoUpdateSeen = signal<Set<string>>(new Set(load<string[]>("autoUpdateSeen", [])));

export async function loadAdapters() {
  try {
    adapters.value = (await api.get(`/api/adapters?lang=${serverLang()}`)).adapters ?? [];
  } catch {
    // Kept as shown.
  }
}

export async function loadRoleStats() {
  roleStats.value = await api.get(`/api/stats/roles?lang=${serverLang()}`).catch(() => null);
}

export async function loadLibrary() {
  try {
    library.value = await api.get(`/api/library?lang=${serverLang()}`);
    picks.value = await api.get(`/api/library/picks?lang=${serverLang()}`);
  } catch (e) {
    show((e as Error).message);
  }
}

export async function adapterAction(id: string, action: "install" | "use", version?: string) {
  try {
    adapters.value = (await api.post(`/api/adapters/${encodeURIComponent(id)}/${action}?lang=${serverLang()}`, version ? { version } : {})).adapters ?? [];
  } catch (e) {
    show((e as Error).message);
  }
}

/** An automatic update shown once is not announced again (Mac: `acknowledgeAutoUpdate`). */
export function acknowledge(a: Json) {
  const key = a?.line?.autoUpdated?.key;
  if (!key || autoUpdateSeen.value.has(key)) return;
  const next = new Set(autoUpdateSeen.value).add(key);
  autoUpdateSeen.value = next;
  save("autoUpdateSeen", [...next].slice(-50));
}

/** Read `read` now and whenever the global stream names one of `notices`. */
export function useFollow(read: () => void, notices: string[]) {
  useEffect(() => {
    read();
    return onNotice((name) => notices.includes(name) && read());
  }, []);
}

export async function testConnection(id: string) {
  try {
    await api.post(`/api/connections/${encodeURIComponent(id)}/test`);
  } catch (e) {
    show(`${(e as Error).message}`);
  }
}

export async function login(id: string, started: string) {
  try {
    await api.post(`/api/connections/${encodeURIComponent(id)}/login`);
    show(started);
  } catch (e) {
    show((e as Error).message);
  }
}
