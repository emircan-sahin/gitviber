import { emitTo, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";
import { Toaster } from "@/components/Toaster";
import { TooltipProvider } from "@/components/ui/tooltip";
import { listenHere, runInMain, SETTINGS_WINDOW, SETTINGS_WINDOW_SECTION } from "@/lib/app/settingsWindow";
import { type Action, fromPassedKey, hasHandler, runCommand, useCommands } from "@/lib/commands/keybindings";
import { stepUiScale } from "@/lib/settings";
import { cn } from "@/lib/utils";
import type { Recording } from "./sections/Shortcuts";
import { isSection, type Section, SettingsPanel } from "./SettingsPanel";

// The project the workspace has open, for the Git section; null with none.
const MAIN = "settings-window-main";
const ASK = "settings-window-ask";

const stop = (unlisten: Promise<UnlistenFn>) => void unlisten.then((f) => f()).catch(() => {});

/** In the main window: tells the settings window the open project, as it changes and when it asks. */
export function useShareWithSettingsWindow(main: string | null) {
  const latest = useRef(main);
  useEffect(() => {
    latest.current = main;
    emitTo(SETTINGS_WINDOW, MAIN, main).catch(() => {});
  }, [main]);
  useEffect(() => {
    const unlisten = listenHere(ASK, () => void emitTo(SETTINGS_WINDOW, MAIN, latest.current).catch(() => {}));
    return () => stop(unlisten);
  }, []);
}

/** The settings window's page (settings_window.rs): Settings alone, kept beside the workspace. */
export function SettingsWindow() {
  const [section, setSection] = useState<Section>(isSection(SETTINGS_WINDOW_SECTION) ? SETTINGS_WINDOW_SECTION : "appearance");
  const [main, setMain] = useState<string | null>(null);
  const [recording, setRecording] = useState<Recording>(null);
  useEffect(() => {
    const unlisten = [
      // ⌘, or a "Settings" button in the main window, once this one is open.
      listenHere<string>("settings-section", ({ payload }) => isSection(payload) && setSection(payload)),
      // The menu bar's items while this window is in front (settings_window.rs): one it has a command
      // for (below) runs here, any other in the main window, brought forward. A key this page let
      // through is nothing here.
      listenHere<string>("menu", ({ payload: id }) => {
        if (fromPassedKey()) return;
        if (hasHandler(id as Action)) runCommand(id as Action);
        else runInMain(id, true);
      }),
      listenHere<string | null>(MAIN, ({ payload }) => setMain(payload)).then((f) => {
        emitTo("main", ASK).catch(() => {});
        return f;
      }),
    ];
    return () => unlisten.forEach(stop);
  }, []);
  useCommands({
    "tab.close": () => getCurrentWindow().close(),
    "window.reload": () => location.reload(),
    "workbench.openSettings": () => {},
    "help.shortcuts": () => setSection("shortcuts"),
    "view.zoomIn": () => stepUiScale(1),
    "view.zoomOut": () => stepUiScale(-1),
    "view.zoomReset": () => stepUiScale(0),
  });
  return (
    <TooltipProvider>
      <div className="flex h-full bg-elevated">
        <SettingsPanel section={section} onSection={setSection} main={main} recording={recording} setRecording={setRecording} Title={Title} titleBar />
      </div>
      <Toaster />
    </TooltipProvider>
  );
}

function Title({ className, children }: { className?: string; children: React.ReactNode }) {
  return <h1 className={cn("text-[13.5px] font-semibold", className)}>{children}</h1>;
}
