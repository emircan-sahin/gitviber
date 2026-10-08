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
}

export type BrowserGo = "back" | "forward" | "reload" | "hardReload" | "stop";

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

/** A tab's view as made, or found open: a parked one's picture (`snapshot`) stands in until it loads again. */
export type BrowserCreated = BrowserState & { snapshot: string | null };

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
  /** Tab `id`'s view in worktree `root`, loading `url`, hidden until placed; one already open stays as it is. */
  create: (id: string, root: string, url: string) => inOrder(id, () => invoke<BrowserCreated>("browser_create", { id, root, url })),
  place: (id: string, rect: NativeRect) => invoke<void>("browser_place", { id, rect }),
  /** `aside`: only while something of this page's is drawn over it; otherwise its tab is out of sight, and it may park. */
  hide: (id: string, aside: boolean) => invoke<void>("browser_hide", { id, aside }),
  close: (id: string) => inOrder(id, () => invoke<void>("browser_close", { id })),
  navigate: (id: string, url: string) => invoke<void>("browser_navigate", { id, url }),
  go: (id: string, to: BrowserGo) => invoke<void>("browser_go", { id, to }),
  /** Keys to the page (`page`), or back to the app page. */
  focus: (id: string, page: boolean) => invoke<void>("browser_focus", { id, page }),
  /** The page as it shows, a JPEG data URL; null when there's none. */
  snapshot: (id: string) => invoke<string | null>("browser_snapshot", { id }),
  /** Web Inspector for the page; false where it can't be opened from here. */
  inspect: (id: string) => invoke<boolean>("browser_inspect", { id }),
  /** The chords bound to the app's commands: only these leave a page (keys.rs). */
  setAppKeys: (chords: string[]) => invoke<void>("browser_set_app_keys", { chords }),
  /** Hidden views kept alive, and the minutes until one parks (0: never). */
  configure: (liveHidden: number, parkAfterMin: number) => invoke<void>("browser_configure", { liveHidden, parkAfterMin }),
  /** An agent in `dir` finished: its worktree's pages load again, but those a dev server reloads itself. */
  agentDone: (dir: string) => invoke<void>("browser_agent_done", { dir }),
  /** Cookies, storage and cache of the browser tabs; the app's own are apart. */
  clearData: () => invoke<void>("browser_clear_data"),
  /** What the terminals `ptys` listen on. */
  ports: (ptys: number[]) => invoke<ListeningPort[]>("browser_ports", { ptys }),
};
