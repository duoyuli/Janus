// The translations the Mac app and the web client share: `frontend/Sources/Janus/en.lproj/
// Localizable.strings`, keyed by the Chinese text with Foundation format specifiers, the English
// as the value. Read at build time (`strings-plugin.ts`) and here at test time; Chinese is the key
// itself. See docs/architecture/20261126-web.md#i18n.

/** Parse an Apple `.strings` file: `"key" = "value";` lines, `/* … *\/` comments, `\n` `\"` `\\` escapes. */
export function parseStrings(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  let i = 0;
  const n = text.length;
  const skip = () => {
    for (;;) {
      while (i < n && /\s/.test(text[i]!)) i++;
      if (text.startsWith("/*", i)) {
        const end = text.indexOf("*/", i + 2);
        i = end < 0 ? n : end + 2;
      } else if (text.startsWith("//", i)) {
        const end = text.indexOf("\n", i);
        i = end < 0 ? n : end + 1;
      } else return;
    }
  };
  const quoted = (): string => {
    if (text[i] !== '"') throw new Error(`Localizable.strings: expected " at ${i}`);
    i++;
    let s = "";
    while (i < n && text[i] !== '"') {
      const c = text[i]!;
      if (c === "\\") {
        const e = text[i + 1];
        s += e === "n" ? "\n" : e === "t" ? "\t" : e === "r" ? "\r" : (e ?? "");
        i += 2;
      } else {
        s += c;
        i++;
      }
    }
    i++;
    return s;
  };
  for (;;) {
    skip();
    if (i >= n) break;
    const key = quoted();
    skip();
    if (text[i] !== "=") throw new Error(`Localizable.strings: expected = after ${JSON.stringify(key)}`);
    i++;
    skip();
    const value = quoted();
    skip();
    if (text[i] === ";") i++;
    out[key] = value;
  }
  return out;
}

/** One value put into a translated text: a whole number is `%lld`, anything else `%@`, as Swift's
 * string interpolation makes the key. */
export type Arg = string | number;

/** The key Swift's `L.text("…\(x)…")` looks up for these parts and values. */
export function keyOf(parts: readonly string[], args: readonly Arg[]): string {
  if (args.length === 0) return parts[0] ?? "";
  let key = "";
  parts.forEach((p, k) => {
    key += p.replace(/%/g, "%%");
    if (k < args.length) key += spec(args[k]!);
  });
  return key;
}

function spec(a: Arg): string {
  return typeof a === "number" && Number.isInteger(a) ? "%lld" : "%@";
}

/** Fill a format (`%@`, `%lld`, `%d`, `%1$@`, `%%`) with `args` in order; no args, no formatting. */
export function fill(format: string, args: readonly Arg[]): string {
  if (args.length === 0) return format;
  let next = 0;
  return format.replace(/%(?:(\d+)\$)?(@|lld|ld|d|%)/g, (_m, pos: string | undefined, kind: string) => {
    if (kind === "%") return "%";
    const a = pos ? args[Number(pos) - 1] : args[next++];
    return a === undefined ? "" : String(a);
  });
}
