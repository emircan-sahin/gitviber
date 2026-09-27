import { listen } from "@tauri-apps/api/event";
import { api } from "../api";

// ⌘Q (menu.rs) ends the app without unloading the page: what saves on the way out (the terminals'
// output, unsaved edits) runs as for a reload, then the app goes.
try {
  listen("quit", () => {
    window.dispatchEvent(new Event("pagehide"));
    void api.quit().catch(() => {});
  }).catch(() => {});
} catch {
  // Not in Tauri (the browser-only dev fixture).
}
