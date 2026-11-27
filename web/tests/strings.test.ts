// The shared translations: the parser reads the Mac app's file whole, values fill like Foundation
// formats, and every interface text the web client writes has its English in the file.

import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { fill, keyOf, parseStrings } from "../src/lib/strings.ts";

const STRINGS = fileURLToPath(new URL("../../frontend/Sources/Janus/en.lproj/Localizable.strings", import.meta.url));
const SRC = fileURLToPath(new URL("../src", import.meta.url));
const english = parseStrings(readFileSync(STRINGS, "utf8"));

test("the strings file parses whole, escapes and all", () => {
  const text = readFileSync(STRINGS, "utf8");
  // Every key line, once: a key written twice keeps its last value, as Foundation does.
  const keys = new Set(text.split("\n").flatMap((l) => (l.match(/^"((?:[^"\\]|\\.)*)" = /) ? [l.match(/^"((?:[^"\\]|\\.)*)" = /)![1]] : [])));
  assert.equal(Object.keys(english).length, keys.size);
  assert.equal(english["刚刚"], "Just now");
  assert.ok(english["\n本会话的数字不计入任务累计用量"]?.startsWith("\n"));
  assert.deepEqual(parseStrings('/* c */\n"a \\"b\\"" = "x\\ny"; // d\n"k" = "v";'), { 'a "b"': "x\ny", k: "v" });
});

test("keys are made as Swift's interpolation makes them", () => {
  assert.equal(keyOf(["", " 秒前"], [5]), "%lld 秒前");
  assert.equal(keyOf(["和 ", " 的新对话"], ["Ada"]), "和 %@ 的新对话");
  assert.equal(keyOf(["上下文 ", "%"], [80]), "上下文 %lld%%");
  assert.equal(keyOf(["超过 75%"], []), "超过 75%");
  assert.equal(keyOf(["", " 分 ", " 秒"], [3, "05"]), "%lld 分 %@ 秒");
});

test("values fill in order or by position", () => {
  assert.equal(fill("%lld min ago", [5]), "5 min ago");
  assert.equal(fill("Context %lld%%", [80]), "Context 80%");
  assert.equal(fill("%2$@ before %1$@", ["a", "b"]), "b before a");
  assert.equal(fill("75% done", []), "75% done");
});

/** Every `t\`…\`` and `tr("…")` in the web client's sources, as the key pattern it looks up. */
function usedKeys(): { file: string; text: string; pattern: RegExp }[] {
  const files: string[] = [];
  const walk = (d: string) => {
    for (const n of readdirSync(d)) {
      const p = join(d, n);
      if (statSync(p).isDirectory()) walk(p);
      else if (/\.tsx?$/.test(n)) files.push(p);
    }
  };
  walk(SRC);
  const out: { file: string; text: string; pattern: RegExp }[] = [];
  const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  for (const file of files) {
    const src = readFileSync(file, "utf8");
    for (let i = src.indexOf("t`"); i >= 0; i = src.indexOf("t`", i + 1)) {
      if (/[\w$.]/.test(src[i - 1] ?? "")) continue;
      // Read the template: literal parts and `${…}` holes (braces balanced, nested templates skipped).
      const parts: string[] = [];
      let cur = "";
      let holes = 0;
      let j = i + 2;
      for (; j < src.length && src[j] !== "`"; j++) {
        if (src[j] === "\\") {
          const e = src[j + 1];
          cur += e === "n" ? "\n" : (e ?? "");
          j++;
        } else if (src[j] === "$" && src[j + 1] === "{") {
          parts.push(cur);
          cur = "";
          holes++;
          let depth = 1;
          j += 2;
          for (; j < src.length && depth > 0; j++) {
            if (src[j] === "{") depth++;
            else if (src[j] === "}") depth--;
            else if (src[j] === "`") j = src.indexOf("`", j + 1);
          }
          j--;
        } else cur += src[j];
      }
      parts.push(cur);
      const pattern =
        holes === 0 ? new RegExp(`^${esc(parts[0]!)}$`) : new RegExp("^" + parts.map((p) => esc(p.replace(/%/g, "%%"))).join("(?:%@|%lld)") + "$");
      out.push({ file, text: parts.join("…"), pattern });
    }
    for (const m of src.matchAll(/\btr\("((?:[^"\\]|\\.)*)"/g)) {
      const key = JSON.parse(`"${m[1]}"`) as string;
      out.push({ file, text: key, pattern: new RegExp(`^${esc(key)}$`) });
    }
  }
  return out;
}

test("every interface text of the web client has its English", () => {
  const keys = Object.keys(english);
  const missing = usedKeys().filter((u) => !keys.some((k) => u.pattern.test(k)) && /[一-鿿]/.test(u.text));
  assert.deepEqual(
    missing.map((m) => `${m.file.slice(SRC.length + 1)}: ${m.text}`),
    [],
    "add these to frontend/Sources/Janus/en.lproj/Localizable.strings",
  );
});
