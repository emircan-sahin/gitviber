import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../api";
import { IS_MAC } from "../platform";
import { getSettings, subscribeSettings, type Translucency } from "../settings";

// The Translucency setting on the window: macOS's material behind the page (vibrancy.rs), then
// the page's chrome see-through over it (index.css, through data-translucency on the root).

// Out of focus, or with Reduce transparency on, the page is solid, as native windows go. Out of
// focus until the window says otherwise: it starts hidden.
let focused = false;
let reduce = false;
// The material the window was last asked for, and whether that newest ask has been answered.
// Each ask and each Reduce transparency question is numbered: an older answer arriving late
// (the main thread was busy) changes nothing.
let material = false;
let ready = false;
let asks = 0;
let checks = 0;
let setting = getSettings().translucency;

const shown = (): Translucency => (IS_MAC && !reduce ? setting : "off");

function apply() {
  const level = shown();
  const on = level !== "off";
  const root = document.documentElement;
  if (on && ready && focused) root.dataset.translucency = level;
  else delete root.dataset.translucency;
  if (!IS_MAC || on === material) return;
  material = on;
  ready = false;
  const ask = ++asks;
  if (on) {
    // The material first, then the see-through page: the other way round, the window's own
    // color shows through for a frame.
    api
      .setTranslucent(true)
      .then(() => {
        if (ask !== asks) return;
        ready = true;
        apply();
      })
      .catch(() => {});
  } else {
    // And away once the solid page has been painted over it. A reload before then is covered by
    // lib.rs, which takes the material off as a page starts loading.
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        if (ask === asks) api.setTranslucent(false).catch(() => {});
      }),
    );
  }
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

function settingsChanged() {
  const was = setting;
  setting = getSettings().translucency;
  if (IS_MAC && was === "off" && setting !== "off") check();
  else apply();
}

function focusChanged(now: boolean) {
  focused = now;
  if (now && setting !== "off") check();
  else apply();
}

const stopSettings = subscribeSettings(settingsChanged);
if (IS_MAC && setting !== "off") check();
let stopFocus: Promise<() => void> = Promise.resolve(() => {});
if (IS_MAC) {
  try {
    const win = getCurrentWindow();
    win.isFocused().then(focusChanged, () => {});
    stopFocus = win.onFocusChanged(({ payload }) => focusChanged(payload));
    stopFocus.catch(() => {});
  } catch {
    // Not in a Tauri window (the browser-only dev fixture).
  }
}
// A hot reload re-runs this module: the old listeners go, or each change would be applied twice.
import.meta.hot?.dispose(() => {
  stopSettings();
  void stopFocus.then((stop) => stop()).catch(() => {});
});
