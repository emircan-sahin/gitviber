import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { cleanOverrides } from "./commands";
import { useSyncExternalStore } from "react";

export const CODE_FONTS = {
  "SF Mono": 'ui-monospace, "SF Mono", Menlo, monospace',
  "Geist Mono": '"Geist Mono Variable", ui-monospace, monospace',
  "JetBrains Mono": '"JetBrains Mono Variable", ui-monospace, monospace',
} as const;
export type CodeFont = keyof typeof CODE_FONTS;

export const SYNTAX_THEMES = {
  "github-dark-default": "GitHub Dark",
  "dark-plus": "VS Code Dark+",
  "one-dark-pro": "One Dark Pro",
  "vitesse-dark": "Vitesse Dark",
  "tokyo-night": "Tokyo Night",
  "catppuccin-mocha": "Catppuccin Mocha",
  vesper: "Vesper",
  houston: "Houston",
  // Muted, low-saturation palettes.
  nord: "Nord",
  "rose-pine": "Rosé Pine",
  "kanagawa-wave": "Kanagawa Wave",
  "everforest-dark": "Everforest Dark",
  poimandres: "Poimandres",
} as const;
export type SyntaxTheme = keyof typeof SYNTAX_THEMES;

export const LIGHT_SYNTAX_THEMES = {
  "github-light-default": "GitHub Light",
  "light-plus": "VS Code Light+",
  "one-light": "One Light",
  "vitesse-light": "Vitesse Light",
  "catppuccin-latte": "Catppuccin Latte",
  "min-light": "Min Light",
  "solarized-light": "Solarized Light",
  "rose-pine-dawn": "Rosé Pine Dawn",
  "kanagawa-lotus": "Kanagawa Lotus",
  "everforest-light": "Everforest Light",
} as const;
export type LightSyntaxTheme = keyof typeof LIGHT_SYNTAX_THEMES;

export type Appearance = "system" | "light" | "dark";

export interface Settings {
  codeFont: CodeFont;
  codeFontSize: number;
  lineHeight: number;
  appearance: Appearance;
  syntaxTheme: SyntaxTheme;
  lightSyntaxTheme: LightSyntaxTheme;
  sideBySide: boolean;
  hideUnchanged: boolean;
  wordWrap: boolean;
  ligatures: boolean;
  /** Whole-app zoom, one of UI_SCALES. Separate from the code font size. */
  uiScale: number;
  /** Markdown files open rendered rather than as source (diffs always start on the diff). */
  markdownPreview: boolean;
  /** Per-command overrides of the default key bindings; an empty list unbinds. */
  keybindings: Record<string, string[]>;
}

export const DEFAULT_FONT_SIZE = 12.5;
export const UI_SCALES = [0.8, 0.9, 1, 1.1, 1.2, 1.3, 1.4, 1.5];

const DEFAULTS: Settings = {
  codeFont: "SF Mono",
  codeFontSize: DEFAULT_FONT_SIZE,
  lineHeight: 1.6,
  appearance: "system",
  syntaxTheme: "nord",
  lightSyntaxTheme: "github-light-default",
  sideBySide: false,
  hideUnchanged: false,
  wordWrap: false,
  ligatures: false,
  uiScale: 1,
  markdownPreview: true,
  keybindings: {},
};

// v2: the Monaco-era settings had different fonts and sizes.
const KEY = "gitviber.settings.v2";

function load(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    const s = raw ? { ...DEFAULTS, ...JSON.parse(raw) } : DEFAULTS;
    if (!(s.codeFont in CODE_FONTS)) s.codeFont = DEFAULTS.codeFont;
    if (!(s.syntaxTheme in SYNTAX_THEMES)) s.syntaxTheme = DEFAULTS.syntaxTheme;
    if (!(s.lightSyntaxTheme in LIGHT_SYNTAX_THEMES)) s.lightSyntaxTheme = DEFAULTS.lightSyntaxTheme;
    if (!["system", "light", "dark"].includes(s.appearance)) s.appearance = DEFAULTS.appearance;
    if (!UI_SCALES.includes(s.uiScale)) s.uiScale = DEFAULTS.uiScale;
    if (typeof s.markdownPreview !== "boolean") s.markdownPreview = DEFAULTS.markdownPreview;
    s.keybindings = cleanOverrides(s.keybindings);
    return s;
  } catch {
    return DEFAULTS;
  }
}

/** Settings plus what they resolve to right now: `system` follows the OS appearance. */
export interface ResolvedSettings extends Settings {
  dark: boolean;
  /** The Shiki theme for the active appearance. */
  codeTheme: SyntaxTheme | LightSyntaxTheme;
}

const systemDark = window.matchMedia("(prefers-color-scheme: dark)");

let current = load();
let resolved = resolve();
const listeners = new Set<() => void>();

function resolve(): ResolvedSettings {
  const dark = current.appearance === "system" ? systemDark.matches : current.appearance === "dark";
  return { ...current, dark, codeTheme: dark ? current.syntaxTheme : current.lightSyntaxTheme };
}

let appliedAppearance: Appearance | null = null;
function applyTheme() {
  document.documentElement.dataset.theme = resolved.dark ? "dark" : "light";
  if (current.appearance === appliedAppearance) return;
  appliedAppearance = current.appearance;
  // Native chrome (traffic lights, dialogs, context menus) follows the window theme; null = OS.
  // getCurrentWindow() throws outside Tauri (the browser-only dev fixture).
  try {
    getCurrentWindow()
      .setTheme(current.appearance === "system" ? null : current.appearance)
      .catch(() => {});
  } catch {
    // Not in a Tauri window.
  }
}

let appliedScale: number | null = null;
function applyScale() {
  if (current.uiScale === appliedScale) return;
  appliedScale = current.uiScale;
  // Native page zoom (WKWebView pageZoom, like Safari's ⌘+): layout, viewport units and
  // pointer coordinates all stay consistent, which CSS `zoom` on the root doesn't guarantee.
  document.documentElement.style.setProperty("--ui-scale", String(current.uiScale));
  try {
    getCurrentWebview()
      .setZoom(current.uiScale)
      .catch(() => {});
  } catch {
    // Not in a Tauri webview.
  }
}

function emit() {
  resolved = resolve();
  applyTheme();
  applyScale();
  listeners.forEach((l) => l());
}

applyTheme();
applyScale();
systemDark.addEventListener("change", () => current.appearance === "system" && emit());

export function updateSettings(patch: Partial<Settings>) {
  current = { ...current, ...patch };
  current.codeFontSize = Math.min(24, Math.max(10, Math.round(current.codeFontSize * 2) / 2));
  try {
    localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // Settings still apply for this session.
  }
  emit();
}

export function resetSettings() {
  updateSettings(DEFAULTS);
}

export function subscribeSettings(l: () => void) {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
}

/** Moves the interface scale one step along UI_SCALES; 0 resets it. */
export function stepUiScale(dir: -1 | 0 | 1) {
  const i = UI_SCALES.indexOf(current.uiScale);
  const next = dir === 0 ? 1 : UI_SCALES[Math.min(UI_SCALES.length - 1, Math.max(0, i + dir))];
  updateSettings({ uiScale: next });
}

/** Snapshot for non-React code such as the global key handler. */
export function getSettings() {
  return resolved;
}

export function useSettings() {
  return useSyncExternalStore(subscribeSettings, getSettings);
}

const RECENT_KEY = "gitviber.recent";

export function recentRepos(): string[] {
  try {
    const list: unknown = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    // Saved lists have held nulls (an undefined path serializes as null), and one null crashed
    // the project switcher's sortable list.
    return Array.isArray(list) ? list.filter((p): p is string => typeof p === "string") : [];
  } catch {
    return [];
  }
}

function saveRepos(list: string[]) {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(list));
  } catch {
    // Not critical.
  }
}

/** Adds a repo to the projects list. The user owns the order: opening never moves it. */
export function rememberRepo(path: string) {
  const list = recentRepos();
  if (!list.includes(path)) saveRepos([...list, path]);
}

export function setRepoOrder(list: string[]) {
  saveRepos(list);
}

/** The repo to reopen on launch (the list order no longer tells). */
const LAST_KEY = "gitviber.last";
export function lastRepo(): string | null {
  try {
    return localStorage.getItem(LAST_KEY);
  } catch {
    return null;
  }
}
export function setLastRepo(path: string) {
  try {
    localStorage.setItem(LAST_KEY, path);
  } catch {
    // Not critical.
  }
}

export function forgetRepo(path: string) {
  saveRepos(recentRepos().filter((p) => p !== path));
}
