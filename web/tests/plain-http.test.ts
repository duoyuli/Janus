// A phone opens Janus over plain HTTP (a local network or Tailscale address), where the browser
// leaves out what only secure contexts have: sending a message once threw on
// `crypto.randomUUID` there, and nothing was sent. Only `lib/plain-http.ts` may touch those.

import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const SRC = fileURLToPath(new URL("../src", import.meta.url));
const SECURE_ONLY = /crypto\.randomUUID|crypto\.subtle|navigator\.clipboard|navigator\.serviceWorker|navigator\.share\b|Notification\./;

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? files(p) : /\.tsx?$/.test(n) ? [p] : [];
  });
}

test("only lib/plain-http.ts uses what plain HTTP pages lack", () => {
  const found = files(SRC)
    .filter((p) => relative(SRC, p) !== join("lib", "plain-http.ts"))
    .flatMap((p) => readFileSync(p, "utf8").split("\n").map((l, i) => [p, i + 1, l] as const))
    .filter(([, , l]) => SECURE_ONLY.test(l))
    .map(([p, n]) => `${relative(SRC, p)}:${n}`);
  assert.deepEqual(found, []);
});
