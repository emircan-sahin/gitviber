import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "@fontsource-variable/jetbrains-mono";
import "./index.css";
import { StrictMode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createRoot } from "react-dom/client";
import { CrashScreen } from "./components/CrashScreen";
import { installErrorLog, logError } from "./lib/app/errorLog";
import { installScrollbars } from "./lib/app/scrollbars";
import { IN_SETTINGS_WINDOW } from "./lib/app/settingsWindow";
import "./lib/app/quit";
import "./lib/app/translucency";

installErrorLog();
installScrollbars();

// Imported, not just rendered, by the window it's for: the workspace's modules listen for the
// app's events and drive the menu bar as they load, which the settings window must not.
const page = IN_SETTINGS_WINDOW
  ? import("./features/settings/SettingsWindow").then(({ SettingsWindow }) => <SettingsWindow />)
  : import.meta.env.DEV && location.search.includes("fixture")
    ? import("./dev-fixture").then(({ Fixture }) => <Fixture />)
    : import("./App").then(({ App }) => <App />);

void page.then((root) => {
  // Render errors, with the component they came from; the boundaries (CrashScreen, the workspace's, a view's) show them.
  createRoot(document.getElementById("root")!, {
    onCaughtError: (e, info) => logError("react", e, info.componentStack),
    onUncaughtError: (e, info) => logError("react", e, info.componentStack),
  }).render(
    <StrictMode>
      <CrashScreen>{root}</CrashScreen>
    </StrictMode>,
  );

  // The window starts hidden (tauri.conf.json, settings_window.rs) so a light theme doesn't flash
  // the native dark background first; settings.ts has applied the theme by now. lib.rs shows it
  // anyway after a delay.
  try {
    getCurrentWindow().show().catch(() => {});
  } catch {
    // Not in a Tauri window (the browser-only dev fixture).
  }
});
