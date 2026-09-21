import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": path.resolve(import.meta.dirname, "./src") } },
  clearScreen: false,
  server: {
    // scripts/tauri.mjs picks a free one when another worktree already has 1420.
    port: Number(process.env.GITVIBER_DEV_PORT) || 1420,
    strictPort: true,
    host: "127.0.0.1",
    watch: { ignored: ["**/src-tauri/**"] },
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
