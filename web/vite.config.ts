// The web client's build (`just web`) and dev server (`just web-dev`): docs/architecture/20261126-web.md.

import { defineConfig } from "vite";
import { janusStrings } from "./strings-plugin.ts";

// The Janus service the dev server forwards `/api` to: the same JANUS_ADDR the service listens on.
const service = `http://${process.env.JANUS_ADDR ?? "127.0.0.1:7878"}`;

export default defineConfig({
  plugins: [janusStrings()],
  oxc: { jsx: { runtime: "automatic", importSource: "preact" } },
  build: {
    outDir: "dist",
    assetsDir: "assets",
    // The service embeds every file of dist/; source maps would only make the binary larger.
    sourcemap: false,
    target: "es2022",
  },
  server: {
    // The Host stays the dev server's, so the service sees the page's own origin as same-origin.
    proxy: { "/api": { target: service, changeOrigin: false } },
  },
});
