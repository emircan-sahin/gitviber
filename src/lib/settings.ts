import { emitTo } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow, type Theme as WindowTheme } from "@tauri-apps/api/window";
import type { Whitespace } from "./api";
import { cleanOverrides } from "./commands/commands";
import { listenHere, OTHER_WINDOW } from "./app/settingsWindow";
import { IS_MAC, IS_WINDOWS } from "./platform";
import { writeJson } from "./storage";
import { effortLevels, SUGGEST_PRESETS, type SuggestPreset } from "./git/suggest";
import type { ResumeMode } from "./terminal/agentState";
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
/** The code font's weights, as VS Code's editor.fontWeight; bold text goes two steps up from it. */
export const CODE_FONT_WEIGHTS = { 300: "Light", 400: "Regular", 500: "Medium", 600: "Semibold" } as const;
export type CodeFontWeight = keyof typeof CODE_FONT_WEIGHTS;

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
/** The interface text's weight; medium and semibold labels stay a step above it. */
export const UI_FONT_WEIGHTS = { 400: "Regular", 500: "Medium" } as const;
export type UiFontWeight = keyof typeof UI_FONT_WEIGHTS;
const MAC_ONLY_UI_FONTS: readonly UiFont[] = ["Helvetica Neue", "Avenir Next"];
/** The presets this platform can show. */
export const uiFontChoices = (Object.keys(UI_FONTS) as UiFont[]).filter((f) => IS_MAC || !MAC_ONLY_UI_FONTS.includes(f));

/** A typed font name without the characters that could break out of a quoted CSS family name. */
export const cleanFontName = (name: string) => name.replace(/["'\\;{}]/g, "").trim();

/** A custom font ahead of the preset: CSS falls through to the preset when it isn't installed. */
const withCustom = (name: string, preset: string) => (name ? `"${name}", ${preset}` : preset);

export const codeFontFamily = (s: Settings) => (s.codeFont === "Custom" ? withCustom(s.customCodeFont, CODE_FONTS[DEFAULT_CODE_FONT]) : CODE_FONTS[s.codeFont]);
export const codeFontName = (s: Settings) => (s.codeFont === "Custom" && s.customCodeFont) || s.codeFont;
export const terminalFontFamily = (s: Settings) =>
  s.terminalFont === "Editor" ? codeFontFamily(s) : s.terminalFont === "Custom" ? withCustom(s.customTerminalFont, codeFontFamily(s)) : CODE_FONTS[s.terminalFont];
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

/** How a changed image shows its two versions: next to each other, or one over the other. */
export type ImageCompare = "side" | "swipe" | "onion";

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

/** What ⌥ does in the terminal on macOS: type characters, or send Meta (ESC + the key) from the left ⌥ or either. */
export const OPTION_KEYS = { off: "Off", left: "Left ⌥", both: "Both ⌥" } as const;
export type OptionKey = keyof typeof OPTION_KEYS;

/** How much of the desktop shows through the window's chrome, blurred; macOS only (translucency.ts). */
export const TRANSLUCENCY = { off: "Off", subtle: "Subtle", strong: "Strong" } as const;
export type Translucency = keyof typeof TRANSLUCENCY;

/** Minutes between background fetches; 0 is off. */
export const FETCH_INTERVALS = [0, 5, 15, 30];
/** Seconds a command runs before its end is news (notifyLongCommand). */
export const LONG_COMMAND_SECONDS = [5, 10, 30, 60, 300];

/** Percent a split terminal's other panes fade; 0 is off. */
export const DIM_LEVELS = [0, 10, 20, 35, 50];

/** "Editor" follows the code font; the rest are the code font presets, or Custom by name. */
export type TerminalFont = "Editor" | CodeFont;
export const terminalFontChoices: TerminalFont[] = ["Editor", ...codeFontChoices];
export const TERMINAL_CURSORS = { block: "Block", bar: "Bar", underline: "Underline" } as const;
export type TerminalCursor = keyof typeof TERMINAL_CURSORS;
/** Lines of history each terminal keeps. */
export const SCROLLBACK_LINES = [1_000, 10_000, 50_000, 100_000];
export const TERMINAL_LINE_HEIGHTS = [1, 1.1, 1.2, 1.3];

export interface Settings {
  codeFont: CodeFont;
  customCodeFont: string;
  codeFontSize: number;
  codeFontWeight: CodeFontWeight;
  lineHeight: number;
  /** System follows the OS between `lightTheme` and `darkTheme`. */
  appearance: Appearance;
  darkTheme: DarkTheme;
  lightTheme: LightTheme;
  /** One of TRANSLUCENCY; applied on macOS only, and only while the window is in front. */
  translucency: Translucency;
  uiFont: UiFont;
  customUiFont: string;
  uiFontWeight: UiFontWeight;
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
  /** macOS: whether the terminal's ⌥ is Meta (OPTION_KEYS). */
  optionAsMeta: OptionKey;
  /** New terminals load the shell integration (zsh, bash 4.4+): command marks, ⌘↑ / ⌘↓ between prompts. */
  shellIntegration: boolean;
  /** A restored terminal an agent was running in: its resume command typed at the prompt, run, or neither (agentState.ts). */
  resumeAgents: ResumeMode;
  /** ⌘Q and closing the window ask first while an agent is working, a command runs, or a push or pull is on its way (lib/app/quit). */
  askBeforeQuit: boolean;
  /** How far a split tab's panes other than the focused one fade (one of DIM_LEVELS). */
  terminalInactiveDim: number;
  terminalFont: TerminalFont;
  customTerminalFont: string;
  /** Its own, as in terminal apps: the terminal wants a bigger size than the code view's dense diffs. */
  terminalFontSize: number;
  /** One of TERMINAL_LINE_HEIGHTS. */
  terminalLineHeight: number;
  terminalCursor: TerminalCursor;
  terminalCursorBlink: boolean;
  /** One of SCROLLBACK_LINES. */
  terminalScrollback: number;
  /** xterm's screenReaderMode. Off by default: it keeps a copy of the rows in the page and announces new output. */
  terminalScreenReader: boolean;
  /** Markdown files open rendered rather than as source (diffs always start on the diff). */
  markdownPreview: boolean;
  /** The explorer shows Obsidian's vaults, when Obsidian lists any. */
  obsidian: boolean;
  /** Vaults (by path) the explorer leaves out. */
  hiddenVaults: string[];
  /** The last Code / Preview choice on an SVG; the next one opens the same way. Set from the viewer, not the dialog. */
  svgPreview: boolean;
  /** The last mode picked on a changed image (an SVG preview's too). Set from the viewer, not the dialog. */
  imageCompare: ImageCompare;
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
  /** Desktop notifications while the app is in the background (notify.ts); each NotifyEvent turns its own off. */
  notify: boolean;
  /** An agent in a terminal finished working. */
  notifyAgentDone: boolean;
  /** An agent in a terminal waits for an answer. */
  notifyAgentWaiting: boolean;
  /** A terminal rang its bell or sent a notification escape. */
  notifyTerminal: boolean;
  /** A command shell integration marks ran past `longCommandSeconds` and ended out of sight: its tab is marked, and this notifies. */
  notifyLongCommand: boolean;
  /** One of LONG_COMMAND_SECONDS. */
  longCommandSeconds: number;
  /** A push, pull, fetch, clone or commit ended. */
  notifyGit: boolean;
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
  /** Reasoning effort picked per preset, "" for the CLI's own; a preset missing here runs its default. */
  suggestEfforts: Partial<Record<SuggestPreset, string>>;
  /** Guided reviews' own agent CLI; null runs `suggestCommand`. */
  reviewCommand: string | null;
  /** Guided reviews' own model and effort per preset; a preset missing here runs Commit Messages' (reviewAgent). */
  reviewModels: Partial<Record<SuggestPreset, string>>;
  reviewEfforts: Partial<Record<SuggestPreset, string>>;
  /** The language guided reviews are written in, by name; commit messages and pull requests stay English. */
  reviewLanguage: string;
  /** The app "Open in" runs on a click: a built-in id or a CustomApp's; "" until one is picked. */
  openInApp: string;
  openInCustom: CustomApp[];
  /** List only the user's own "Open in" entries. */
  openInHideBuiltins: boolean;
}

/** What can notify, each with its own switch (Settings → Notifications). */
export const NOTIFY_EVENTS = ["notifyAgentDone", "notifyAgentWaiting", "notifyTerminal", "notifyLongCommand", "notifyGit"] as const;
export type NotifyEvent = (typeof NOTIFY_EVENTS)[number];

export const DEFAULT_FONT_SIZE = 13.5;
export const CODE_FONT_MIN = 10;
export const CODE_FONT_MAX = 24;
export const DEFAULT_TERMINAL_FONT_SIZE = 13;
export const TERMINAL_FONT_MIN = 8;
export const TERMINAL_FONT_MAX = 32;
const clampCodeFont = (size: number) => Math.min(CODE_FONT_MAX, Math.max(CODE_FONT_MIN, Math.round(size * 2) / 2));
const clampTerminalFont = (size: number) => Math.min(TERMINAL_FONT_MAX, Math.max(TERMINAL_FONT_MIN, Math.round(size)));
export const UI_SCALES = [0.8, 0.9, 1, 1.1, 1.2, 1.3, 1.4, 1.5];

const DEFAULTS: Settings = {
  codeFont: DEFAULT_CODE_FONT,
  customCodeFont: "",
  codeFontSize: DEFAULT_FONT_SIZE,
  codeFontWeight: 500,
  lineHeight: 1.6,
  appearance: "system",
  darkTheme: "dark",
  lightTheme: "light",
  translucency: "off",
  uiFont: "System",
  customUiFont: "",
  uiFontWeight: 500,
  syntaxTheme: "dark-plus",
  lightSyntaxTheme: "github-light-default",
  sideBySide: false,
  hideUnchanged: true,
  ignoreWhitespace: false,
  whitespaceMode: "amount",
  wordWrap: false,
  ligatures: false,
  uiScale: 1,
  optionAsMeta: "off",
  shellIntegration: true,
  resumeAgents: "type",
  askBeforeQuit: true,
  terminalInactiveDim: 20,
  terminalFont: "Editor",
  customTerminalFont: "",
  terminalFontSize: DEFAULT_TERMINAL_FONT_SIZE,
  // As Ghostty, iTerm2 and VS Code: at 1.2 a pane had a sixth fewer rows, and Claude Code drops its
  // usage lines, then its header, below ~16 rows.
  terminalLineHeight: 1,
  terminalCursor: "block",
  terminalCursorBlink: true,
  terminalScrollback: 10_000,
  terminalScreenReader: false,
  markdownPreview: true,
  obsidian: true,
  hiddenVaults: [],
  svgPreview: false,
  imageCompare: "side",
  blame: false,
  shortcutOverlay: true,
  keybindings: {},
  signOffRepos: [],
  // Off until asked for: a fetch can prompt for an SSH key (1Password, a hardware key) every few minutes.
  backgroundFetch: 0,
  // Off until asked for: turning it on is what asks the OS for permission.
  notify: false,
  notifyAgentDone: true,
  notifyAgentWaiting: true,
  notifyTerminal: true,
  // Off as in Ghostty and kitty: an update shouldn't start notifying.
  notifyLongCommand: false,
  // Ghostty's is 5 s, for a notification alone; here the tab's dot comes with it.
  longCommandSeconds: 10,
  notifyGit: true,
  autoUpdate: true,
  cloneParent: null,
  worktreeRoot: null,
  suggestEnabled: true,
  suggestCommand: "claude -p",
  suggestModels: {},
  suggestEfforts: {},
  reviewCommand: null,
  reviewModels: {},
  reviewEfforts: {},
  reviewLanguage: "English",
  openInApp: "",
  openInCustom: [],
  openInHideBuiltins: false,
};

// v2: the Monaco-era settings had different fonts and sizes.
const KEY = "gitviber.settings.v2";

/** Model ids typed per preset, the presets there are only. */
function validModels(v: unknown) {
  const models = (v && typeof v === "object" ? v : {}) as Record<string, unknown>;
  return Object.fromEntries(Object.keys(SUGGEST_PRESETS).filter((k) => typeof models[k] === "string").map((k) => [k, models[k]]));
}

/** Efforts picked per preset, each one of its preset's levels or "" (the CLI's own). */
function validEfforts(v: unknown) {
  const efforts = (v && typeof v === "object" ? v : {}) as Record<string, unknown>;
  return Object.fromEntries((Object.keys(SUGGEST_PRESETS) as SuggestPreset[]).filter((k) => efforts[k] === "" || effortLevels(k).includes(efforts[k] as string)).map((k) => [k, efforts[k]]));
}

function load(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    const stored = raw ? JSON.parse(raw) : null;
    const s = stored ? { ...DEFAULTS, ...stored } : DEFAULTS;
    if (!Object.hasOwn(CODE_FONTS, s.codeFont) && s.codeFont !== "Custom") s.codeFont = DEFAULTS.codeFont;
    if (!Object.hasOwn(CODE_FONT_WEIGHTS, s.codeFontWeight)) s.codeFontWeight = DEFAULTS.codeFontWeight;
    if (!IS_MAC && MAC_ONLY_FONTS.includes(s.codeFont)) s.codeFont = DEFAULTS.codeFont;
    if (!Object.hasOwn(UI_FONTS, s.uiFont) && s.uiFont !== "Custom") s.uiFont = DEFAULTS.uiFont;
    if (!IS_MAC && MAC_ONLY_UI_FONTS.includes(s.uiFont)) s.uiFont = DEFAULTS.uiFont;
    if (!Object.hasOwn(UI_FONT_WEIGHTS, s.uiFontWeight)) s.uiFontWeight = DEFAULTS.uiFontWeight;
    if (typeof s.codeFontSize !== "number" || !Number.isFinite(s.codeFontSize)) s.codeFontSize = DEFAULTS.codeFontSize;
    s.codeFontSize = clampCodeFont(s.codeFontSize);
    s.customCodeFont = typeof s.customCodeFont === "string" ? cleanFontName(s.customCodeFont) : "";
    s.customUiFont = typeof s.customUiFont === "string" ? cleanFontName(s.customUiFont) : "";
    if (!Object.hasOwn(SYNTAX_THEMES, s.syntaxTheme)) s.syntaxTheme = DEFAULTS.syntaxTheme;
    if (!Object.hasOwn(LIGHT_SYNTAX_THEMES, s.lightSyntaxTheme)) s.lightSyntaxTheme = DEFAULTS.lightSyntaxTheme;
    // Before the named themes, Dimmed was an appearance of its own and System's dark a darkVariant.
    const old = s as { appearance: string; darkVariant?: string };
    // Dark always meant the graphite dark; Light switched to System later took the variant.
    if (old.appearance === "dim" || (old.appearance !== "dark" && old.darkVariant === "dim" && s.darkTheme === DEFAULTS.darkTheme)) s.darkTheme = "dim";
    if (old.appearance === "dim") s.appearance = "dark";
    delete old.darkVariant;
    if (!["system", "light", "dark"].includes(s.appearance)) s.appearance = DEFAULTS.appearance;
    if (!Object.hasOwn(DARK_THEMES, s.darkTheme)) s.darkTheme = DEFAULTS.darkTheme;
    if (!Object.hasOwn(LIGHT_THEMES, s.lightTheme)) s.lightTheme = DEFAULTS.lightTheme;
    if (!Object.hasOwn(TRANSLUCENCY, s.translucency)) s.translucency = DEFAULTS.translucency;
    if (!UI_SCALES.includes(s.uiScale)) s.uiScale = DEFAULTS.uiScale;
    if (!Object.hasOwn(OPTION_KEYS, s.optionAsMeta)) s.optionAsMeta = DEFAULTS.optionAsMeta;
    if (typeof s.shellIntegration !== "boolean") s.shellIntegration = DEFAULTS.shellIntegration;
    if (!["type", "run", "off"].includes(s.resumeAgents)) s.resumeAgents = DEFAULTS.resumeAgents;
    if (!DIM_LEVELS.includes(s.terminalInactiveDim)) s.terminalInactiveDim = DEFAULTS.terminalInactiveDim;
    if (!terminalFontChoices.includes(s.terminalFont) && s.terminalFont !== "Custom") s.terminalFont = DEFAULTS.terminalFont;
    s.customTerminalFont = typeof s.customTerminalFont === "string" ? cleanFontName(s.customTerminalFont) : "";
    // Up to 0.1.5 the terminal took the code font's size: keep one a user enlarged.
    if (stored && !Object.hasOwn(stored, "terminalFontSize")) s.terminalFontSize = Math.max(DEFAULT_TERMINAL_FONT_SIZE, Math.round(s.codeFontSize));
    if (typeof s.terminalFontSize !== "number" || !Number.isFinite(s.terminalFontSize)) s.terminalFontSize = DEFAULTS.terminalFontSize;
    s.terminalFontSize = clampTerminalFont(s.terminalFontSize);
    if (!TERMINAL_LINE_HEIGHTS.includes(s.terminalLineHeight)) s.terminalLineHeight = DEFAULTS.terminalLineHeight;
    if (!Object.hasOwn(TERMINAL_CURSORS, s.terminalCursor)) s.terminalCursor = DEFAULTS.terminalCursor;
    if (typeof s.terminalCursorBlink !== "boolean") s.terminalCursorBlink = DEFAULTS.terminalCursorBlink;
    if (!SCROLLBACK_LINES.includes(s.terminalScrollback)) s.terminalScrollback = DEFAULTS.terminalScrollback;
    if (typeof s.terminalScreenReader !== "boolean") s.terminalScreenReader = DEFAULTS.terminalScreenReader;
    if (typeof s.markdownPreview !== "boolean") s.markdownPreview = DEFAULTS.markdownPreview;
    if (typeof s.obsidian !== "boolean") s.obsidian = DEFAULTS.obsidian;
    s.hiddenVaults = Array.isArray(s.hiddenVaults) ? s.hiddenVaults.filter((p: unknown) => typeof p === "string") : [];
    if (typeof s.svgPreview !== "boolean") s.svgPreview = DEFAULTS.svgPreview;
    if (!["side", "swipe", "onion"].includes(s.imageCompare)) s.imageCompare = DEFAULTS.imageCompare;
    if (typeof s.blame !== "boolean") s.blame = DEFAULTS.blame;
    if (typeof s.ignoreWhitespace !== "boolean") s.ignoreWhitespace = DEFAULTS.ignoreWhitespace;
    if (!["amount", "all"].includes(s.whitespaceMode)) s.whitespaceMode = DEFAULTS.whitespaceMode;
    if (typeof s.shortcutOverlay !== "boolean") s.shortcutOverlay = DEFAULTS.shortcutOverlay;
    s.keybindings = cleanOverrides(s.keybindings);
    for (const k of ["notify", ...NOTIFY_EVENTS] as const) if (typeof s[k] !== "boolean") s[k] = DEFAULTS[k];
    if (!LONG_COMMAND_SECONDS.includes(s.longCommandSeconds)) s.longCommandSeconds = DEFAULTS.longCommandSeconds;
    if (typeof s.autoUpdate !== "boolean") s.autoUpdate = DEFAULTS.autoUpdate;
    if (typeof s.suggestEnabled !== "boolean") s.suggestEnabled = DEFAULTS.suggestEnabled;
    if (typeof s.suggestCommand !== "string") s.suggestCommand = DEFAULTS.suggestCommand;
    s.suggestModels = validModels(s.suggestModels);
    s.suggestEfforts = validEfforts(s.suggestEfforts);
    if (typeof s.reviewCommand !== "string") s.reviewCommand = null;
    s.reviewModels = validModels(s.reviewModels);
    s.reviewEfforts = validEfforts(s.reviewEfforts);
    if (typeof s.reviewLanguage !== "string") s.reviewLanguage = DEFAULTS.reviewLanguage;
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
  const root = document.documentElement.style;
  root.setProperty("--font-ui", uiFontFamily(current));
  // Tailwind's font-normal/medium/semibold read these: shifted with the base, a label set in
  // medium still stands out from Medium body text.
  const step = current.uiFontWeight - 400;
  for (const [name, weight] of [["normal", 400], ["medium", 500], ["semibold", 600]] as const) root.setProperty(`--font-weight-${name}`, String(weight + step));
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

function apply(patch: Partial<Settings>) {
  current = { ...current, ...patch };
  current.codeFontSize = clampCodeFont(current.codeFontSize);
  current.terminalFontSize = clampTerminalFont(current.terminalFontSize);
  current.customCodeFont = cleanFontName(current.customCodeFont);
  current.customTerminalFont = cleanFontName(current.customTerminalFont);
  current.customUiFont = cleanFontName(current.customUiFont);
  // Settings still apply for this session when they can't be stored.
  writeJson(KEY, current);
  emit();
}

// Each of the two windows (settingsWindow.ts) applies and stores the other's changes as they
// happen: a Tauri event, as the `storage` event isn't promised between two web views on every
// platform. Only the patch goes, so changes to different settings in both at once both stay;
// applied, not passed on. Sends can land out of order, so each setting keeps the newest.
const CHANGED = "settings-changed";
const heard = new Map<string, number>();
listenHere<{ at: number; patch: Partial<Settings> }>(CHANGED, ({ payload: { at, patch } }) => {
  const newer = Object.entries(patch).filter(([key]) => at > (heard.get(key) ?? 0));
  newer.forEach(([key]) => heard.set(key, at));
  if (newer.length) apply(Object.fromEntries(newer));
})
  // What the other window changed before this one listened is in storage by now.
  .then(() => {
    const stored = load();
    if (JSON.stringify(stored) === JSON.stringify(current)) return;
    current = stored;
    emit();
  })
  .catch(() => {});

export function updateSettings(patch: Partial<Settings>) {
  apply(patch);
  // Rises across reloads too, unlike a counter. Kept here too: an older patch from the other
  // window that lands later mustn't take this change back.
  const at = performance.timeOrigin + performance.now();
  Object.keys(patch).forEach((key) => heard.set(key, at));
  emitTo(OTHER_WINDOW, CHANGED, { at, patch }).catch(() => {});
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

/** Moves the terminal font size a point; 0 resets it. */
export function stepTerminalFont(dir: -1 | 0 | 1) {
  updateSettings({ terminalFontSize: dir ? current.terminalFontSize + dir : DEFAULT_TERMINAL_FONT_SIZE });
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
