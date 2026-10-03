import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

// KaTeX's stylesheet lists each font as woff2, woff and ttf; the webview reads woff2, so the
// other two (~1 MB) stay out of the app.
const katexWoff2Only: Plugin = {
  name: "katex-woff2-only",
  enforce: "pre",
  transform: (code, id) => (/katex[\\/]dist[\\/]katex(\.min)?\.css$/.test(id) ? code.replace(/,url\([^)]*\.(woff|ttf)\) format\("(woff|truetype)"\)/g, "") : undefined),
};

// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react(), tailwindcss(), katexWoff2Only],
  resolve: { alias: { "@": path.resolve(import.meta.dirname, "./src") } },
  clearScreen: false,
  server: {
    // scripts/tauri.mjs picks a free one when another worktree already has 1420.
    port: Number(process.env.GITVIBER_DEV_PORT) || 1420,
    strictPort: true,
    host: "127.0.0.1",
    // Agent worktrees live in .claude/worktrees: their writes reloaded this app's page over and over.
    watch: { ignored: ["**/src-tauri/**", "**/.claude/**"] },
  },
  // ES workers can code-split, so each Shiki grammar/theme loads only when first used.
  worker: { format: "es" },
  build: {
    target: "safari16",
    chunkSizeWarningLimit: 1500,
    // Keep the ~1200 icon SVGs as separate files instead of base64 inside the JS bundle.
    assetsInlineLimit: (file) => (file.includes("material-icon-theme") ? false : undefined),
  },
});
