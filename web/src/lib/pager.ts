// A run list read a page at a time, newest first (`GET /api/runs`, docs/interfaces/
// 20261031-http_api.md#run_list), like the Mac app's `RunPager`: the sidebar's chats, a
// workspace's task records. `more` reads the next page; `refresh` reads again what is shown, in
// one request, so statuses stay current and new runs appear at the top.

import { signal } from "@preact/signals";
import { api, type Json } from "./api.ts";

export class RunPager {
  readonly query: string;
  readonly pageSize: number;
  readonly runs = signal<Json[]>([]);
  /** The cursor of the next page; null once the last page is in. */
  readonly next = signal<string | null>(null);
  readonly loaded = signal(false);
  private loading = false;

  constructor(query: string, pageSize = 50) {
    this.query = query;
    this.pageSize = pageSize;
  }

  get more(): boolean {
    return this.next.value !== null;
  }

  private path(limit: number, cursor: string | null): string {
    let p = `/api/runs?${this.query}&limit=${Math.min(limit, 500)}`;
    if (cursor) p += `&cursor=${encodeURIComponent(cursor)}`;
    return p;
  }

  async refresh() {
    let page: Json;
    try {
      page = await api.get(this.path(Math.max(this.pageSize, this.runs.value.length), null));
    } catch {
      return;
    }
    const fresh: Json[] = page.runs ?? [];
    let merged = fresh;
    let cursor: string | null = page.next ?? null;
    const old = this.runs.value;
    // One request reads at most 500: what was read further down stays after the refreshed part.
    const last = fresh[fresh.length - 1]?.id;
    const i = old.findIndex((r) => r.id === last);
    if (cursor !== null && i >= 0 && i + 1 < old.length) {
      const seen = new Set(fresh.map((r) => r.id));
      merged = fresh.concat(old.slice(i + 1).filter((r) => !seen.has(r.id)));
      cursor = this.next.value;
    }
    if (JSON.stringify(merged) !== JSON.stringify(old)) this.runs.value = merged;
    this.next.value = cursor;
    this.loaded.value = true;
  }

  async loadMore() {
    const cursor = this.next.value;
    if (cursor === null || this.loading) return;
    this.loading = true;
    try {
      const page = await api.get(this.path(this.pageSize, cursor));
      // A refresh in the meantime moved on; this page belongs to the old list.
      if (cursor !== this.next.value) return;
      const known = new Set(this.runs.value.map((r) => r.id));
      this.runs.value = this.runs.value.concat((page.runs ?? []).filter((r: Json) => !known.has(r.id)));
      this.next.value = page.next ?? null;
    } catch {
      // Kept as shown; the next scroll to the end asks again.
    } finally {
      this.loading = false;
    }
  }
}
