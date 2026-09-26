// The terminal's look: the code view's font, the app's colors (index.css), ANSI colors per theme.
import type { ISearchDecorationOptions } from "@xterm/addon-search";
import type { ITerminalOptions } from "@xterm/xterm";
import { codeFontFamily, getSettings, type Theme } from "../settings";
import { cssVar, toHex } from "../ui/color";

const ANSI_DARK = {
  black: "#3a3a3e",
  red: "#f47067",
  green: "#57d17a",
  yellow: "#e2b84c",
  blue: "#4a9ff5",
  magenta: "#b392f0",
  cyan: "#56c8d8",
  white: "#d4d4d8",
  brightBlack: "#6c6c73",
  brightRed: "#ff8a80",
  brightGreen: "#7ee29a",
  brightYellow: "#f0cf74",
  brightBlue: "#74b6f7",
  brightMagenta: "#c9b0f5",
  brightCyan: "#7fdbe6",
  brightWhite: "#ffffff",
};

const ANSI_LIGHT = {
  black: "#1d1d1f",
  red: "#cf222e",
  green: "#1a7f37",
  yellow: "#9a6700",
  blue: "#0969da",
  magenta: "#8250df",
  cyan: "#1b7c83",
  white: "#6e7781",
  brightBlack: "#5b5b62",
  brightRed: "#a40e26",
  brightGreen: "#116329",
  brightYellow: "#7d4e00",
  brightBlue: "#0550ae",
  brightMagenta: "#6639ba",
  brightCyan: "#136061",
  brightWhite: "#8c959f",
};

const NAMES = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"] as const;
/** Eight colors and their eight bright ones, in xterm's order. */
const ansi = (normal: string, bright: string) => {
  const [n, b] = [normal.split(" "), bright.split(" ")];
  return Object.fromEntries(NAMES.flatMap((name, i) => [[name, n[i]], [`bright${name[0].toUpperCase()}${name.slice(1)}`, b[i]]]));
};

// Each named theme's own terminal colors (its VS Code theme's terminal.ansi*).
const SOLARIZED = ansi("#073642 #dc322f #859900 #b58900 #268bd2 #d33682 #2aa198 #eee8d5", "#002b36 #cb4b16 #586e75 #657b83 #839496 #6c71c4 #93a1a1 #fdf6e3");
const ANSI: Partial<Record<Theme, Record<string, string>>> = {
  nord: ansi("#3b4252 #bf616a #a3be8c #ebcb8b #81a1c1 #b48ead #88c0d0 #e5e9f0", "#4c566a #bf616a #a3be8c #ebcb8b #81a1c1 #b48ead #8fbcbb #eceff4"),
  "catppuccin-mocha": ansi("#45475a #f38ba8 #a6e3a1 #f9e2af #89b4fa #f5c2e7 #94e2d5 #a6adc8", "#585b70 #f37799 #89d88b #ebd391 #74a8fc #f2aede #6bd7ca #bac2de"),
  "tokyo-night": ansi("#363b54 #f7768e #73daca #e0af68 #7aa2f7 #bb9af7 #7dcfff #787c99", "#363b54 #f7768e #73daca #e0af68 #7aa2f7 #bb9af7 #7dcfff #acb0d0"),
  "rose-pine": ansi("#26233a #eb6f92 #31748f #f6c177 #9ccfd8 #c4a7e7 #ebbcba #e0def4", "#908caa #eb6f92 #31748f #f6c177 #9ccfd8 #c4a7e7 #ebbcba #e0def4"),
  "solarized-dark": SOLARIZED,
  "catppuccin-latte": ansi("#5c5f77 #d20f39 #40a02b #df8e1d #1e66f5 #ea76cb #179299 #acb0be", "#6c6f85 #de293e #49af3d #eea02d #456eff #fe85d8 #2d9fa8 #bcc0cc"),
  "rose-pine-dawn": ansi("#f2e9e1 #b4637a #286983 #ea9d34 #56949f #907aa9 #d7827e #575279", "#797593 #b4637a #286983 #ea9d34 #56949f #907aa9 #d7827e #575279"),
  "solarized-light": SOLARIZED,
};

/** Styled like the code view: its font and size, the app's own background and accents. */
export function terminalOptions(): ITerminalOptions {
  const s = getSettings();
  return {
    fontFamily: codeFontFamily(s),
    fontSize: s.codeFontSize,
    fontWeight: s.codeFontWeight,
    // Bold stays two steps above whatever the text is, so it still stands out at Semibold.
    fontWeightBold: s.codeFontWeight + 200,
    // The code view's 1.6 is for reading; TUIs draw box lines that need to touch.
    lineHeight: 1.2,
    // As VS Code's terminal does: a theme's own dim colors (Solarized Dark's bright black is its
    // background) are lifted until they read, so autosuggestions and dimmed output show.
    minimumContrastRatio: 4.5,
    theme: {
      ...(ANSI[s.theme] ?? (s.dark ? ANSI_DARK : ANSI_LIGHT)),
      background: cssVar("--background"),
      foreground: cssVar("--foreground"),
      cursor: cssVar("--primary"),
      cursorAccent: cssVar("--background"),
      selectionBackground: `${cssVar("--primary")}55`,
      // --scrollbar-thumb, -hover and -active from index.css: 18%, 36% and 50% of the foreground.
      scrollbarSliderBackground: `${cssVar("--foreground")}2e`,
      scrollbarSliderHoverBackground: `${cssVar("--foreground")}5c`,
      scrollbarSliderActiveBackground: `${cssVar("--foreground")}80`,
    },
  };
}

// The app's find colors (index.css). Matches take solid colors only: the washes are laid on the background here.
export function findColors(): ISearchDecorationOptions {
  const under = cssVar("--background");
  const mark = toHex(cssVar("--find-mark"), under);
  return { matchBackground: toHex(cssVar("--find-match"), under), activeMatchBackground: toHex(cssVar("--find-current"), under), matchOverviewRuler: mark, activeMatchColorOverviewRuler: mark };
}
