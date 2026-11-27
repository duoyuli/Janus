// `virtual:janus-strings`: the Mac app's English translations, read from its Localizable.strings
// when the web client is built, so both clients share one table
// (docs/architecture/20261126-web.md#i18n).

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import type { Plugin } from "vite";
import { parseStrings } from "./src/lib/strings.ts";

export const STRINGS = fileURLToPath(new URL("../frontend/Sources/Janus/en.lproj/Localizable.strings", import.meta.url));

const ID = "virtual:janus-strings";
const RESOLVED = "\0" + ID;

export function janusStrings(): Plugin {
  return {
    name: "janus-strings",
    resolveId: (id) => (id === ID ? RESOLVED : undefined),
    load(id) {
      if (id !== RESOLVED) return undefined;
      this.addWatchFile(STRINGS);
      return `export default ${JSON.stringify(parseStrings(readFileSync(STRINGS, "utf8")))};`;
    },
  };
}
