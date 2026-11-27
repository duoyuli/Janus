// A paired browser unpairs itself from Settings › 手机访问: the service forgets it, and the window
// goes back to saying how to pair, also after a reload. Runs last: the cookie is gone after it.

import { expect, test } from "@playwright/test";
import { drawn, fromPage, service, shot } from "./helpers.ts";

test("this browser unpairs itself and is sent back to pairing", async ({ page, request }, info) => {
  await page.goto("/settings");
  await expect.poll(async () => (await drawn(page)).pages.phoneAccess?.me).toBeTruthy();
  const st = (await drawn(page)).pages.phoneAccess;
  const listed = (await service(request, "/api/devices")).map((d: any) => d.id);
  expect(st.devices).toEqual(listed);
  // The switch and the QR codes are the Mac's: a device may not turn phone access on.
  expect((await fromPage(page, "/api/lan-access", { on: true }, "PUT")).status).toBe(403);
  await shot(page, info, "phone-access");

  await page.locator(`[data-device="${st.me}"]`).getByRole("button", { name: "取消配对" }).click();
  await expect.poll(async () => Object.keys((await drawn(page)).pages)).toEqual(["unpaired"]);
  expect((await service(request, "/api/devices")).map((d: any) => d.id)).not.toContain(st.me);
  await page.reload();
  await expect(page.getByText("这个浏览器还没有和 Janus 配对")).toBeVisible();
  expect((await fromPage(page, "/api/inbox")).status).toBe(401);
  await shot(page, info, "unpaired-after");
});
