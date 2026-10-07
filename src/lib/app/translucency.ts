import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../api";
import { IS_MAC } from "../platform";
import { getSettings, type Settings, subscribeSettings, type Theme, WINDOW_OPACITY } from "../settings";
import { cssVar } from "../ui/color";
import { opacityFloor } from "./opacityFloor";

// Window opacity and Background blur on the window: clear and blurred behind (translucency.rs),
// then the page's surfaces see-through at that opacity (index.css, through data-translucency and
// the --glass levels on the root). At 100% neither happens, so a solid window costs nothing.

/**
 * The page's levels at `opacity` (0-1): the code at it, the bars and side panels a step more
 * opaque so the layout still reads, and `step`, the tint a panel's color adds in the code to reach
 * the bars' level (tabs, file headers, cards).
 */
export function glassLevels(opacity: number) {
  const step = 0.25;
  const pct = (v: number) => `${Math.round(v * 1000) / 10}%`;
  return { glass: pct(opacity), chrome: pct(1 - (1 - opacity) * (1 - step)), step: pct(step) };
}

// With Reduce transparency on, or in full screen (where the desktop is out of sight and the
// window's back shows gray), the window is solid, as with the setting at 100%.
let reduce = false;
let fullscreen = false;
// Whether the window was last asked to be clear, and whether that newest ask has been answered.
// Each ask and each Reduce transparency question is numbered: an older answer arriving late
// (the main thread was busy) changes nothing.
let clear = false;
let ready = false;
let asks = 0;
let checks = 0;
// The blur the window has, and whether a frame is due to send a newer one: a dragged slider
// sends one per frame at most.
let blurSent = 0;
let blurDue = false;
let setting = getSettings();
// A dragged slider's value, shown before it's saved: each save re-renders whatever reads the
// settings, which dropped a drag to a few frames a second.
let preview: Partial<Pick<Settings, "windowOpacity" | "backgroundBlur">> = {};
const live = () => ({ ...setting, ...preview });

/** A palette's text and page colors, from its rule in index.css (the default dark one is plain :root). */
function paletteColors(theme: Theme): [string, string] | null {
  const selector = theme === "dark" ? ":root" : `:root[data-theme="${theme}"]`;
  for (const sheet of document.styleSheets ?? []) {
    let rules: CSSRuleList;
    try {
      rules = sheet.cssRules;
    } catch {
      continue;
    }
    for (const rule of rules) {
      if (rule instanceof CSSStyleRule && rule.selectorText === selector) {
        const [fg, bg] = ["--foreground", "--background"].map((v) => rule.style.getPropertyValue(v).trim());
        if (fg && bg) return [fg, bg];
      }
    }
  }
  return null;
}

/**
 * The lowest opacity (opacityFloor) of the light and the dark theme chosen, the higher of the two:
 * one range for both, so following the system's appearance doesn't move the slider.
 */
export function appearanceFloor() {
  const { lightTheme, darkTheme } = getSettings();
  const key = `${lightTheme} ${darkTheme}`;
  // Read once per pair: a drag asks every frame, and the walk goes through every rule.
  if (floor?.key !== key) {
    const floors = [lightTheme, darkTheme].map((t) => {
      const colors = paletteColors(t) ?? [cssVar("--foreground"), cssVar("--background")];
      return opacityFloor(colors[0], colors[1], WINDOW_OPACITY);
    });
    floor = { key, value: Math.max(...floors) };
  }
  return floor.value;
}
let floor: { key: string; value: number } | null = null;

// Never below the floor; the setting keeps what was chosen, for themes that allow it.
const opacity = () => {
  const { windowOpacity } = live();
  return IS_MAC && !reduce && !fullscreen && windowOpacity < 100 ? Math.max(windowOpacity, appearanceFloor()) : 100;
};

function apply() {
  const shown = opacity();
  const on = shown < 100;
  const root = document.documentElement;
  if (on && ready) {
    const levels = glassLevels(shown / 100);
    root.style.setProperty("--glass", levels.glass);
    root.style.setProperty("--glass-chrome", levels.chrome);
    root.style.setProperty("--glass-step", levels.step);
    root.dataset.translucency = "";
  } else delete root.dataset.translucency;
  if (!IS_MAC) return;
  if (on === clear) {
    if (on && ready) sendBlur();
    return;
  }
  clear = on;
  ready = false;
  const ask = ++asks;
  if (on) {
    // The window first, then the see-through page: the other way round, the window's own
    // color shows through for a frame.
    const blur = live().backgroundBlur;
    api
      .setTranslucent(true, blur)
      .then(() => {
        if (ask !== asks) return;
        ready = true;
        blurSent = blur;
        apply();
      })
      .catch(() => {});
  } else {
    // And solid again once the solid page has been painted. A reload before then is covered by
    // lib.rs, which makes the window solid as a page starts loading.
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        if (ask === asks) api.setTranslucent(false, 0).catch(() => {});
      }),
    );
  }
}

function sendBlur() {
  if (blurDue || live().backgroundBlur === blurSent) return;
  blurDue = true;
  requestAnimationFrame(() => {
    blurDue = false;
    const blur = live().backgroundBlur;
    // Solid by now, or about to be: the next clear ask takes the blur along.
    if (!clear || !ready || blur === blurSent) return;
    blurSent = blur;
    api.setTranslucent(true, blur).catch(() => {});
  });
}

/** Reduce transparency is set in System Settings, so it's asked again on the way back, then applied. */
function check() {
  const n = ++checks;
  api
    .reduceTransparency()
    .then(
      (answer) => {
        if (n === checks) reduce = answer;
      },
      () => {},
    )
    .finally(() => {
      if (n === checks) apply();
    });
}

function changed(was: number) {
  if (IS_MAC && was === 100 && live().windowOpacity < 100) check();
  else apply();
}

function settingsChanged() {
  const was = live().windowOpacity;
  setting = getSettings();
  // The drag's value arrives as the setting.
  preview = {};
  changed(was);
}

/** Shows a slider's value while it's dragged, without saving it; `{}` drops what was shown. */
export function previewTranslucency(next: typeof preview) {
  const was = live().windowOpacity;
  preview = next;
  changed(was);
}

const stopSettings = subscribeSettings(settingsChanged);
if (IS_MAC && setting.windowOpacity < 100) check();
let stops: Promise<() => void>[] = [];
if (IS_MAC) {
  try {
    const win = getCurrentWindow();
    stops = [
      win.onFocusChanged(({ payload }) => {
        if (payload && setting.windowOpacity < 100) check();
      }),
      // Switched in System Settings while this window is in front (translucency.rs).
      win.listen("reduce-transparency", () => {
        if (setting.windowOpacity < 100) check();
      }),
      // titlebar.rs, as each transition starts; the main window's only.
      win.listen<boolean>("fullscreen", ({ payload }) => {
        fullscreen = payload;
        apply();
      }),
    ];
    // A reload while in full screen gets no event.
    win.isFullscreen().then((now) => {
      if (now && !fullscreen) {
        fullscreen = true;
        apply();
      }
    }, () => {});
    stops.forEach((stop) => stop.catch(() => {}));
  } catch {
    // Not in a Tauri window (the browser-only dev fixture).
  }
}
// A hot reload re-runs this module: the old listeners go, or each change would be applied twice.
import.meta.hot?.dispose(() => {
  stopSettings();
  stops.forEach((stop) => void stop.then((f) => f()).catch(() => {}));
});
