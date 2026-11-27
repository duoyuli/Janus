// What the web client draws, against what the service returns, at phone and desktop width; and
// what is clicked in it reaching the service. The runs come from e2e/web.py (`JANUS_WEB_DATA`).
// See docs/operations/20261031-testing.md#web_e2e.

import { expect, test, type APIRequestContext, type Page, type TestInfo } from "@playwright/test";
import { join } from "node:path";
import { readFileSync } from "node:fs";
import { parseStrings } from "../src/lib/strings.ts";

const data = JSON.parse(process.env.JANUS_WEB_DATA ?? "{}") as { workspace: string; asked: string; later: string; delivered: string };
const shots = join(process.env.JANUS_WEB_REPORT ?? "/tmp", "shots");
const english = parseStrings(readFileSync(new URL("../../frontend/Sources/Janus/en.lproj/Localizable.strings", import.meta.url), "utf8"));

type Drawn = { route: { page: string }; lang: string; connected: boolean; pages: Record<string, any> };

async function drawn(page: Page): Promise<Drawn> {
  return page.evaluate(() => window.__janus!.state() as unknown as Drawn);
}

/** What the service answers, asked the way a program on this machine asks (no browser headers). */
async function service(request: APIRequestContext, path: string): Promise<any> {
  const res = await request.get(path);
  expect(res.ok(), `${path}: ${res.status()}`).toBeTruthy();
  return res.json();
}

async function shot(page: Page, info: TestInfo, name: string) {
  await page.screenshot({ path: join(shots, `${name}-${info.project.name}.png`), fullPage: true });
}

// Every test has a browser context of its own: it starts in the browser's language (Chinese) and appearance.
const phone = (info: TestInfo) => info.project.name === "phone";

test("the inbox draws what the service has, entry by entry", async ({ page, request }, info) => {
  await page.goto("/inbox");
  const server = await service(request, "/api/inbox?lang=zh");
  await expect.poll(async () => (await drawn(page)).pages.inbox?.open.map((e: any) => e.key)).toEqual(server.open.map((e: any) => e.key));
  const st = await drawn(page);
  expect(st.connected).toBe(true);
  for (const [i, e] of server.open.entries()) {
    const d = st.pages.inbox.open[i];
    expect(d).toEqual({ key: e.key, run: e.run, kind: e.kind, title: e.title, text: e.text, sub: e.sub, canWait: e.canWait, actions: e.actions.map((a: any) => a.label) });
  }
  for (const run of [data.asked, data.later, data.delivered]) expect(st.pages.inbox.open.some((e: any) => e.run === run), run).toBe(true);
  // What the page shows is what it says it draws: each entry's title, and the quick answers.
  for (const e of server.open) await expect(page.locator(`[data-key="${e.key}"] .inbox-title`)).toHaveText(e.title);
  const asked = server.open.find((e: any) => e.run === data.asked && e.kind === "question");
  for (const a of asked.actions) await expect(page.locator(`[data-key="${asked.key}"] .inbox-quick button`, { hasText: a.label })).toBeVisible();
  await shot(page, info, "inbox");
  if (phone(info)) {
    await page.emulateMedia({ colorScheme: "dark" });
    await shot(page, info, "inbox-dark");
    await page.emulateMedia({ colorScheme: "light" });
  }
});

test("a phone has the tab bar and a wide window the sidebar", async ({ page }, info) => {
  await page.goto("/inbox");
  await expect(page.locator(".inbox-entry").first()).toBeVisible();
  if (phone(info)) {
    await expect(page.locator(".sidebar")).toBeHidden();
    await expect(page.locator(".tabbar")).toBeVisible();
    // Everything tapped in the tab bar is at least 44 points high.
    for (const box of await page.locator(".tabbar .tab").evaluateAll((els) => els.map((e) => e.getBoundingClientRect().height))) expect(box).toBeGreaterThanOrEqual(44);
    await page.locator(".tabbar .tab", { hasText: "团队" }).click();
    await expect.poll(async () => (await drawn(page)).route.page).toBe("workspaces");
    await shot(page, info, "workspaces");
    await page.locator(`.row[href="/workspaces/${data.workspace}"]`).click();
    await expect.poll(async () => (await drawn(page)).route.page).toBe("workspace");
    // A page under another one has a back button in the top bar.
    await expect(page.locator(".topbar [aria-label='返回']")).toBeVisible();
    await page.locator(".tabbar .tab", { hasText: "对话" }).click();
    await expect.poll(async () => (await drawn(page)).route.page).toBe("chats");
  } else {
    await expect(page.locator(".sidebar")).toBeVisible();
    await expect(page.locator(".tabbar")).toBeHidden();
    await page.locator(`.sidebar-item[href="/workspaces/${data.workspace}"]`).click();
    await expect.poll(async () => (await drawn(page)).route.page).toBe("workspace");
  }
});

test("the workspace page lists what waits on the person and its task records", async ({ page, request }, info) => {
  await page.goto(`/workspaces/${data.workspace}`);
  const inbox = await service(request, "/api/inbox?lang=zh");
  const records = await service(request, `/api/runs?workspace=${data.workspace}&limit=50`);
  const waiting = [...new Set(inbox.open.filter((e: any) => e.workspace === data.workspace).map((e: any) => e.run))];
  await expect.poll(async () => (await drawn(page)).pages.workspace?.waiting.map((w: any) => w.run)).toEqual(waiting);
  const st = (await drawn(page)).pages.workspace;
  expect(st.records.map((r: any) => r.id)).toEqual(records.runs.map((r: any) => r.id).filter((id: string) => !waiting.includes(id)));
  for (const w of st.waiting) expect(w.entries).toEqual(inbox.open.filter((e: any) => e.run === w.run).map((e: any) => e.key));
  await shot(page, info, "workspace");
});

test("the interface and what the service words follow the language", async ({ page, request }, info) => {
  await page.goto("/settings");
  await page.getByRole("radio", { name: "English" }).click();
  await expect.poll(async () => (await drawn(page)).lang).toBe("en");
  await shot(page, info, "settings-en");
  await page.goto("/inbox");
  const server = await service(request, "/api/inbox?lang=en");
  await expect.poll(async () => (await drawn(page)).pages.inbox?.open.map((e: any) => e.title)).toEqual(server.open.map((e: any) => e.title));
  if (!phone(info)) await expect(page.locator(".section-header h1")).toHaveText(english["待处理"]!);
  await expect(page.locator(".inbox-entry").first().locator(".btn").last()).toHaveText(new RegExp(`${english["去处理"]}|${english["详情"]}`));
  await shot(page, info, "inbox-en");
});

test("a quick answer, putting off and ignoring reach the service", async ({ page, request }, info) => {
  test.skip(phone(info), "the desktop answers; the phone only looks, so both see the same runs");
  await page.goto("/inbox");
  const before = await service(request, "/api/inbox?lang=zh");
  const asked = before.open.find((e: any) => e.run === data.asked && e.kind === "question");
  const later = before.open.find((e: any) => e.run === data.later && e.kind === "question");
  const delivered = before.open.find((e: any) => e.run === data.delivered && e.kind === "delivered");
  expect(later.canWait && delivered.canWait).toBe(true);

  // The lead's question, answered right in the inbox.
  await page.locator(`[data-key="${asked.key}"] .inbox-quick button`, { hasText: "测试" }).click();
  await expect
    .poll(async () => (await service(request, `/api/runs/${data.asked}`)).events.some((e: any) => e.type === "question.answered"))
    .toBe(true);
  await expect.poll(async () => (await drawn(page)).pages.inbox.open.some((e: any) => e.key === asked.key)).toBe(false);

  // Put off for an hour: on the service's 「稍后」 list, and folded away here.
  await page.locator(`[data-key="${later.key}"] [aria-label="稍后"]`).click();
  await page.getByRole("menuitem", { name: "1 小时后" }).click();
  await expect.poll(async () => (await service(request, "/api/inbox?lang=zh")).later.map((e: any) => e.key)).toContain(later.key);
  await expect.poll(async () => (await drawn(page)).pages.inbox.later.map((e: any) => e.key)).toContain(later.key);

  // Ignored: gone from the service's inbox and from the page.
  await page.locator(`[data-key="${delivered.key}"] [aria-label="忽略"]`).click();
  await expect.poll(async () => (await service(request, "/api/inbox?lang=zh")).open.map((e: any) => e.key)).not.toContain(delivered.key);
  await expect.poll(async () => (await drawn(page)).pages.inbox.open.map((e: any) => e.key)).not.toContain(delivered.key);
  await page.getByRole("button", { name: /稍后处理/ }).click();
  await shot(page, info, "inbox-after");
});
