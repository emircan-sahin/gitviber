import { browserApi, type BrowserKey, type BrowserState, github } from "../api";
import { listenHere } from "../app/settingsWindow";
import { failed, toast } from "../app/toast";
import { bindingsFor, COMMANDS } from "../commands/commands";
import { getSettings, subscribeSettings } from "../settings";
import { createStore } from "../store";

// What the browser tabs' native views report (browser/macos.rs), and the keys they hand back.

const states = createStore<ReadonlyMap<string, BrowserState>>(new Map());

export function setBrowserState(state: BrowserState) {
  states.set(new Map(states.get()).set(state.id, state));
}

/** Tab `id`'s page as last reported; null before its view is made, or for no id. */
export function useBrowserState(id: string | null): BrowserState | null {
  const all = states.use();
  return (id && all.get(id)) || null;
}

listenHere<BrowserState>("browser-state", ({ payload }) => setBrowserState(payload)).catch(() => {});

// Only the chords bound to the app's commands, and the tab's own (⌘L, ⌘R), leave a page
// (keys.rs); the rest are the page's. Sent again as the user rebinds them.
let sentKeys = "";
function sendAppKeys() {
  const { keybindings } = getSettings();
  const chords = COMMANDS.filter((c) => !("local" in c) || c.local === "the browser").flatMap((c) => bindingsFor(c.id, keybindings));
  const key = chords.join(" ");
  if (key === sentKeys) return;
  sentKeys = key;
  void browserApi.setAppKeys(chords).catch(() => {});
}
subscribeSettings(sendAppKeys);
sendAppKeys();

// Keys typed into a page that the app takes: its tab's own first, then the app's commands, as
// if typed into this page.
const keyHandlers = new Map<string, (e: KeyboardEvent) => boolean>();

/** `handle` sees tab `id`'s keys first; true when it took one. */
export function onBrowserKey(id: string, handle: (e: KeyboardEvent) => boolean) {
  keyHandlers.set(id, handle);
  return () => void (keyHandlers.get(id) === handle && keyHandlers.delete(id));
}

listenHere<BrowserKey>("browser-key", ({ payload: { id, ...key } }) => {
  const e = new KeyboardEvent("keydown", { ...key, bubbles: true, cancelable: true });
  if (!keyHandlers.get(id)?.(e)) window.dispatchEvent(e);
}).catch(() => {});

// The page whose view has the keys, until this page takes them back (its window gets focus).
let focusedPage: string | null = null;
export const pageHasFocus = (id: string) => focusedPage === id;
window.addEventListener("focus", () => (focusedPage = null));

// A page took focus: keys now go to it, so nothing in this page should look focused.
listenHere<{ id: string }>("browser-focus", ({ payload: { id } }) => {
  focusedPage = id;
  if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
}).catch(() => {});

// Files a page would download aren't saved here.
listenHere<{ id: string; url: string }>("browser-download", ({ payload: { url } }) =>
  toast("info", "Downloads open in your browser", url, { label: "Open in Browser", run: () => void github.openUrl(url).catch(failed("Could not open the link")) }),
).catch(() => {});
