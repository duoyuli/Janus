// A goal's page on the web, against the service's view of it: the workbench (run card, delivery,
// inspector, team), the audit log, the team page, a subtask, a diff and a document; an action the
// service does not allow says why, and accepting reaches the service. Phone and desktop width.
// See docs/operations/20261031-testing.md#web_e2e.

import { expect, type Page, test, type TestInfo } from "@playwright/test";
import { data, drawn, service, shot } from "./helpers.ts";

const phone = (info: TestInfo) => info.project.name === "phone";

test("the workbench draws the service's view of a delivered goal", async ({ page, request }, info) => {
  const id = data.reviewed;
  const view = await service(request, `/api/runs/${id}/view?lang=zh`);
  await page.goto(`/runs/${id}`);
  await expect.poll(async () => (await drawn(page)).pages.runBoard?.review?.stage).toBe(view.review.stage);
  const b = (await drawn(page)).pages.runBoard;
  expect(b.capsule).toEqual(view.capsule);
  expect(b.title).toBe(view.title);
  expect(b.review.signals).toEqual(view.review.signals.map((s: any) => s.text));
  expect(b.review.facts).toBe(view.delivery.facts);
  expect(b.rows).toEqual(view.delivery.tasks.map((r: any) => ({ task: r.task, metrics: r.metrics, markers: r.markers })));
  expect(b.inspector).toEqual({
    tasks: view.tasks.map((x: any) => x.id),
    diffs: view.page.inspector.diffs.map((a: any) => a.path),
    files: view.page.inspector.files.map((a: any) => a.path),
    delivered: view.page.inspector.delivered.map((d: any) => d.path),
  });
  expect(b.members).toEqual(view.members.map((m: any) => ({ id: m.id, phase: m.phase, line: m.line })));
  expect(b.actions).toEqual(view.actions);
  // The run card's line: the subtasks' progress, then the ledger's items as the service words them.
  const usage = await service(request, `/api/runs/${id}/usage?lang=zh`);
  expect(b.summary).toBe([view.page.progress, ...usage.words.card].filter(Boolean).join(" · "));
  await expect(page.locator("[data-summary]")).toHaveText(b.summary);
  await expect(page.locator("[data-facts]")).toHaveText(view.delivery.facts);
  await expect(page.locator(".run-card .pill").first()).toHaveText(view.capsule.text);
  await shot(page, info, "run-board");
  if (phone(info)) expect(await fits(page)).toEqual([]);
});

/** What on the run page is wider than a phone's screen (tables, code and the timeline scroll inside). */
async function fits(page: Page): Promise<string[]> {
  return page.evaluate(() =>
    [...document.querySelectorAll(".run-page *")]
      .filter((e) => e.getBoundingClientRect().right > window.innerWidth + 1 && !e.closest(".timeline-scroll, .code-block, .md-table, .run-views, .audit-line, .inline-diff, .diff-code"))
      .map((e) => `${e.tagName.toLowerCase()}.${e.className}`),
  );
}

test("the audit log, the team, a subtask, a diff and a document follow the service", async ({ page, request }, info) => {
  const id = data.reviewed;
  const view = await service(request, `/api/runs/${id}/view?lang=zh`);
  await page.goto(`/runs/${id}/audit`);
  await expect.poll(async () => (await drawn(page)).pages.audit?.rules).toEqual(view.audit.map((r: any) => ({ text: r.text, ok: r.ok })));
  const events = (await service(request, `/api/runs/${id}`)).events;
  expect((await drawn(page)).pages.audit.events).toBe(events.length);
  await shot(page, info, "run-audit");

  await page.goto(`/runs/${id}/team`);
  await expect.poll(async () => (await drawn(page)).pages.team?.workspace).toBe(view.page.team.workspace);
  await shot(page, info, "run-team");

  const task = view.tasks[0].id;
  await page.goto(`/runs/${id}/tasks/${task}`);
  await expect.poll(async () => (await drawn(page)).pages.task?.trace).toEqual(view.page.tasks[task].trace);
  const tp = (await drawn(page)).pages.task;
  expect({ criteria: tp.criteria, passed: tp.passed }).toEqual({ criteria: view.evidence[task].criteria, passed: view.evidence[task].passed });
  expect(tp.downstream).toEqual(view.page.tasks[task].downstream);
  if (phone(info)) expect(await fits(page)).toEqual([]);
  await shot(page, info, "run-task");

  const diff = view.page.inspector.diffs[0];
  expect(diff, "the delivered goal has a diff to review").toBeTruthy();
  const served = await service(request, `/api/runs/${id}/diff?path=${encodeURIComponent(diff.path)}`);
  await page.goto(`/runs/${id}/diff/${encodeURIComponent(diff.path)}`);
  await expect.poll(async () => (await drawn(page)).pages.diff?.files?.map((f: any) => f.path)).toEqual(served.files.map((f: any) => f.path));
  await shot(page, info, "run-diff");

  const doc = view.docs[0];
  expect(doc, "the delivered goal has a document").toBeTruthy();
  const text = await (await request.get(`/api/runs/${id}/doc?path=${encodeURIComponent(doc.path)}`)).text();
  await page.goto(`/runs/${id}/doc/${encodeURIComponent(doc.path)}`);
  await expect.poll(async () => (await drawn(page)).pages.doc?.length).toBe(text.length);
  await shot(page, info, "run-doc");
});

test("what the service does not allow says why, and accepting reaches it", async ({ page, request }, info) => {
  // A goal still at work cannot be deleted: the button says what the service says.
  const busy = await service(request, `/api/runs/${data.asked}/view?lang=zh`);
  expect(busy.actions.delete.ok).toBe(false);
  await page.goto(`/runs/${data.asked}`);
  const del = page.locator(".run-head-tools [aria-label='删除任务']");
  await expect(del).toHaveAttribute("aria-disabled", "true");
  await expect(del).toHaveAttribute("title", busy.actions.delete.why);
  // Its question waits on the person on the workbench too, with the service's answers.
  await expect.poll(async () => (await drawn(page)).pages.runBoard?.requests?.length ?? 0).toBeGreaterThan(0);
  if (phone(info)) return;
  const id = data.reviewed;
  await page.goto(`/runs/${id}`);
  await page.locator("[data-action='accept']").click();
  await expect.poll(async () => (await service(request, `/api/runs/${id}/view?lang=zh`)).review.stage).toBe("accepted");
  await expect.poll(async () => (await drawn(page)).pages.runBoard?.review?.stage).toBe("accepted");
  const view = await service(request, `/api/runs/${id}/view?lang=zh`);
  expect((await drawn(page)).pages.runBoard.review.line).toBe(view.page.reviewLine);
  await shot(page, info, "run-accepted");
});
