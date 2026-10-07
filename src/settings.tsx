// The settings window's page (settings.html, settings_window.rs): main.tsx's boot without the
// workspace, whose modules listen for the app's events and drive the menu bar as they load.
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "@fontsource-variable/jetbrains-mono";
import "./index.css";
import { StrictMode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createRoot } from "react-dom/client";
import { CrashScreen } from "./components/CrashScreen";
import { SettingsWindow } from "./features/settings/SettingsWindow";
import { installErrorLog, logError } from "./lib/app/errorLog";
import { installScrollbars } from "./lib/app/scrollbars";
import "./lib/app/translucency";

installErrorLog();
installScrollbars();

createRoot(document.getElementById("root")!, {
  onCaughtError: (e, info) => logError("react", e, info.componentStack),
  onUncaughtError: (e, info) => logError("react", e, info.componentStack),
}).render(
  <StrictMode>
    <CrashScreen>
      <SettingsWindow />
    </CrashScreen>
  </StrictMode>,
);

// Hidden until now, as the main window (main.tsx): settings.ts has applied the theme.
getCurrentWindow()
  .show()
  .catch(() => {});
