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

// A create still on its way, by tab: a close waits for it, or the view it makes would stay open.
const creating = new Map<string, Promise<unknown>>();

/** The browser tab's native view (commands/browser.rs); macOS only for now, the rest fail elsewhere. */
export const browserApi = {
  /** Tab `id`'s view in worktree `root`, loading `url`, hidden until placed; one already open stays as it is. */
  create: (id: string, root: string, url: string) => {
    const made = invoke<BrowserState>("browser_create", { id, root, url });
    creating.set(id, made);
    void made.catch(() => {}).finally(() => creating.get(id) === made && creating.delete(id));
    return made;
  },
  place: (id: string, rect: NativeRect) => invoke<void>("browser_place", { id, rect }),
  hide: (id: string) => invoke<void>("browser_hide", { id }),
  close: (id: string) =>
    (creating.get(id) ?? Promise.resolve())
      .catch(() => {})
      .then(() => invoke<void>("browser_close", { id })),
  navigate: (id: string, url: string) => invoke<void>("browser_navigate", { id, url }),
  go: (id: string, to: BrowserGo) => invoke<void>("browser_go", { id, to }),
  /** Keys to the page (`page`), or back to the app page. */
  focus: (id: string, page: boolean) => invoke<void>("browser_focus", { id, page }),
  /** The page as it shows, a JPEG data URL; null when there's none. */
  snapshot: (id: string) => invoke<string | null>("browser_snapshot", { id }),
  /** The chords bound to the app's commands: only these leave a page (keys.rs). */
  setAppKeys: (chords: string[]) => invoke<void>("browser_set_app_keys", { chords }),
};
