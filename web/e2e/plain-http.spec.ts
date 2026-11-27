// A phone opens Janus at a local network or Tailscale address over plain HTTP: not a secure
// context, so the browser leaves out crypto.randomUUID, the Clipboard API and the like. On
// 2026-11-27 sending a message did nothing there, while every test at 127.0.0.1 (which counts as
// secure) passed. This project reaches the same server as `janus.test` (Chrome maps the name to
// 127.0.0.1), an origin Chrome does not trust, pairs there the way a phone does, and sends and
// answers through the page. See docs/architecture/20261126-web.md#plain_http.

import { expect, request as http, test } from "@playwright/test";
import { data, drawn, service, shot } from "./helpers.ts";

test("over plain HTTP a phone pairs, sends a message and answers a request", async ({ page }, info) => {
  // The service read as a program on this Mac reads it, at 127.0.0.1 (Node does not know janus.test).
  const request = await http.newContext({ baseURL: process.env.JANUS_WEB_URL });
  const pairing = await service(request, "/api/devices/pairing", "POST");
  await page.goto(`/pair/${pairing.code}`);
  await expect.poll(async () => (await drawn(page)).route.page).toBe("inbox");
  expect(await page.evaluate(() => window.isSecureContext)).toBe(false);
  expect(await page.evaluate(() => typeof (crypto as any).randomUUID)).toBe("undefined");

  // A message from the conversation's own composer reaches the service and comes back answered.
  await page.goto(`/runs/${data.chat}`);
  await page.locator(".composer-field").fill("明文 HTTP 上发一条");
  await page.locator(".composer-buttons .btn-primary").click();
  await expect(page.locator(".chat-text", { hasText: "回复：明文 HTTP 上发一条" })).toBeVisible({ timeout: 30_000 });
  const events = (await service(request, `/api/runs/${data.chat}`)).events;
  expect(events.some((e: any) => e.type === "user.message" && e.data?.text === "明文 HTTP 上发一条")).toBe(true);
  await shot(page, info, "plain-http-chat");

  // A quick answer from the inbox goes out too (every action carries a commandId).
  await page.goto("/inbox");
  const before = (await service(request, "/api/inbox?lang=zh")).open.length;
  const quick = page.locator(".inbox-quick .btn").first();
  if (await quick.count()) {
    await quick.click();
    await expect.poll(async () => (await service(request, "/api/inbox?lang=zh")).open.length).toBeLessThan(before);
  }
});
