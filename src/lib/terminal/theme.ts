// The terminal's look: the code view's font, the app's colors (index.css), ANSI colors per light or dark.
import type { ISearchDecorationOptions } from "@xterm/addon-search";
import type { ITerminalOptions } from "@xterm/xterm";
import { codeFontFamily, getSettings } from "../settings";
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

/** Styled like the code view: its font and size, the app's own background and accents. */
export function terminalOptions(): ITerminalOptions {
  const s = getSettings();
  return {
    fontFamily: codeFontFamily(s),
    fontSize: s.codeFontSize,
    // The code view's 1.6 is for reading; TUIs draw box lines that need to touch.
    lineHeight: 1.2,
    theme: {
      ...(s.dark ? ANSI_DARK : ANSI_LIGHT),
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
