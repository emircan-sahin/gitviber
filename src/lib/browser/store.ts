import { browserApi, type BrowserKey, type BrowserPick, type BrowserState } from "../api";
import { listenHere } from "../app/settingsWindow";
import { toast } from "../app/toast";
import { bindingsFor, COMMANDS } from "../commands/commands";
import { getSettings, subscribeSettings } from "../settings";
import { createStore } from "../store";
import { type BrowserDownload, reported } from "./downloads";
import { skillWanted } from "./skill";
import { onAgentFinished } from "../terminal/agents";

// What the browser tabs' native views report (browser/macos/), and the keys they hand back.

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

// Parked to save memory (registry.rs): its view is gone, and what it last said with it. A tab
// on show (parked as it came back) makes it again.
const parks = createStore<ReadonlyMap<string, number>>(new Map());
listenHere<{ id: string }>("browser-parked", ({ payload: { id } }) => {
  const all = new Map(states.get());
  if (all.delete(id)) states.set(all);
  parks.set(new Map(parks.get()).set(id, (parks.get().get(id) ?? 0) + 1));
}).catch(() => {});

/** How many times tab `id`'s view has parked: a new view is due each time. */
export const useParks = (id: string) => parks.use().get(id) ?? 0;


// Settings → Browser: how many pages out of sight stay alive, for how long, how many agents' tabs
// draw at once, whether they keep a console, and whether agents' tabs are muted.
let sentPolicy = "";
function sendPolicy() {
  const { browserLiveHidden, browserParkAfterMin, browserAgentDrawn, browserConsole, browserAgentControl, browserMuteAgentTabs } = getSettings();
  const key = `${browserLiveHidden} ${browserParkAfterMin} ${browserAgentDrawn} ${browserConsole} ${browserAgentControl} ${browserMuteAgentTabs}`;
  if (key === sentPolicy) return;
  sentPolicy = key;
  void browserApi.configure(browserLiveHidden, browserParkAfterMin, browserAgentDrawn, browserConsole, browserAgentControl, browserMuteAgentTabs).catch(() => {});
}
subscribeSettings(sendPolicy);
sendPolicy();

// The agents' skill, written from this window alone (Settings only reads it), and at launch: a new
// version's HELP replaces the one an older one wrote.
let sentSkill: boolean | null = null;
function sendSkill() {
  const on = skillWanted(getSettings());
  if (on === sentSkill) return;
  sentSkill = on;
  void browserApi.skill(on).catch(() => {});
}
subscribeSettings(sendSkill);
sendSkill();

// An agent done with its turn: its worktree's pages show what it changed (Settings → Browser).
// Agents in one worktree often finish together: one reload for them.
const finishing = new Map<string, ReturnType<typeof setTimeout>>();
onAgentFinished((dir) => {
  if (!getSettings().browserReloadOnAgentDone) return;
  clearTimeout(finishing.get(dir));
  finishing.set(
    dir,
    setTimeout(() => {
      finishing.delete(dir);
      void browserApi.agentDone(dir).catch(() => {});
    }, 1000),
  );
});

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

// The element picker's result by tab: a pick to write a note on, or null when it ended without
// one (cancelled, or the page moved on). Undefined: no picker news since the tab last took it.
// Queued, as ⇧-clicks may bring a few before the tab takes them.
const picked = createStore<ReadonlyMap<string, readonly (BrowserPick | null)[]>>(new Map());
listenHere<{ id: string; pick: BrowserPick | null }>("browser-picked", ({ payload: { id, pick } }) =>
  picked.set(new Map(picked.get()).set(id, [...(picked.get().get(id) ?? []), pick])),
).catch(() => {});
export const usePicked = (id: string) => picked.use().get(id);
/** The tab has taken its picker news (shown the note, or seen it end). */
export function takePicked(id: string) {
  const next = new Map(picked.get());
  if (next.delete(id)) picked.set(next);
}

// What each tab's page logged since its console was last cleared (console.rs), for its badge.
const logged = createStore<ReadonlyMap<string, { errors: number; warnings: number }>>(new Map());
listenHere<{ id: string; errors: number; warnings: number }>("browser-console", ({ payload: { id, ...counts } }) => logged.set(new Map(logged.get()).set(id, counts))).catch(() => {});
export const useLogged = (id: string) => logged.use().get(id) ?? { errors: 0, warnings: 0 };

/** A closed tab's page, forgotten: its view went with it, and a reopened one starts afresh. */
export function forgetBrowser(id: string) {
  const drop = <V>(store: ReturnType<typeof createStore<ReadonlyMap<string, V>>>) => {
    const next = new Map(store.get());
    if (next.delete(id)) store.set(next);
  };
  drop(states);
  drop(parks);
  drop(picked);
  drop(logged);
  drop(downloads);
}

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

// What each tab's page downloads (browser/user_downloads.rs), shown under its address until dismissed,
// or for a minute once ended. A dismissed one stays off, whatever is reported of it after.
const downloads = createStore<ReadonlyMap<string, readonly BrowserDownload[]>>(new Map());
const dismissed = new Set<number>();
const SHOWN_AFTER_END_MS = 60_000;
listenHere<BrowserDownload>("browser-download", ({ payload }) => {
  downloads.set(new Map(downloads.get()).set(payload.tab, reported(downloads.get().get(payload.tab) ?? [], payload, dismissed)));
  if (payload.state !== "going") setTimeout(() => dismissDownload(payload.tab, payload.key), SHOWN_AFTER_END_MS);
}).catch(() => {});

// A tab refused a download (a burst of them, or one from a page out of sight): one quiet toast for the burst.
let blockedAt = 0;
listenHere("browser-download-blocked", () => {
  if (Date.now() - blockedAt < 5000) return;
  blockedAt = Date.now();
  toast("info", "Download blocked", "A page started several downloads at once, or one out of sight.");
}).catch(() => {});

const NONE: readonly BrowserDownload[] = [];
export const useDownloads = (id: string) => downloads.use().get(id) ?? NONE;

/** Download `key` of tab `id` taken off its list. */
export function dismissDownload(id: string, key: number) {
  dismissed.add(key);
  const left = (downloads.get().get(id) ?? NONE).filter((d) => d.key !== key);
  const next = new Map(downloads.get());
  if (left.length) next.set(id, left);
  else next.delete(id);
  downloads.set(next);
}
