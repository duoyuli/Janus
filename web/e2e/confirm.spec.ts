// Asking before what cannot be undone, in the page (ui/ConfirmDialog.tsx): stopping a goal and
// deleting one ask first; 取消 leaves the service as it was, the named action reaches it. And a
// delivery's suggested lines for the shared material go to the shared material page and are saved
// there. The phone looks (a sheet from the bottom) and cancels; the desktop goes ahead.
// See docs/operations/20261031-testing.md#web_e2e.

import { expect, test, type TestInfo } from "@playwright/test";
import { data, drawn, service, shot } from "./helpers.ts";

const phone = (info: TestInfo) => info.project.name === "phone";

test("stopping a goal asks first; 取消 keeps it running, 停止 stops it", async ({ page, request }, info) => {
  const id = data.stopping;
  expect((await service(request, `/api/runs/${id}/view?lang=zh`)).actions.cancel.ok).toBe(true);
  await page.goto(`/runs/${id}`);
  await page.locator("[data-action='cancel']").click();
  const dialog = page.getByRole("alertdialog");
  await expect(dialog).toContainText("停止这个任务？");
  await expect(dialog.getByRole("button", { name: "停止" })).toHaveClass(/btn-destructive/);
  // A destructive question starts on 取消, as on the Mac.
  await expect(dialog.getByRole("button", { name: "取消" })).toBeFocused();
  await shot(page, info, "confirm-stop");
  if (phone(info)) {
    // A sheet along the bottom edge of the screen.
    const box = (await dialog.boundingBox())!;
    expect(Math.round(box.y + box.height)).toBe(page.viewportSize()!.height);
  }
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await page.locator("[data-action='cancel']").click();
  await dialog.getByRole("button", { name: "取消" }).click();
  await expect(dialog).toHaveCount(0);
  expect((await service(request, `/api/runs/${id}`)).run.status).not.toBe("cancelled");
  if (phone(info)) return;
  await page.locator("[data-action='cancel']").click();
  await dialog.getByRole("button", { name: "停止" }).click();
  await expect.poll(async () => (await service(request, `/api/runs/${id}`)).run.status).toBe("cancelled");
  await expect.poll(async () => (await drawn(page)).pages.runBoard?.capsule).toEqual((await service(request, `/api/runs/${id}/view?lang=zh`)).capsule);
});

test("deleting a goal asks first; 取消 keeps it, 删除 removes it", async ({ page, request }, info) => {
  const id = data.gone;
  expect((await service(request, `/api/runs/${id}/view?lang=zh`)).actions.delete.ok).toBe(true);
  await page.goto(`/runs/${id}`);
  await expect.poll(async () => (await drawn(page)).pages.runBoard?.title).toBeTruthy();
  const remove = async () => {
    if (phone(info)) {
      await page.getByRole("button", { name: "更多" }).click();
      await page.getByRole("menuitem", { name: "删除任务…" }).click();
    } else {
      await page.locator(".run-head-tools [aria-label='删除任务']").click();
    }
  };
  await remove();
  const dialog = page.getByRole("alertdialog");
  await expect(dialog).toContainText("删除任务？");
  await expect(dialog).toContainText("不再提醒");
  await shot(page, info, "confirm-delete");
  await dialog.getByRole("button", { name: "取消" }).click();
  await expect(dialog).toHaveCount(0);
  expect((await request.get(`/api/runs/${id}`)).status()).toBe(200);
  if (phone(info)) return;
  await remove();
  await dialog.getByRole("button", { name: "删除" }).click();
  await expect.poll(async () => (await request.get(`/api/runs/${id}`)).status()).toBe(404);
  await expect.poll(() => new URL(page.url()).pathname).toBe("/workspaces/web-gone");
});
