import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import { latestRelease, repoStars } from "./scripts/release.ts";

// The app's version, [workspace.package] in Cargo.toml: the fallback when GitHub can't be reached.
const cargo = readFileSync(new URL("../Cargo.toml", import.meta.url), "utf8");
const appVersion = /^\[workspace\.package\]\n(?:[^[].*\n)*?version = "([^"]+)"/m.exec(cargo)?.[1];
if (!appVersion) throw new Error("vite.config.ts: no version in Cargo.toml's [workspace.package]");

// Where the site is served. CI passes the Pages URL (actions/configure-pages), so a custom
// domain only needs setting in the repo's Pages settings, not here.
const siteUrl = (process.env.SITE_URL || "https://emircan-sahin.github.io/gitviber").replace(/\/$/, "");
const [release, stars] = await Promise.all([latestRelease(), repoStars()]);

export default defineConfig({
  base: `${new URL(siteUrl).pathname.replace(/\/$/, "")}/`,
  plugins: [react(), tailwindcss()],
  // SITE_OUT lets two builds run side by side without clobbering each other's output.
  build: { outDir: process.env.SITE_OUT || "dist" },
  define: {
    __RELEASE__: JSON.stringify(release),
    __SITE_URL__: JSON.stringify(siteUrl),
    __STARS__: JSON.stringify(stars),
    __VERSION__: JSON.stringify(release.version ?? appVersion),
  },
  server: { port: 4321 },
});
