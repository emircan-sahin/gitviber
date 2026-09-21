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

export interface Settings {
  codeFont: CodeFont;
  codeFontSize: number;
  lineHeight: number;
  syntaxTheme: SyntaxTheme;
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
  syntaxTheme: "nord",
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
    return s;
  } catch {
    return DEFAULTS;
  }
}

let current = load();
const listeners = new Set<() => void>();

export function updateSettings(patch: Partial<Settings>) {
  current = { ...current, ...patch };
  current.codeFontSize = Math.min(24, Math.max(10, Math.round(current.codeFontSize * 2) / 2));
  try {
    localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // Settings still apply for this session.
  }
  listeners.forEach((l) => l());
}

export function resetSettings() {
  updateSettings(DEFAULTS);
}

export function useSettings() {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => current,
  );
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
