import { api } from "../api";
import { getSettings } from "../settings";
import { ask } from "./ask";
import { listenHere } from "./settingsWindow";

// ⌘Q (quit.rs) ends the app without unloading the page: what saves on the way out (the terminals'
// output, unsaved edits) runs as for a reload, then the app goes.
/** Runs what saves on pagehide, for a way out that doesn't unload the page (quit, an update's relaunch). */
export const saveNow = () => window.dispatchEvent(new Event("pagehide"));

const before: (() => Promise<unknown>)[] = [];
/** Work the save on ⌘Q waits for (the terminals' agents looked up again); quit.rs ends the app 2 s after the go regardless. */
export const beforeQuit = (work: () => Promise<unknown>) => void before.push(work);
const BEFORE_QUIT_MS = 1000;

const stops: (() => Promise<string[]>)[] = [];
/** What quitting would stop that's worth asking about ("2 agents working"), for Settings → Terminal's ask. */
export const stoppedByQuit = (what: () => Promise<string[]>) => void stops.push(what);

/** Whether the user lets the quit go: asked only while something runs. quit.rs ends the app if this hangs. */
async function letGo() {
  if (!getSettings().askBeforeQuit) return true;
  const stopped = (await Promise.all(stops.map((what) => what().catch(() => [])))).flat();
  if (!stopped.length) return true;
  await api.quitAnswer("asking");
  return ask(`Quitting GitViber stops ${stopped.join(" and ")}.`, { title: "Quit GitViber", kind: "warning", okLabel: "Quit" });
}

let leaving = false;
try {
  // `now`: a second ⌘Q while the ask is up; it quits without asking.
  listenHere<boolean>("quit", async ({ payload: now }) => {
    if (!now) {
      const go = await letGo().catch(() => true);
      if (leaving) return;
      await api.quitAnswer(go ? "go" : "stay").catch(() => {});
      if (!go) return;
    }
    if (leaving) return;
    leaving = true;
    await Promise.race([Promise.allSettled(before.map((work) => work())), new Promise((done) => setTimeout(done, BEFORE_QUIT_MS))]);
    saveNow();
    void api.quit().catch(() => {});
  }).catch(() => {});
} catch {
  // Not in Tauri (the browser-only dev fixture).
}
