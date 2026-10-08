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

/** The browser tab's native view (commands/browser.rs); macOS only for now, the rest fail elsewhere. */
export const browserApi = {
  /** Tab `id`'s view in worktree `root`, loading `url`, hidden until placed; one already open stays as it is. */
  create: (id: string, root: string, url: string) => invoke<BrowserState>("browser_create", { id, root, url }),
  place: (id: string, rect: NativeRect) => invoke<void>("browser_place", { id, rect }),
  hide: (id: string) => invoke<void>("browser_hide", { id }),
  close: (id: string) => invoke<void>("browser_close", { id }),
  /** A removed worktree's pages. */
  closeRoot: (root: string) => invoke<void>("browser_close_root", { root }),
  navigate: (id: string, url: string) => invoke<void>("browser_navigate", { id, url }),
  go: (id: string, to: BrowserGo) => invoke<void>("browser_go", { id, to }),
  /** Keys to the page (`page`), or back to the app page. */
  focus: (id: string, page: boolean) => invoke<void>("browser_focus", { id, page }),
  list: (root: string) => invoke<BrowserState[]>("browser_list", { root }),
  /** The page as it shows, a JPEG data URL; null when there's none. */
  snapshot: (id: string) => invoke<string | null>("browser_snapshot", { id }),
};
