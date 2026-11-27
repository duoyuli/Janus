// The web client at a phone's width, against what went wrong there (2026-11-27): no page wider than
// the screen, the inbox's summary keeping its line, bars that hide what scrolls under them, menus
// inside the window, a conversation that fills the screen and stays at its newest line.
// See docs/operations/20261031-testing.md#web_e2e.

import { expect, test, type Page } from "@playwright/test";
import { data, shot } from "./helpers.ts";

/** How far the page reaches past the window, and the outermost elements that do. */
async function overflow(page: Page) {
  return page.evaluate(() => {
    const vw = document.documentElement.clientWidth;
    const all = [...document.querySelectorAll("body *")].filter((e) => {
      const r = e.getBoundingClientRect();
      // A row that scrolls sideways on purpose (views, categories, code, a diff) may hold more.
      return r.width > 0 && r.right > vw + 1 && !e.closest(".segmented, pre, .code-block, .diff-code");
    });
    return { over: document.documentElement.scrollWidth - vw, roots: all.filter((e) => !all.includes(e.parentElement!)).slice(0, 4).map((e) => `${e.tagName}.${e.className}`) };
  });
}

/** Go somewhere the way a person does inside the app (a full load is the pairing's own test). */
async function go(page: Page, path: string) {
  await page.evaluate((p) => {
    history.pushState({}, "", p);
    dispatchEvent(new PopStateEvent("popstate"));
  }, path);
  await page.waitForTimeout(800);
}

test.beforeEach(async ({ page }) => {
  await page.goto("/inbox");
  await expect(page.locator(".inbox-entry").first()).toBeVisible();
});

test("no page is wider than the phone", async ({ page }, info) => {
  const pages = [
    "/inbox", "/workspaces", `/workspaces/${data.workspace}`, `/workspaces/${data.workspace}/settings`, "/workspaces/new",
    `/workspaces/${data.workspace}/memory`, "/chats", "/chats/new", `/runs/${data.chat}`, `/runs/${data.goal}`,
    `/runs/${data.reviewed}`, `/runs/${data.reviewed}/audit`, `/runs/${data.reviewed}/team`, `/runs/${data.goal}/tasks/T1`,
    "/settings", "/settings/general", "/settings/agents", "/settings/library", "/settings/usage", "/settings/devices",
  ];
  for (const p of pages) {
    await go(page, p);
    const o = await overflow(page);
    expect(o, `${p} is ${o.over}px wider than the phone: ${o.roots.join(", ")}`).toEqual({ over: 0, roots: [] });
  }
  // An editor with a long path under a field (the Agent's folder in Janus home).
  await go(page, "/settings/agents");
  const editor = await page.locator('a[href*="/settings/agents/agent/"]').first().getAttribute("href");
  await go(page, editor!);
  expect((await overflow(page)).over).toBe(0);
  await shot(page, info, "mobile-agent-editor");
});

test("an inbox entry keeps its summary a line of its own, and the bars are opaque", async ({ page }, info) => {
  // The summary takes the row; its buttons go under it, not beside it squeezing the title.
  const width = await page.locator(".inbox-summary").first().evaluate((e) => e.getBoundingClientRect().width);
  expect(width).toBeGreaterThan(250);
  for (const bar of [".topbar", ".tabbar"]) {
    const bg = await page.locator(bar).evaluate((e) => getComputedStyle(e).backgroundColor);
    expect(bg, `${bar} lets the page show through`).not.toMatch(/rgba\([^)]*,\s*0?\.\d+\)/);
  }
  // The 「稍后」 menu opens inside the window.
  const later = page.locator(".inbox-entry .menu button[aria-haspopup]").first();
  await later.click();
  const box = await page.locator(".menu-list").boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(390);
  await shot(page, info, "mobile-inbox-menu");
  await page.keyboard.press("Escape");
});

test("a conversation fills the screen above the tabs and stays at its newest line", async ({ page }, info) => {
  await go(page, `/runs/${data.chat}`);
  await expect(page.locator(".chat-row").first()).toBeVisible();
  const foot = await page.locator(".session-foot").boundingBox();
  const tabs = await page.locator(".tabbar").boundingBox();
  // The composer sits right above the tabs: nothing under it, no blank band.
  expect(foot!.y + foot!.height).toBeLessThanOrEqual(tabs!.y + 1);
  expect(tabs!.y - (foot!.y + foot!.height)).toBeLessThan(24);
  const atEnd = () => page.locator(".chat-scroll").evaluate((e) => e.scrollHeight - e.scrollTop - e.clientHeight);
  await expect.poll(atEnd).toBeLessThanOrEqual(2);
  await page.locator(".composer-field").fill("手机上再发一条");
  await page.locator(".composer-buttons .btn-primary").click();
  await expect(page.locator(".chat-text", { hasText: "回复：手机上再发一条" })).toBeVisible({ timeout: 30_000 });
  // The reply came back and the view came back with it: still at the newest line.
  await expect.poll(atEnd).toBeLessThanOrEqual(2);
  await shot(page, info, "mobile-chat-followed");
});
