import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "@fontsource-variable/jetbrains-mono";
import "./index.css";
import { StrictMode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { CrashScreen } from "./components/CrashScreen";
import { installErrorLog, logError } from "./lib/errorLog";
import { installScrollbars } from "./lib/scrollbars";

import { Fixture } from "./dev-fixture";

installErrorLog();
installScrollbars();

// Render errors, with the component they came from; the boundaries (CrashScreen, the workspace's, a view's) show them.
createRoot(document.getElementById("root")!, {
  onCaughtError: (e, info) => logError("react", e, info.componentStack),
  onUncaughtError: (e, info) => logError("react", e, info.componentStack),
}).render(
  <StrictMode>
    <CrashScreen>{import.meta.env.DEV && location.search.includes("fixture") ? <Fixture /> : <App />}</CrashScreen>
  </StrictMode>,
);

// The window starts hidden (tauri.conf.json) so a light theme doesn't flash the native dark
// background first; settings.ts has applied the theme by now. lib.rs shows it anyway after a delay.
try {
  getCurrentWindow().show().catch(() => {});
} catch {
  // Not in a Tauri window (the browser-only dev fixture).
}

