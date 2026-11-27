// Settings on the web, against what the service returns, at phone and desktop width: digital
// employees made and changed (a picture too), an Agent's editor reading its private values, the
// library's skills and MCP servers (an MCP value read back), the usage bill, and what only the Mac
// may set. See docs/operations/20261031-testing.md#web_e2e.

import { expect, test, type TestInfo } from "@playwright/test";
import { fileURLToPath } from "node:url";
import { drawn, fromPage, service, shot } from "./helpers.ts";

const phone = (info: TestInfo) => info.project.name === "phone";
/** A name of this project's own, so the phone's and the desktop's things do not meet. */
const own = (info: TestInfo, name: string) => `${name}-${info.project.name}`;
const picture = fileURLToPath(new URL("../../docs/assets/icon.png", import.meta.url));

test("a digital employee is made and changed, picture and all", async ({ page, request }, info) => {
  const name = own(info, "e2e 评审");
  await page.goto("/settings/agents/role/new");
  await page.getByRole("textbox", { name: "名称" }).fill(name);
  await page.getByRole("textbox", { name: "职责" }).fill("独立检查变更");
  await page.locator('input[type="file"]').setInputFiles(picture);
  await page.getByRole("textbox", { name: "指令" }).fill("先读测试。");
  await shot(page, info, "role-editor");
  await page.getByRole("button", { name: "创建" }).click();
  await expect.poll(async () => (await service(request, "/api/setup?lang=zh")).roles.find((r: any) => r.name === name)?.avatar ?? "").not.toBe("");
  const role = (await service(request, "/api/setup?lang=zh")).roles.find((r: any) => r.name === name);
  expect(role.line).toBe("独立检查变更 · 提示词");
  const png = await request.get(`/api/roles/${role.id}/avatar`);
  expect(png.headers()["content-type"]).toBe("image/png");

  await page.goto("/settings/agents");
  await expect.poll(async () => (await drawn(page)).pages.agents?.roles.find((r: any) => r.id === role.id)?.line).toBe(role.line);
  const st = (await drawn(page)).pages.agents;
  const served = await service(request, "/api/setup?lang=zh");
  expect(st.roles.map((r: any) => r.id)).toEqual(served.roles.map((r: any) => r.id));
  expect(st.groups).toEqual(served.roleGroups.map((g: any) => ({ title: g.title, roles: g.roles })));
  expect(st.connections.map((c: any) => c.summary)).toEqual(served.connections.map((c: any) => c.summary));
  await shot(page, info, "agents");

  await expect(page.locator(`[data-role="${role.id}"]`)).toHaveAttribute("href", `/settings/agents/role/${role.id}`);
  await page.goto(`/settings/agents/role/${role.id}`);
  await page.getByRole("textbox", { name: "职责" }).fill("独立检查变更，找反例");
  await page.getByRole("button", { name: "保存" }).click();
  await expect.poll(async () => (await service(request, "/api/setup?lang=zh")).roles.find((r: any) => r.id === role.id).desc).toBe("独立检查变更，找反例");
});

test("an Agent's editor shows its private values, read from the service", async ({ page, request }, info) => {
  const token = `sk-e2e-${info.project.name}`;
  const res = await request.post("/api/setup/connections", {
    data: { kind: "claude", name: own(info, "网关"), auth: "apikey", envText: `export ANTHROPIC_BASE_URL=https://gw.example\nexport ANTHROPIC_AUTH_TOKEN=${token}` },
  });
  expect(res.ok(), await res.text()).toBeTruthy();
  const c = await res.json();
  const listed = (await service(request, "/api/setup?lang=zh")).connections.find((x: any) => x.id === c.id);
  expect(listed.env.ANTHROPIC_AUTH_TOKEN, "the list does not carry the private value").toBeNull();
  await page.goto(`/settings/agents/agent/${c.id}`);
  const env = page.getByRole("textbox", { name: "环境变量" });
  await expect(env).toHaveValue(new RegExp(`ANTHROPIC_AUTH_TOKEN=${token}`));
  await shot(page, info, "agent-editor");
  expect((await fromPage(page, `/api/connections/${c.id}/env`)).status, "a paired device reads the values").toBe(200);
});

test("the library's skills and MCP servers are added, and an MCP value reads back", async ({ page, request }, info) => {
  const skill = own(info, "e2e-web-skill");
  await page.goto("/settings/library/skills/new");
  await page.getByRole("textbox", { name: /名称：小写字母/ }).fill(skill);
  await page.getByRole("textbox", { name: /什么时候用/ }).fill("写 e2e 时使用");
  await page.getByRole("textbox", { name: "SKILL.md" }).fill("照着做。");
  await page.getByRole("button", { name: "保存" }).click();
  await expect.poll(async () => (await service(request, "/api/library?lang=zh")).items.some((i: any) => i.name === skill)).toBe(true);
  await expect.poll(async () => (await drawn(page)).pages.library?.items.some((i: any) => i.name === skill)).toBe(true);
  const lib = await service(request, "/api/library?lang=zh");
  const st = (await drawn(page)).pages.library;
  expect(st.items).toEqual(lib.items.filter((i: any) => i.kind === "skill").map((i: any) => ({ id: i.id, name: i.name, warning: i.warning, origin: i.originText })));
  expect(st.picks.length).toBeGreaterThan(0);
  await shot(page, info, "library-skills");

  const server = own(info, "e2e-mcp");
  await page.goto("/settings/library/servers/new");
  await page.getByRole("textbox", { name: "名称" }).fill(server);
  await page.getByRole("textbox", { name: "命令" }).fill("npx -y 'e2e mcp'");
  await page.getByRole("button", { name: "+ 添加" }).click();
  await page.getByRole("textbox", { name: "GITHUB_TOKEN" }).fill("E2E_KEY");
  await page.getByRole("textbox", { name: "值" }).fill("v-1");
  await page.getByRole("radio", { name: "JSON" }).click();
  await expect(page.getByRole("textbox", { name: "JSON" })).toHaveValue(/"E2E_KEY": "v-1"/);
  await page.getByRole("radio", { name: "表单" }).click();
  await page.getByRole("button", { name: "保存" }).click();
  await expect.poll(async () => (await service(request, "/api/library?lang=zh")).items.find((i: any) => i.name === server)?.id ?? "").not.toBe("");
  const item = (await service(request, "/api/library?lang=zh")).items.find((i: any) => i.name === server);
  expect(item.server.command).toBe("npx");
  expect(item.server.args).toEqual(["-y", "e2e mcp"]);
  expect((await service(request, `/api/library/mcp/${item.id}/values`)).values.E2E_KEY).toBe("v-1");
  await page.goto(`/settings/library/servers/${item.id}`);
  await expect(page.getByRole("textbox", { name: "值" })).toHaveValue("v-1");
  await expect(page.getByRole("textbox", { name: "命令" })).toHaveValue("npx -y 'e2e mcp'");
  await shot(page, info, "mcp-editor");
  await page.goto("/settings/library/servers");
  await expect.poll(async () => (await drawn(page)).pages.library?.items.some((i: any) => i.name === server)).toBe(true);
  await shot(page, info, "library-servers");
});

test("the usage bill draws the service's receipt", async ({ page, request }, info) => {
  await page.goto("/settings/usage");
  await expect.poll(async () => (await drawn(page)).pages.usage?.headline ?? null).not.toBeNull();
  const offset = await page.evaluate(() => -new Date().getTimezoneOffset());
  const served = (await service(request, `/api/usage/receipt?period=days30&offset=${offset}&lang=zh`)).receipt;
  const st = (await drawn(page)).pages.usage;
  expect(st.headline).toBe(served.headline);
  expect(st.groups).toEqual(served.groups.map((g: any) => g.name));
  expect(st.totals).toEqual(served.totals);
  await shot(page, info, "usage");
});

test("what decides what agents reach stays on the Mac", async ({ page }, info) => {
  await page.goto("/settings/general");
  await expect(page.getByText("macOS 权限和本机环境只能在 Mac 上的 Janus 设置")).toBeVisible();
  expect((await fromPage(page, "/api/host-env")).status).toBe(403);
  await shot(page, info, "general");
  if (phone(info)) {
    await page.goto("/settings");
    await expect(page.locator(".settings-nav-item")).toHaveCount(5);
    await shot(page, info, "settings-list");
  }
});
