// The web client's end-to-end tests, run by e2e/web.py against a server it started
// (docs/operations/20261031-testing.md#web_e2e). The Chrome installed on this Mac drives them;
// nothing is downloaded. Results and pictures go to the run's report directory, never the repository.

import { defineConfig, devices } from "@playwright/test";
import { tmpdir } from "node:os";
import { join } from "node:path";

const report = process.env.JANUS_WEB_REPORT ?? join(tmpdir(), "janus-web-e2e");

export default defineConfig({
  testDir: ".",
  outputDir: join(report, "results"),
  reporter: [["list"], ["html", { outputFolder: join(report, "playwright"), open: "never" }]],
  // One server, one set of runs: the phone looks first, then the desktop answers and puts off.
  workers: 1,
  fullyParallel: false,
  timeout: 60_000,
  expect: { timeout: 15_000 },
  use: {
    baseURL: process.env.JANUS_WEB_URL,
    channel: "chrome",
    locale: "zh-CN",
    trace: "retain-on-failure",
  },
  projects: [
    { name: "phone", testMatch: /.*\.spec\.ts/, use: { ...devices["iPhone 13"], channel: "chrome", defaultBrowserType: "chromium", viewport: { width: 390, height: 844 } } },
    { name: "desktop", testMatch: /.*\.spec\.ts/, dependencies: ["phone"], use: { viewport: { width: 1280, height: 820 } } },
  ],
});
