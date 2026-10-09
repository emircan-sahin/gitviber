import { invoke } from "@tauri-apps/api/core";

/** A browser tab's page as its address bar shows it (browser/mod.rs State); each `browser-state` event is one. */
export interface BrowserState {
  id: string;
  url: string;
  title: string;
  loading: boolean;
  /** 0 to 1, WebKit's estimate. */
  progress: number;
  canBack: boolean;
  canForward: boolean;
  /** An https page on this machine whose self-signed certificate was let through. */
  insecure: boolean;
  /** A page that never loaded (no server on that port, say), shown in its place. */
  failed: { url: string; message: string } | null;
  /** It has loaded something since its view was made: a parked page's picture can go. */
  committed: boolean;
}

export type BrowserGo = "back" | "forward" | "reload" | "hardReload" | "stop";

/** A device's screen the view shows: the page's viewport in device CSS px (its zoom and height come from it), the corner radius in points and which corners the page reaches, and the pixel ratio. */
export interface NativeScreen {
  width: number;
  height: number;
  radius: number;
  corners: { topLeft: boolean; topRight: boolean; bottomRight: boolean; bottomLeft: boolean };
  dpr: number | null;
}

/** Where the page's native view goes, in the app page's points from its top left. */
export interface NativeRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A key typed into a page that the app takes (`browser-key`), named as a KeyboardEvent names it. */
export interface BrowserKey {
  id: string;
  key: string;
  code: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  repeat: boolean;
}

/** A tab's view as made, or found open: a parked one's picture (`snapshot`) stands in until it loads again; `dpr`, whether it can report a device's pixel ratio. */
export type BrowserCreated = BrowserState & { snapshot: string | null; dpr: boolean };

/** An element picked on a page (browser/picks.rs), as it goes to an agent. */
export interface BrowserPick {
  selector: string;
  tag: string;
  html: string;
  text: string;
  /** In the page's CSS px, from its viewport's top left. */
  box: { x: number; y: number; w: number; h: number };
  styles: Record<string, string>;
  /** React components it's in, nearest first; none outside React or in a minified build's names. */
  components: string[];
  /** Its picture, a PNG kept a day. */
  screenshot: string | null;
  url: string;
  /** Picked with ⇧ held: one of a few, the picker still on. */
  more?: boolean;
  /** Its place among the picker's picks, from 1: a slow one may come after a later one. */
  number?: number;
}

/** A line the page logged (browser/console.rs); "load" marks where a page began. */
export interface ConsoleEntry {
  level: "error" | "warn" | "load";
  msg: string;
  stack: string;
  url: string;
  ts: number;
}

/** A program in a terminal listening on a port (browser/ports.rs). */
export interface ListeningPort {
  port: number;
  pid: number;
  process: string;
  /** On loopback only, not every interface. */
  loopback: boolean;
}

// A tab's view is made and closed in the order asked: a close right after a create, then a
// reopen (⇧⌘T), must reach the backend as create, close, create, or the reopened tab has none.
const lifecycle = new Map<string, Promise<unknown>>();
function inOrder<T>(id: string, run: () => Promise<T>): Promise<T> {
  const next = (lifecycle.get(id) ?? Promise.resolve()).catch(() => {}).then(run);
  lifecycle.set(id, next);
  void next.catch(() => {}).finally(() => lifecycle.get(id) === next && lifecycle.delete(id));
  return next;
}

/** The browser tab's native view (commands/browser.rs); macOS only for now, the rest fail elsewhere. */
export const browserApi = {
  /**
   * Tab `id`'s view in worktree `root`, loading `url` as the tab shows it (its device's `ua`, `zoom`, `dark`: null for as
   * the app), hidden until placed; one already open stays as it is.
   */
  create: (id: string, root: string, url: string, ua: string | null, zoom: number | null, dark: boolean | null) =>
    inOrder(id, () => invoke<BrowserCreated>("browser_create", { id, root, url, ua, zoom, dark })),
  /** `screen`: the device it shows, in device mode. */
  place: (id: string, rect: NativeRect, screen: NativeScreen | null) => invoke<void>("browser_place", { id, rect, screen }),
  /** The device's user agent (null: WebKit's own), the page loaded again when it changes. */
  setAgent: (id: string, ua: string | null) => invoke<void>("browser_set_agent", { id, ua }),
  /** `aside`: only while something of this page's is drawn over it; otherwise its tab is out of sight, and it may park. */
  hide: (id: string, aside: boolean) => invoke<void>("browser_hide", { id, aside }),
  close: (id: string) => inOrder(id, () => invoke<void>("browser_close", { id })),
  navigate: (id: string, url: string) => invoke<void>("browser_navigate", { id, url }),
  go: (id: string, to: BrowserGo) => invoke<void>("browser_go", { id, to }),
  /** Keys to the page (`page`), or back to the app page. */
  focus: (id: string, page: boolean) => invoke<void>("browser_focus", { id, page }),
  /** The page as it shows, a JPEG data URL; null when there's none. */
  snapshot: (id: string) => invoke<string | null>("browser_snapshot", { id }),
  /** The next match of `text` in the page (or the one before): whether there was one; null where WebKit can't find. */
  find: (id: string, text: string, backwards: boolean, caseSensitive: boolean) => invoke<boolean | null>("browser_find", { id, text, backwards, caseSensitive }),
  /** The page's zoom outside device mode, 1 for 100%. */
  zoom: (id: string, zoom: number) => invoke<void>("browser_zoom", { id, zoom }),
  /** The page light or dark (`prefers-color-scheme` follows), or as the app (null). */
  appearance: (id: string, dark: boolean | null) => invoke<void>("browser_appearance", { id, dark }),
  /** Web Inspector for the page; false where it can't be opened from here. */
  inspect: (id: string) => invoke<boolean>("browser_inspect", { id }),
  /** The chords bound to the app's commands: only these leave a page (keys.rs). */
  setAppKeys: (chords: string[]) => invoke<void>("browser_set_app_keys", { chords }),
  /** Hidden views kept alive, the minutes until one parks (0: never), and the console on or off. */
  configure: (liveHidden: number, parkAfterMin: number, console: boolean, agentControl: boolean) =>
    invoke<void>("browser_configure", { liveHidden, parkAfterMin, console, agentControl }),
  /** The element picker on or off; its pick comes as `browser-picked`. */
  pick: (id: string, on: boolean) => invoke<void>("browser_pick", { id, on }),
  console: (id: string) => invoke<ConsoleEntry[]>("browser_console", { id }),
  consoleClear: (id: string) => invoke<void>("browser_console_clear", { id }),
  /** An agent in `dir` finished: its worktree's pages load again, but those a dev server reloads itself. */
  agentDone: (dir: string) => invoke<void>("browser_agent_done", { dir }),
  /** Cookies, storage and cache of the browser tabs; the app's own are apart. */
  clearData: () => invoke<void>("browser_clear_data"),
  /** What the terminals `ptys` listen on. */
  ports: (ptys: number[]) => invoke<ListeningPort[]>("browser_ports", { ptys }),
};
