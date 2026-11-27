// The workspace's page, starting goals and chats, team memory, Loops and the workspace editor,
// against what the service returns, at phone and desktop width; and what is done in them reaching
// the service. The runs, notes and Loop come from e2e/web.py (`JANUS_WEB_DATA`).
// See docs/operations/20261031-testing.md#web_e2e.

import { expect, test, type TestInfo } from "@playwright/test";
import { data, drawn, service, shot } from "./helpers.ts";

const phone = (info: TestInfo) => info.project.name === "phone";
const extra = data as typeof data & { newWorkspace: string; loop: string; loopRun: string; folder: string };

test("the workspace page draws what the service says of it", async ({ page, request }, info) => {
  await page.goto(`/workspaces/${extra.workspace}`);
  const setup = await service(request, "/api/setup?lang=zh");
  const ws = setup.workspaces.find((w: any) => w.id === extra.workspace);
  const inbox = await service(request, "/api/inbox?lang=zh");
  const memory = await service(request, `/api/workspaces/${extra.workspace}/memories?lang=zh`);
  const loops = (await service(request, `/api/loops?lang=zh`)).loops.filter((l: any) => l.workspace === extra.workspace);
  const waiting = inbox.byWorkspace[extra.workspace] ?? [];
  await expect.poll(async () => (await drawn(page)).pages.workspace?.memoryStatus).toBe(memory.status);
  const st = (await drawn(page)).pages.workspace;
  expect(st.teamLine).toBe(ws.teamLine);
  expect(st.briefStatus).toBe(ws.briefStatus);
  expect(st.problems).toEqual(ws.problems.map((p: any) => p.text));
  expect(st.waiting).toEqual(waiting.map((w: any) => ({ run: w.run, loop: w.loop ?? null, title: w.title, label: w.label, tone: w.tone, entries: w.entries })));
  expect(st.loops).toEqual(loops.map((l: any) => ({ id: l.id, title: l.title, pill: l.pill.text, schedule: l.scheduleText })));
  const records = await service(request, `/api/runs?workspace=${extra.workspace}&limit=50&lang=zh`);
  const shown = records.runs.filter((r: any) => !waiting.some((w: any) => w.run === r.id));
  expect(st.records).toEqual(shown.map((r: any) => ({ id: r.id, title: r.title, label: r.label.text })));
  // What the page shows is what it says it draws.
  await expect(page.locator(".ws-team .secondary")).toHaveText(ws.teamLine);
  await expect(page.locator(".knowledge-tile").nth(1)).toContainText(memory.status);
  await shot(page, info, "home");
});

test("the list of workspaces marks those at work and the deleted ones as the service does", async ({ page, request }) => {
  await page.goto("/workspaces");
  const listed = await service(request, "/api/runs/workspaces");
  await expect.poll(async () => (await drawn(page)).pages.workspaces?.working).toEqual(listed.working);
  expect((await drawn(page)).pages.workspaces.past).toEqual(listed.workspaces.filter((w: any) => w.deleted).map((w: any) => w.id));
});

test("a goal started on the web runs to delivery", async ({ page, request }, info) => {
  const goal = `网页上提的目标（${info.project.name}）`;
  await page.goto(`/workspaces/${extra.newWorkspace}`);
  const box = page.getByRole("textbox", { name: "想要什么结果？" });
  await box.fill(goal);
  await shot(page, info, "home-goal");
  await page.getByRole("button", { name: "交给负责人" }).click();
  await expect.poll(async () => (await drawn(page)).route.page).toBe("run");
  const id = decodeURIComponent(new URL(page.url()).pathname.split("/")[2]!);
  const run = await service(request, `/api/runs/${id}`);
  expect(run.run.goal).toBe(goal);
  expect(run.run.config.workspace.id).toBe(extra.newWorkspace);
  await expect.poll(async () => (await service(request, `/api/runs/${id}`)).run.status, { timeout: 60_000 }).toBe("delivered");
});

test("a new chat starts with the employee picked", async ({ page, request }, info) => {
  await page.goto("/chats/new");
  const setup = await service(request, "/api/setup?lang=zh");
  await expect.poll(async () => (await drawn(page)).pages.newChat?.groups).toEqual(setup.roleGroups.map((g: any) => ({ title: g.title, roles: g.roles })));
  const ready = setup.roles.find((r: any) => r.id === `${extra.newWorkspace}-m1`);
  expect(ready.unavailable).toBeNull();
  await page.locator(".role-choice", { hasText: ready.name }).click();
  await expect.poll(async () => (await drawn(page)).pages.newChat.chosen).toBe(ready.id);
  await shot(page, info, "new-chat");
  await page.getByRole("button", { name: `和 ${ready.name} 开始对话` }).click();
  await expect.poll(async () => (await drawn(page)).route.page).toBe("run");
  const id = decodeURIComponent(new URL(page.url()).pathname.split("/")[2]!);
  const run = (await service(request, `/api/runs/${id}`)).run;
  expect([run.kind, run.config.team[0].role_id]).toEqual(["chat", ready.id]);
});

test("team memory lists the notes as the service words them, and changes reach it", async ({ page, request }, info) => {
  await page.goto(`/workspaces/${extra.workspace}/memory`);
  const memory = await service(request, `/api/workspaces/${extra.workspace}/memories?lang=zh`);
  await expect.poll(async () => (await drawn(page)).pages.memory?.notes.map((n: any) => n.id)).toEqual(memory.memories.map((n: any) => n.id));
  const st = (await drawn(page)).pages.memory;
  expect(st.status).toBe(memory.status);
  for (const [i, n] of memory.memories.entries()) {
    expect(st.notes[i]).toMatchObject({ kind: n.kindLabel, title: n.title, hook: n.hook });
    expect(st.notes[i].source.startsWith(`${n.source} · `)).toBe(true);
  }
  await shot(page, info, "memory");
  const howto = memory.memories.find((n: any) => n.kind === "howto");
  await page.locator(`a[href$="/memory/${howto.id}"]`).click();
  await expect.poll(async () => (await drawn(page)).pages.note?.found).toBe(true);
  await shot(page, info, "memory-note");
  // Added to the shared material: the service's line for the note, unsaved there.
  await page.getByRole("button", { name: "加入共享资料…" }).click();
  await expect.poll(async () => (await drawn(page)).route.page).toBe("brief");
  await expect.poll(async () => (await drawn(page)).pages.brief?.text).toContain(`- ${howto.briefLine}`);
  expect((await drawn(page)).pages.brief.saved).toBe("");
  await shot(page, info, "brief");
  if (phone(info)) return;
  // Changed and deleted on the desktop.
  await page.goto(`/workspaces/${extra.workspace}/memory/${howto.id}`);
  await page.getByRole("textbox", { name: "标题" }).fill("怎么跑测试（改过）");
  await page.getByRole("button", { name: "保存" }).click();
  await expect.poll(async () => (await service(request, `/api/workspaces/${extra.workspace}/memories?lang=zh`)).memories.find((n: any) => n.id === howto.id)?.title).toBe("怎么跑测试（改过）");
  const gone = memory.memories.find((n: any) => n.title === "要删的笔记");
  await page.goto(`/workspaces/${extra.workspace}/memory/${gone.id}`);
  page.once("dialog", (d) => void d.accept());
  await page.getByRole("button", { name: "删除…" }).click();
  await expect.poll(async () => (await service(request, `/api/workspaces/${extra.workspace}/memories?lang=zh`)).memories.some((n: any) => n.id === gone.id)).toBe(false);
});

test("a Loop's page and its form say what the service says, and a change reaches it", async ({ page, request }, info) => {
  await page.goto(`/runs/${extra.loopRun}`);
  const loop = await service(request, `/api/loops/${extra.loop}?lang=zh`);
  await expect.poll(async () => (await drawn(page)).pages.loop?.id).toBe(extra.loop);
  expect((await drawn(page)).pages.loop).toMatchObject({ title: loop.title, pill: loop.pill.text, schedule: loop.scheduleText, dispatchedCount: loop.dispatchedCount });
  await expect(page.locator(".loop-head")).toContainText(loop.scheduleText);
  await shot(page, info, "loop");
  await page.goto(`/loops/${extra.loop}/edit`);
  await expect.poll(async () => (await drawn(page)).pages.loopEditor?.upcoming.length).toBe(3);
  expect((await drawn(page)).pages.loopEditor.cron).toBe(loop.schedule.cron);
  await shot(page, info, "loop-edit");
  if (phone(info)) return;
  await page.getByRole("combobox", { name: "频率" }).selectOption("hours");
  await page.getByRole("combobox", { name: "小时" }).selectOption("2");
  await expect.poll(async () => (await drawn(page)).pages.loopEditor.cron).toBe("0 */2 * * *");
  await expect.poll(async () => (await drawn(page)).pages.loopEditor.upcoming.length).toBe(3);
  await page.getByRole("button", { name: "保存" }).click();
  await expect.poll(async () => (await service(request, `/api/loops/${extra.loop}?lang=zh`)).scheduleText).toBe("每 2 小时");
  // A new Loop, made only.
  await page.goto(`/workspaces/${extra.workspace}/loops/new`);
  await page.getByPlaceholder("每次开工都把这段话交给它。写清楚看哪里、什么情况派成任务、什么情况问你。").fill("每天看一眼 CI");
  await page.getByRole("radio", { name: "只在你叫它时" }).click();
  await page.getByRole("button", { name: "只创建" }).click();
  await expect.poll(async () => (await service(request, "/api/loops?lang=zh")).loops.some((l: any) => l.prompt === "每天看一眼 CI" && l.scheduleText === "只在你叫它时")).toBe(true);
});

test("a workspace is made from a folder typed in, checked by the service as it is typed", async ({ page, request }, info) => {
  await page.goto("/workspaces/new");
  await page.getByPlaceholder("/path/to/your/repo").fill(extra.folder);
  const inspect = await service(request, `/api/inspect?path=${encodeURIComponent(extra.folder)}&lang=zh`);
  await expect.poll(async () => (await drawn(page)).pages.workspaceEditor?.inspect).toBe(inspect.text);
  await expect(page.locator(".field .caption").first()).toHaveText(inspect.text);
  await shot(page, info, "workspace-new");
  if (phone(info)) return;
  await page.getByPlaceholder("名称（可选，默认用目录名）").fill("网页建的工作区");
  await page.getByRole("button", { name: "创建工作区" }).click();
  await expect.poll(async () => (await drawn(page)).route.page).toBe("workspace");
  const setup = await service(request, "/api/setup?lang=zh");
  expect(setup.workspaces.some((w: any) => w.name === "网页建的工作区")).toBe(true);
});
