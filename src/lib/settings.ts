import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow, type Theme as WindowTheme } from "@tauri-apps/api/window";
import type { Whitespace } from "./api";
import { cleanOverrides } from "./commands/commands";
import { IS_MAC, IS_WINDOWS } from "./platform";
import { writeJson } from "./storage";
import { SUGGEST_PRESETS, type SuggestPreset } from "./git/suggest";
import { useSyncExternalStore } from "react";

const CODE_FONTS = {
  "SF Mono": 'ui-monospace, "SF Mono", Menlo, monospace',
  "Geist Mono": '"Geist Mono Variable", ui-monospace, monospace',
  "JetBrains Mono": '"JetBrains Mono Variable", ui-monospace, monospace',
  // Installed with macOS, so nothing to bundle.
  Menlo: "Menlo, ui-monospace, monospace",
  Monaco: "Monaco, ui-monospace, monospace",
  "Courier New": '"Courier New", ui-monospace, monospace',
} as const;
/** "Custom" is any installed font, by the name in `customCodeFont`. */
export type CodeFont = keyof typeof CODE_FONTS | "Custom";

// Only macOS has these; elsewhere they'd render as the system monospace under a wrong name.
const MAC_ONLY_FONTS: readonly CodeFont[] = ["SF Mono", "Menlo", "Monaco"];
/** The presets this platform can show. */
export const codeFontChoices = (Object.keys(CODE_FONTS) as CodeFont[]).filter((f) => IS_MAC || !MAC_ONLY_FONTS.includes(f));
/** SF Mono on macOS; elsewhere a bundled font, so it looks the same on every desktop. */
const DEFAULT_CODE_FONT: keyof typeof CODE_FONTS = IS_MAC ? "SF Mono" : "JetBrains Mono";

// System mirrors index.css's --font-ui.
export const UI_FONTS = {
  System: '-apple-system, BlinkMacSystemFont, "Geist Variable", "Segoe UI", sans-serif',
  Geist: '"Geist Variable", -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
  // Installed with macOS, so nothing to bundle.
  "Helvetica Neue": '"Helvetica Neue", -apple-system, BlinkMacSystemFont, sans-serif',
  "Avenir Next": '"Avenir Next", -apple-system, BlinkMacSystemFont, sans-serif',
} as const;
export type UiFont = keyof typeof UI_FONTS | "Custom";
const MAC_ONLY_UI_FONTS: readonly UiFont[] = ["Helvetica Neue", "Avenir Next"];
/** The presets this platform can show. */
export const uiFontChoices = (Object.keys(UI_FONTS) as UiFont[]).filter((f) => IS_MAC || !MAC_ONLY_UI_FONTS.includes(f));

/** A typed font name without the characters that could break out of a quoted CSS family name. */
export const cleanFontName = (name: string) => name.replace(/["'\\;{}]/g, "").trim();

/** A custom font ahead of the preset: CSS falls through to the preset when it isn't installed. */
const withCustom = (name: string, preset: string) => (name ? `"${name}", ${preset}` : preset);

export const codeFontFamily = (s: Settings) => (s.codeFont === "Custom" ? withCustom(s.customCodeFont, CODE_FONTS[DEFAULT_CODE_FONT]) : CODE_FONTS[s.codeFont]);
export const codeFontName = (s: Settings) => (s.codeFont === "Custom" && s.customCodeFont) || s.codeFont;
const uiFontFamily = (s: Settings) => (s.uiFont === "Custom" ? withCustom(s.customUiFont, UI_FONTS.System) : UI_FONTS[s.uiFont]);

export const SYNTAX_THEMES = {
  "github-dark-default": "GitHub Dark",
  "dark-plus": "VS Code Dark+",
  "one-dark-pro": "One Dark Pro",
  "vitesse-dark": "Vitesse Dark",
  "tokyo-night": "Tokyo Night",
  "solarized-dark": "Solarized Dark",
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

/** A user's own "Open in" entry: a command with {path}, {file} and {line}, run without a shell (open_in.rs). */
export interface CustomApp {
  id: string;
  name: string;
  command: string;
}

export type Appearance = "system" | "light" | "dark";

/** The palettes behind [data-theme] in index.css, each with the syntax theme picking it sets. */
export const THEMES = {
  dark: { label: "Dark", dark: true, syntax: "dark-plus" },
  dim: { label: "Dimmed", dark: true, syntax: "dark-plus" },
  nord: { label: "Nord", dark: true, syntax: "nord" },
  "catppuccin-mocha": { label: "Catppuccin Mocha", dark: true, syntax: "catppuccin-mocha" },
  "tokyo-night": { label: "Tokyo Night", dark: true, syntax: "tokyo-night" },
  "rose-pine": { label: "Rosé Pine", dark: true, syntax: "rose-pine" },
  "solarized-dark": { label: "Solarized Dark", dark: true, syntax: "solarized-dark" },
  light: { label: "Light", dark: false, syntax: "github-light-default" },
  "catppuccin-latte": { label: "Catppuccin Latte", dark: false, syntax: "catppuccin-latte" },
  "rose-pine-dawn": { label: "Rosé Pine Dawn", dark: false, syntax: "rose-pine-dawn" },
  "solarized-light": { label: "Solarized Light", dark: false, syntax: "solarized-light" },
} as const satisfies Record<string, { label: string; dark: boolean; syntax: SyntaxTheme | LightSyntaxTheme }>;
export type Theme = keyof typeof THEMES;
export type DarkTheme = { [K in Theme]: (typeof THEMES)[K]["dark"] extends true ? K : never }[Theme];
export type LightTheme = Exclude<Theme, DarkTheme>;
export const DARK_THEMES = Object.fromEntries(Object.entries(THEMES).filter(([, t]) => t.dark).map(([id, t]) => [id, t.label])) as Record<DarkTheme, string>;
export const LIGHT_THEMES = Object.fromEntries(Object.entries(THEMES).filter(([, t]) => !t.dark).map(([id, t]) => [id, t.label])) as Record<LightTheme, string>;

/** Minutes between background fetches; 0 is off. */
export const FETCH_INTERVALS = [0, 5, 15, 30];

export interface Settings {
  codeFont: CodeFont;
  customCodeFont: string;
  codeFontSize: number;
  lineHeight: number;
  /** System follows the OS between `lightTheme` and `darkTheme`. */
  appearance: Appearance;
  darkTheme: DarkTheme;
  lightTheme: LightTheme;
  uiFont: UiFont;
  customUiFont: string;
  syntaxTheme: SyntaxTheme;
  lightSyntaxTheme: LightSyntaxTheme;
  sideBySide: boolean;
  hideUnchanged: boolean;
  /** Diffs hide lines whose only change is whitespace, of the kind `whitespaceMode` says. */
  ignoreWhitespace: boolean;
  whitespaceMode: Whitespace;
  wordWrap: boolean;
  ligatures: boolean;
  /** Whole-app zoom, one of UI_SCALES. Separate from the code font size. */
  uiScale: number;
  /** Markdown files open rendered rather than as source (diffs always start on the diff). */
  markdownPreview: boolean;
  /** The last Code / Preview choice on an SVG; the next one opens the same way. Set from the viewer, not the dialog. */
  svgPreview: boolean;
  /** The file view shows who last changed each line. Set from the viewer, not the dialog. */
  blame: boolean;
  /** Holding ⌘ by itself shows the shortcuts that apply (ShortcutOverlay); its key command works either way. */
  shortcutOverlay: boolean;
  /** Per-command overrides of the default key bindings; an empty list unbinds. */
  keybindings: Record<string, string[]>;
  /** Repos (main worktree paths) whose commits are signed off: people who sign off always do. Set from the commit box. */
  signOffRepos: string[];
  /** Minutes between quiet fetches of the open repo (one of FETCH_INTERVALS); 0 is off. */
  backgroundFetch: number;
  /** A desktop notification when a push, pull or the like ends while the app is in the background. */
  notify: boolean;
  /** Ask GitHub Releases for a newer GitViber at launch and every few hours (updates.ts). */
  autoUpdate: boolean;
  /** Where the last clone went; the next one offers the same folder. */
  cloneParent: string | null;
  /** New worktrees go in `<worktreeRoot>/<project>/` rather than `<project>.worktrees` beside it; null is off. */
  worktreeRoot: string | null;
  /** A ✦ button in the commit box runs `suggestCommand` for a message. Off: nothing is ever run. */
  suggestEnabled: boolean;
  /** The user's own agent CLI, split like a shell command line (suggest.rs). */
  suggestCommand: string;
  /** Model ids typed per preset; a preset missing here runs its default, so a newer default reaches it. */
  suggestModels: Partial<Record<SuggestPreset, string>>;
  /** The app "Open in" runs on a click: a built-in id or a CustomApp's; "" until one is picked. */
  openInApp: string;
  openInCustom: CustomApp[];
  /** List only the user's own "Open in" entries. */
  openInHideBuiltins: boolean;
}

export const DEFAULT_FONT_SIZE = 12.5;
export const UI_SCALES = [0.8, 0.9, 1, 1.1, 1.2, 1.3, 1.4, 1.5];

const DEFAULTS: Settings = {
  codeFont: DEFAULT_CODE_FONT,
  customCodeFont: "",
  codeFontSize: DEFAULT_FONT_SIZE,
  lineHeight: 1.6,
  appearance: "system",
  darkTheme: "dark",
  lightTheme: "light",
  uiFont: "System",
  customUiFont: "",
  syntaxTheme: "dark-plus",
  lightSyntaxTheme: "github-light-default",
  sideBySide: false,
  hideUnchanged: false,
  ignoreWhitespace: false,
  whitespaceMode: "amount",
  wordWrap: false,
  ligatures: false,
  uiScale: 1,
  markdownPreview: true,
  svgPreview: false,
  blame: false,
  shortcutOverlay: true,
  keybindings: {},
  signOffRepos: [],
  // Off until asked for: a fetch can prompt for an SSH key (1Password, a hardware key) every few minutes.
  backgroundFetch: 0,
  // Off until asked for: turning it on is what asks the OS for permission.
  notify: false,
  autoUpdate: true,
  cloneParent: null,
  worktreeRoot: null,
  suggestEnabled: true,
  suggestCommand: "claude -p",
  suggestModels: {},
  openInApp: "",
  openInCustom: [],
  openInHideBuiltins: false,
};

// v2: the Monaco-era settings had different fonts and sizes.
const KEY = "gitviber.settings.v2";

function load(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    const s = raw ? { ...DEFAULTS, ...JSON.parse(raw) } : DEFAULTS;
    if (!(s.codeFont in CODE_FONTS) && s.codeFont !== "Custom") s.codeFont = DEFAULTS.codeFont;
    if (!IS_MAC && MAC_ONLY_FONTS.includes(s.codeFont)) s.codeFont = DEFAULTS.codeFont;
    if (!(s.uiFont in UI_FONTS) && s.uiFont !== "Custom") s.uiFont = DEFAULTS.uiFont;
    if (!IS_MAC && MAC_ONLY_UI_FONTS.includes(s.uiFont)) s.uiFont = DEFAULTS.uiFont;
    s.customCodeFont = typeof s.customCodeFont === "string" ? cleanFontName(s.customCodeFont) : "";
    s.customUiFont = typeof s.customUiFont === "string" ? cleanFontName(s.customUiFont) : "";
    if (!(s.syntaxTheme in SYNTAX_THEMES)) s.syntaxTheme = DEFAULTS.syntaxTheme;
    if (!(s.lightSyntaxTheme in LIGHT_SYNTAX_THEMES)) s.lightSyntaxTheme = DEFAULTS.lightSyntaxTheme;
    // Before the named themes, Dimmed was an appearance of its own and System's dark a darkVariant.
    const old = s as { appearance: string; darkVariant?: string };
    if (old.appearance === "dim" || (old.appearance === "system" && old.darkVariant === "dim" && s.darkTheme === DEFAULTS.darkTheme)) s.darkTheme = "dim";
    if (old.appearance === "dim") s.appearance = "dark";
    delete old.darkVariant;
    if (!["system", "light", "dark"].includes(s.appearance)) s.appearance = DEFAULTS.appearance;
    if (!(s.darkTheme in DARK_THEMES)) s.darkTheme = DEFAULTS.darkTheme;
    if (!(s.lightTheme in LIGHT_THEMES)) s.lightTheme = DEFAULTS.lightTheme;
    if (!UI_SCALES.includes(s.uiScale)) s.uiScale = DEFAULTS.uiScale;
    if (typeof s.markdownPreview !== "boolean") s.markdownPreview = DEFAULTS.markdownPreview;
    if (typeof s.svgPreview !== "boolean") s.svgPreview = DEFAULTS.svgPreview;
    if (typeof s.blame !== "boolean") s.blame = DEFAULTS.blame;
    if (typeof s.ignoreWhitespace !== "boolean") s.ignoreWhitespace = DEFAULTS.ignoreWhitespace;
    if (!["amount", "all"].includes(s.whitespaceMode)) s.whitespaceMode = DEFAULTS.whitespaceMode;
    if (typeof s.shortcutOverlay !== "boolean") s.shortcutOverlay = DEFAULTS.shortcutOverlay;
    s.keybindings = cleanOverrides(s.keybindings);
    if (typeof s.notify !== "boolean") s.notify = DEFAULTS.notify;
    if (typeof s.autoUpdate !== "boolean") s.autoUpdate = DEFAULTS.autoUpdate;
    if (typeof s.suggestEnabled !== "boolean") s.suggestEnabled = DEFAULTS.suggestEnabled;
    if (typeof s.suggestCommand !== "string") s.suggestCommand = DEFAULTS.suggestCommand;
    const models = s.suggestModels && typeof s.suggestModels === "object" ? s.suggestModels : {};
    s.suggestModels = Object.fromEntries(Object.keys(SUGGEST_PRESETS).filter((k) => typeof models[k] === "string").map((k) => [k, models[k]]));
    s.signOffRepos = Array.isArray(s.signOffRepos) ? s.signOffRepos.filter((p: unknown) => typeof p === "string") : DEFAULTS.signOffRepos;
    if (!FETCH_INTERVALS.includes(s.backgroundFetch)) s.backgroundFetch = DEFAULTS.backgroundFetch;
    if (typeof s.cloneParent !== "string") s.cloneParent = null;
    if (typeof s.worktreeRoot !== "string") s.worktreeRoot = null;
    if (typeof s.openInApp !== "string") s.openInApp = DEFAULTS.openInApp;
    s.openInCustom = Array.isArray(s.openInCustom)
      ? s.openInCustom.filter((c: CustomApp) => c && typeof c.id === "string" && typeof c.name === "string" && typeof c.command === "string")
      : DEFAULTS.openInCustom;
    if (typeof s.openInHideBuiltins !== "boolean") s.openInHideBuiltins = DEFAULTS.openInHideBuiltins;
    return s;
  } catch {
    return DEFAULTS;
  }
}

/** Settings plus what they resolve to right now: `system` follows the OS appearance. */
interface ResolvedSettings extends Settings {
  theme: Theme;
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
  const theme = dark ? current.darkTheme : current.lightTheme;
  return { ...current, theme, dark, codeTheme: dark ? current.syntaxTheme : current.lightSyntaxTheme };
}

/**
 * Linux: the desktop's light or dark, which System sets the window to. There null can't stand for
 * the OS: tao applies it as gtk-application-prefer-dark-theme = false, and WebKitGTK takes
 * prefers-color-scheme from that flag, so a dark Wayland desktop got the light UI. Until the
 * first setTheme, theme() is what tao read from the XDG portal; after that tao passes on the
 * portal's changes (and echoes ours) as theme-changed events. Null elsewhere.
 */
let desktopTheme: Promise<WindowTheme | null> = Promise.resolve(null);
if (!IS_MAC && !IS_WINDOWS) {
  try {
    const win = getCurrentWindow();
    desktopTheme = win.theme().catch(() => null);
    void win
      .onThemeChanged(({ payload }) => {
        if (current.appearance === "system") desktopTheme = Promise.resolve(payload);
      })
      .catch(() => {});
  } catch {
    // Not in a Tauri window.
  }
}

let appliedAppearance: Appearance | null = null;
function applyTheme() {
  document.documentElement.dataset.theme = resolved.theme;
  if (current.appearance === appliedAppearance) return;
  appliedAppearance = current.appearance;
  // Native chrome (traffic lights, dialogs, context menus) follows the window theme; null = OS.
  // getCurrentWindow() throws outside Tauri (the browser-only dev fixture).
  try {
    const win = getCurrentWindow();
    const chosen = current.appearance === "system" ? null : resolved.dark ? "dark" : "light";
    desktopTheme.then((desktop) => win.setTheme(chosen ?? desktop)).catch(() => {});
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

function applyUiFont() {
  document.documentElement.style.setProperty("--font-ui", uiFontFamily(current));
}

function emit() {
  resolved = resolve();
  applyTheme();
  applyScale();
  applyUiFont();
  listeners.forEach((l) => l());
}

applyTheme();
applyScale();
applyUiFont();
systemDark.addEventListener("change", () => current.appearance === "system" && emit());

export function updateSettings(patch: Partial<Settings>) {
  current = { ...current, ...patch };
  current.codeFontSize = Math.min(24, Math.max(10, Math.round(current.codeFontSize * 2) / 2));
  current.customCodeFont = cleanFontName(current.customCodeFont);
  current.customUiFont = cleanFontName(current.customUiFont);
  // Settings still apply for this session when they can't be stored.
  writeJson(KEY, current);
  emit();
}

/**
 * Back to the defaults, except what the user built up rather than chose: the repos they sign
 * off in, their own Open in apps, the folder clones go to.
 */
export function resetSettings() {
  const { signOffRepos, openInCustom, cloneParent } = current;
  updateSettings({ ...DEFAULTS, signOffRepos, openInCustom, cloneParent });
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

/** The whitespace diffs ignore now, or null. */
export const diffWhitespace = (s: Settings): Whitespace | null => (s.ignoreWhitespace ? s.whitespaceMode : null);

/** Snapshot for non-React code such as the global key handler. */
export function getSettings() {
  return resolved;
}

export function useSettings() {
  return useSyncExternalStore(subscribeSettings, getSettings);
}
