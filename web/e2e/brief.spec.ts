// A delivery's lines the lead suggests for the shared material: 「加入共享资料…」 on the run page
// opens the workspace's shared material with them added at the end, and saving keeps them there.
// The phone looks; the desktop saves. See docs/operations/20261031-testing.md#web_e2e.

import { expect, test, type APIRequestContext, type TestInfo } from "@playwright/test";
import { data, drawn, service, shot } from "./helpers.ts";

const phone = (info: TestInfo) => info.project.name === "phone";

test("a delivery's suggested lines go to the shared material and are saved there", async ({ page, request }, info) => {
  const id = data.suggested;
  const lines = await suggested(request, id);
  expect(lines).toEqual(["定价页在 docs/pricing.md", "免费版限 3 个项目"]);
  await page.goto(`/runs/${id}`);
  await page.getByRole("button", { name: "加入共享资料…" }).click();
  await expect.poll(() => new URL(page.url()).pathname).toBe("/workspaces/web-brief/brief");
  await expect.poll(async () => (await drawn(page)).pages.brief?.text ?? "").toContain(lines[0]);
  const text: string = (await drawn(page)).pages.brief.text;
  for (const l of lines) expect(text).toContain(l);
  await shot(page, info, "brief-suggested");
  if (phone(info)) return;
  await page.getByRole("button", { name: "保存" }).click();
  await expect.poll(async () => ((await service(request, "/api/setup")).workspaces.find((w: any) => w.id === "web-brief")?.brief ?? "") as string).toBe(text);
});

/** The suggested lines of the run's delivery, as its record holds them. */
async function suggested(request: APIRequestContext, id: string): Promise<string[]> {
  const events = (await service(request, `/api/runs/${id}`)).events as any[];
  return events.filter((e) => e.type === "run.delivered").at(-1)?.data?.briefSuggestions ?? [];
}
