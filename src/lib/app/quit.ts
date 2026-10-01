import { listen } from "@tauri-apps/api/event";
import { api } from "../api";

// ⌘Q (menu.rs) ends the app without unloading the page: what saves on the way out (the terminals'
// output, unsaved edits) runs as for a reload, then the app goes.
/** Runs what saves on pagehide, for a way out that doesn't unload the page (quit, an update's relaunch). */
export const saveNow = () => window.dispatchEvent(new Event("pagehide"));

const before: (() => Promise<unknown>)[] = [];
/** Work the save on ⌘Q waits for (the terminals' agents looked up again); menu.rs ends the app 2 s in regardless. */
export const beforeQuit = (work: () => Promise<unknown>) => void before.push(work);
const BEFORE_QUIT_MS = 1000;

try {
  listen("quit", async () => {
    await Promise.race([Promise.allSettled(before.map((work) => work())), new Promise((done) => setTimeout(done, BEFORE_QUIT_MS))]);
    saveNow();
    void api.quit().catch(() => {});
  }).catch(() => {});
} catch {
  // Not in Tauri (the browser-only dev fixture).
}
