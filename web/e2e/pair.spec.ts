// Pairing Chrome with Janus the way a phone does: without a cookie the window says how to pair,
// a wrong code is refused, and a code the Mac app took pairs it. The cookie it gets is kept for the
// phone and desktop tests (`PAIRED`). See docs/architecture/20261126-web.md#pairing.

import { expect, test } from "@playwright/test";
import { PAIRED } from "./playwright.config.ts";
import { drawn, fromPage, service, shot } from "./helpers.ts";

test("an unpaired browser is told how to pair, and reads nothing", async ({ page, request }, info) => {
  // The program on this Mac still reads everything without a cookie.
  expect((await service(request, "/api/inbox?lang=zh")).open.length).toBeGreaterThan(0);
  await page.goto("/inbox");
  await expect.poll(async () => Object.keys((await drawn(page)).pages)).toEqual(["unpaired"]);
  await expect(page.getByText("这个浏览器还没有和 Janus 配对")).toBeVisible();
  expect((await fromPage(page, "/api/inbox")).status).toBe(401);
  await shot(page, info, "unpaired");
  await page.setViewportSize({ width: 390, height: 844 });
  await shot(page, info, "unpaired-phone-width");
});

test("a wrong code is refused with the service's reason", async ({ page }, info) => {
  await page.goto("/pair/abcdefghjk");
  await expect.poll(async () => (await drawn(page)).pages.pair?.error).toContain("配对码不对或已经过期");
  await expect(page.getByText("配对没有成功")).toBeVisible();
  await shot(page, info, "pair-refused");
});

test("a code the Mac app took pairs this browser", async ({ page, request, context }, info) => {
  const pairing = await service(request, "/api/devices/pairing", "POST");
  expect(pairing.code).toHaveLength(10);
  await page.goto(`/pair/${pairing.code}`);
  await expect.poll(async () => (await drawn(page)).route.page).toBe("inbox");
  await expect.poll(async () => (await drawn(page)).connected).toBe(true);
  const me = JSON.parse((await fromPage(page, "/api/devices/me")).text);
  expect(me.name).toBe("Mac · Chrome");
  const devices = await service(request, "/api/devices");
  expect(devices.map((d: any) => d.id)).toContain(me.id);
  // The code was used once.
  const again = await fromPage(page, "/api/devices/claim", { code: pairing.code, name: "again" });
  expect(again.status).toBe(401);
  await shot(page, info, "paired");
  await context.storageState({ path: PAIRED });
});
