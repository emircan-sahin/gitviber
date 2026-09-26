import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { latestRelease } from "./scripts/release.ts";
import app from "../package.json" with { type: "json" };

// Where the site is served. CI passes the Pages URL (actions/configure-pages), so a custom
// domain only needs setting in the repo's Pages settings, not here.
const siteUrl = (process.env.SITE_URL || "https://emircan-sahin.github.io/gitviber").replace(/\/$/, "");
const release = await latestRelease();

export default defineConfig({
  base: `${new URL(siteUrl).pathname.replace(/\/$/, "")}/`,
  plugins: [react(), tailwindcss()],
  // SITE_OUT lets two builds run side by side without clobbering each other's output.
  build: { outDir: process.env.SITE_OUT || "dist" },
  define: {
    __RELEASE__: JSON.stringify(release),
    __SITE_URL__: JSON.stringify(siteUrl),
    __VERSION__: JSON.stringify(release.version ?? app.version),
  },
  server: { port: 4321 },
});
