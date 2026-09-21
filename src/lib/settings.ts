import { getCurrentWindow } from "@tauri-apps/api/window";
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
}

export const DEFAULT_FONT_SIZE = 12.5;

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

function emit() {
  resolved = resolve();
  applyTheme();
  listeners.forEach((l) => l());
}

applyTheme();
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

export const getSettings = () => resolved;

export function useSettings() {
  return useSyncExternalStore(subscribeSettings, getSettings);
}

const RECENT_KEY = "gitviber.recent";

export function recentRepos(): string[] {
  try {
    return JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
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
