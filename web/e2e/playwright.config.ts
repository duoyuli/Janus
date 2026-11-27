// The web client's end-to-end tests, run by e2e/web.py against a server it started
// (docs/operations/20261031-testing.md#web_e2e). The Chrome installed on this Mac drives them;
// nothing is downloaded. Results and pictures go to the run's report directory, never the repository.
//
// The projects run one after another on one server and one set of runs: `pair` pairs Chrome the
// way a phone does and keeps its cookie, the phone looks, the desktop answers and puts off, and
// `unpair` ends the pairing last.

import { defineConfig, devices } from "@playwright/test";
import { tmpdir } from "node:os";
import { join } from "node:path";

const report = process.env.JANUS_WEB_REPORT ?? join(tmpdir(), "janus-web-e2e");
/** The paired browser's cookie, written by `pair` for the projects after it. */
export const PAIRED = join(report, "paired.json");
/** A name for 127.0.0.1 that Chrome does not treat as a secure context (`plain-http`). */
const PLAIN_HOST = "janus.test";

export default defineConfig({
  testDir: ".",
  outputDir: join(report, "results"),
  reporter: [["list"], ["html", { outputFolder: join(report, "playwright"), open: "never" }]],
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
    { name: "pair", testMatch: /\/pair\.spec\.ts$/, use: { viewport: { width: 1280, height: 820 } } },
    {
      name: "phone",
      testMatch: /(web|home|settings|session|run|confirm|brief|mobile)\.spec\.ts/,
      dependencies: ["pair"],
      use: { ...devices["iPhone 13"], channel: "chrome", defaultBrowserType: "chromium", viewport: { width: 390, height: 844 }, storageState: PAIRED },
    },
    { name: "desktop", testMatch: /(web|home|settings|session|run|confirm|brief)\.spec\.ts/, dependencies: ["phone"], use: { viewport: { width: 1280, height: 820 }, storageState: PAIRED } },
    { name: "unpair", testMatch: /unpair\.spec\.ts/, dependencies: ["desktop"], use: { viewport: { width: 1280, height: 820 }, storageState: PAIRED } },
    // The same server under a name Chrome does not count as secure, as a phone on the local
    // network or Tailscale sees it; it pairs there itself.
    {
      name: "plain-http",
      testMatch: /plain-http\.spec\.ts/,
      dependencies: ["unpair"],
      use: {
        ...devices["iPhone 13"],
        channel: "chrome",
        defaultBrowserType: "chromium",
        viewport: { width: 390, height: 844 },
        baseURL: process.env.JANUS_WEB_URL?.replace("127.0.0.1", PLAIN_HOST),
        launchOptions: { args: [`--host-resolver-rules=MAP ${PLAIN_HOST} 127.0.0.1`] },
      },
    },
  ],
});
