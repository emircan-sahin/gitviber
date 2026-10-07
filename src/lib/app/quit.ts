import { api } from "../api";
import { netActivity } from "../repo/netActivity";
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
/** What leaving would stop in the terminals ("2 agents working"), registered by them. */
export const stoppedByQuit = (what: () => Promise<string[]>) => void stops.push(what);

/** What leaving the app now stops that's worth asking about: a push or pull on its way, the terminals' commands. A quit and an update's restart ask it. */
export async function stoppedByLeaving() {
  const terminals = (await Promise.all(stops.map((what) => what().catch(() => [])))).flat();
  const net = netActivity()?.label;
  return net ? [net, ...terminals] : terminals;
}

/** Whether the user lets the quit go: asked only while something runs. quit.rs ends the app if this hangs. */
async function letGo() {
  if (!getSettings().askBeforeQuit) return true;
  const running = await stoppedByLeaving();
  if (!running.length) return true;
  await api.quitAnswer("asking");
  return ask(`Quitting GitViber stops what's still running: ${running.join(", ")}.`, { title: "Quit GitViber", kind: "warning", okLabel: "Quit" });
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
