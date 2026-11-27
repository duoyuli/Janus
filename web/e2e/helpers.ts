// What the web client's end-to-end tests share: the runs e2e/web.py made, the page's drawn state,
// the service read as a program on this Mac reads it, and pictures.

import { expect, type APIRequestContext, type Page, type TestInfo } from "@playwright/test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { parseStrings } from "../src/lib/strings.ts";

export const data = JSON.parse(process.env.JANUS_WEB_DATA ?? "{}") as { workspace: string; asked: string; later: string; delivered: string; chat: string; goal: string; reviewed: string };
const shots = join(process.env.JANUS_WEB_REPORT ?? "/tmp", "shots");
export const english = parseStrings(readFileSync(new URL("../../frontend/Sources/Janus/en.lproj/Localizable.strings", import.meta.url), "utf8"));

export type Drawn = { route: { page: string }; lang: string; connected: boolean; pages: Record<string, any> };

export async function drawn(page: Page): Promise<Drawn> {
  return page.evaluate(() => window.__janus!.state() as unknown as Drawn);
}

/** What the service answers, asked the way a program on this Mac asks: Playwright's own requests
 * carry no `Origin` or `Sec-Fetch-Site`, so the service lets them in as it lets the Mac app in. */
export async function service(request: APIRequestContext, path: string, method: "GET" | "POST" = "GET"): Promise<any> {
  const res = method === "GET" ? await request.get(path) : await request.post(path, { data: {} });
  expect(res.ok(), `${method} ${path}: ${res.status()} ${await res.text()}`).toBeTruthy();
  return res.json();
}

/** What a request the page itself makes gets: its status and text (the page's cookie, if any, goes
 * along). `body` is sent as JSON with `method` (POST when there is a body). */
export async function fromPage(page: Page, path: string, body?: unknown, method?: string): Promise<{ status: number; text: string }> {
  return page.evaluate(
    async ([p, b, m]) => {
      const init: RequestInit = b === undefined ? { method: m ?? "GET" } : { method: m ?? "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(b) };
      const r = await fetch(p, init);
      return { status: r.status, text: await r.text() };
    },
    [path, body, method] as const,
  );
}

export async function shot(page: Page, info: TestInfo, name: string) {
  await page.screenshot({ path: join(shots, `${name}-${info.project.name}.png`), fullPage: true });
}
