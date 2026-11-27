// A conversation in the web client against the run's view the service serves: the rows in turns
// at the display level chosen, each event in the words the service wrote, the composer, the
// suggestions, and the requests answered from it. The phone looks and talks; the desktop answers.
// See docs/operations/20261031-testing.md#web_e2e.

import { expect, test, type APIRequestContext, type Page, type TestInfo } from "@playwright/test";
import { data, drawn, service, shot } from "./helpers.ts";

const phone = (info: TestInfo) => info.project.name === "phone";

/** The rows the service laid out, as shown with no clicks: a folded turn leaves out what folds with it. */
function expected(rows: any[]): number[] {
  const folded = new Set(rows.filter((r) => r.kind === "head" && r.open === false).map((r) => r.turn));
  return rows.filter((r) => !(r.fold && folded.has(r.turn))).filter((r) => r.kind !== "activity" || (r.events ?? []).length).map((r) => r.id);
}

async function view(request: APIRequestContext, run: string, detail = false) {
  return service(request, `/api/runs/${run}/view?lang=zh${detail ? "&detail=1" : ""}`);
}

async function session(page: Page, key: string) {
  return (await drawn(page)).pages[`session:${key}`];
}

test("a chat draws its rows and lines as the service laid them out, and takes a message", async ({ page, request }, info) => {
  await page.goto(`/runs/${data.chat}`);
  const v = await view(request, data.chat);
  const lead = v.sessions["agent:lead"];
  await expect.poll(async () => (await session(page, "agent:lead"))?.rows.map((r: any) => r.id)).toEqual(expected(lead.rows));
  const st = await session(page, "agent:lead");
  expect(st.state).toBe(lead.mode.state);
  expect(st.buttons).toEqual(lead.mode.buttons.map((b: any) => b.label));
  // Each event shows in the words the service wrote: the chat's divider, the person's bubble.
  const own = new Set(lead.rows.flatMap((r: any) => [r.seq, ...(r.injected ?? [])]).filter(Boolean).map(String));
  for (const [seq, line] of Object.entries<any>(v.lines).filter(([s]) => own.has(s))) {
    if (line.kind === "divider") await expect(page.locator(`[data-seq="${seq}"] .chat-divider`)).toHaveText(line.text);
    if (line.kind === "bubbles") await expect(page.locator(`[data-seq="${seq}"] .bubble-text`).first()).toHaveText(line.items[0].text);
  }
  await expect(page.locator(".chat-text", { hasText: "回复：你好" })).toBeVisible();
  await shot(page, info, "chat");
  if (!phone(info)) return;
  // The phone sends with its button.
  await page.locator(".composer-field").fill("手机上发一条");
  await page.locator(".composer-buttons .btn-primary").click();
  await expect(page.locator(".chat-text", { hasText: "回复：手机上发一条" })).toBeVisible({ timeout: 30_000 });
  const events = (await service(request, `/api/runs/${data.chat}`)).events;
  expect(events.some((e: any) => e.type === "user.message" && e.data?.text === "手机上发一条")).toBe(true);
  await expect.poll(async () => (await session(page, "agent:lead"))?.rows.length).toBe(expected((await view(request, data.chat)).sessions["agent:lead"].rows).length);
  await shot(page, info, "chat-sent");
});

test("a task's whole session: calls in turns, the detailed level, and its lines", async ({ page, request }, info) => {
  await page.goto(`/runs/${data.goal}?session=task:T1`);
  const v = await view(request, data.goal);
  const t1 = v.sessions["task:T1"];
  await expect.poll(async () => (await session(page, "task:T1"))?.rows.map((r: any) => r.id)).toEqual(expected(t1.rows));
  expect((await session(page, "task:T1")).state).toBe(t1.mode.state);
  // The attempt's header and a tool call read as the service wrote them.
  const attempt = Object.entries<any>(v.lines).find(([s, l]) => l.kind === "attempt" && t1.events.includes(Number(s)));
  expect(attempt).toBeTruthy();
  await expect(page.locator(`[data-seq="${attempt![0]}"] .strong`).first()).toHaveText(attempt![1].title);
  // The detailed level lays every turn and group open, as the service does at that level.
  await page.locator(".session-head [role=radio]", { hasText: "详细" }).click();
  const detail = (await view(request, data.goal, true)).sessions["task:T1"];
  await expect.poll(async () => (await session(page, "task:T1"))?.rows.map((r: any) => r.id)).toEqual(expected(detail.rows));
  const calls = new Set(detail.rows.filter((r: any) => r.kind === "activity" && r.open !== false).flatMap((r: any) => r.events).map(String));
  const tool = Object.entries<any>((await view(request, data.goal, true)).lines).find(([s, l]) => l.kind === "tool" && !l.board && calls.has(s));
  if (tool) await expect(page.locator(`[data-seq="${tool[0]}"] .tool-line`).first()).toContainText(tool[1].what);
  await shot(page, info, "task-session");
  await page.locator(".session-head [role=radio]", { hasText: "摘要" }).click();
  await expect.poll(async () => (await session(page, "task:T1"))?.density).toBe("summary");
});

test("the composer suggests the workspace's files for @", async ({ page, request }) => {
  await page.goto(`/runs/${data.goal}?session=agent:lead`);
  await expect.poll(async () => (await session(page, "agent:lead"))?.state).toBeTruthy();
  const field = page.locator(".composer-field");
  await field.fill("看看 @READ");
  const served = await service(request, `/api/runs/${data.goal}/suggest?key=agent:lead&draft=${encodeURIComponent("看看 @READ")}&lang=zh`);
  expect(served.rows.length).toBeGreaterThan(0);
  await expect(page.locator(".suggest-row .mono")).toHaveText(served.rows.map((r: any) => r.label));
  await page.locator(".suggest-row").first().click();
  await expect(field).toHaveValue(served.rows[0].completion);
  await field.fill("");
});

test("an approval and a form wait above the composer and are answered there", async ({ page, request }, info) => {
  await page.goto(`/runs/${data.goal}?session=task:T2`);
  const v = await view(request, data.goal);
  const key = v.sessions["task:T2"].requests[0];
  const card = v.cards.find((c: any) => c.key === key);
  expect(card.kind).toBe("approval");
  await expect.poll(async () => (await session(page, "task:T2"))?.requests).toEqual([key]);
  const docked = page.locator(".docked .request-card");
  await expect(docked.locator(".request-title")).toHaveText(card.title);
  await expect(docked.locator(".row-buttons .btn")).toHaveText(card.actions.map((a: any) => a.label));
  await shot(page, info, "approval");

  await page.goto(`/runs/${data.goal}?session=task:T3`);
  const form = (await view(request, data.goal)).cards.find((c: any) => c.kind === "form");
  const shown = page.locator(".docked .request-card");
  await expect(shown.locator(".request-title")).toHaveText(form.title);
  await expect(shown.locator(".form-field .strong").first()).toContainText(form.fields[0].title);
  await shot(page, info, "form");
  if (phone(info)) return;

  // The desktop answers both.
  await page.goto(`/runs/${data.goal}?session=task:T2`);
  await page.locator(".docked .request-card .row-buttons .btn", { hasText: card.actions[0].label }).click();
  await expect
    .poll(async () => (await service(request, `/api/runs/${data.goal}`)).events.some((e: any) => e.type === "approval.granted" && e.task === "T2"))
    .toBe(true);
  await page.goto(`/runs/${data.goal}?session=task:T3`);
  const submit = page.locator(".docked .request-card .btn-primary");
  await expect(submit).toBeDisabled();
  await page.locator(".docked .request-card .form-field input, .docked .request-card .form-field textarea").first().fill("Ada");
  await submit.click();
  await expect
    .poll(async () => (await service(request, `/api/runs/${data.goal}`)).events.some((e: any) => e.type === "input.answered" && e.task === "T3"))
    .toBe(true);
  await expect.poll(async () => (await session(page, "task:T3"))?.requests).toEqual([]);
});

test("a page keeps one event stream, its run's events riding on it", async ({ page }) => {
  // A browser holds six connections to the host: two streams a page used them up with three pages open.
  await page.addInitScript(() => {
    const O = window.EventSource;
    const open: EventSource[] = [];
    (window as any).__openStreams = () => open.filter((e) => e.readyState !== 2).map((e) => e.url.replace(location.origin, ""));
    (window as any).EventSource = function (url: string, init?: EventSourceInit) {
      const e = new O(url, init);
      open.push(e);
      return e;
    };
    (window as any).EventSource.prototype = O.prototype;
  });
  await page.goto(`/runs/${data.chat}`);
  await expect(page.locator(".chat-text", { hasText: "回复：你好" })).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as any).__openStreams())).toEqual([expect.stringMatching(new RegExp(`^/api/stream\\?run=${data.chat}&`))]);
  // Leaving the run, the stream goes back to the lists only.
  await page.evaluate(() => { history.pushState({}, "", "/inbox"); dispatchEvent(new PopStateEvent("popstate")); });
  await expect.poll(() => page.evaluate(() => (window as any).__openStreams())).toEqual(["/api/stream"]);
});
