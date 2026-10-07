import { emitTo, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";
import { Toaster } from "@/components/Toaster";
import { TooltipProvider } from "@/components/ui/tooltip";
import { listenHere, runInMain, SETTINGS_WINDOW, SETTINGS_WINDOW_SECTION } from "@/lib/app/settingsWindow";
import { mirrorUpdates, type UpdateMode, useUpdateMode, useUpdates } from "@/lib/app/updates";
import type { UpdateState } from "@/lib/app/updateState";
import { fromPassedKey, useCommands } from "@/lib/commands/keybindings";
import { stepUiScale } from "@/lib/settings";
import { cn } from "@/lib/utils";
import type { Recording } from "./sections/Shortcuts";
import { isSection, type Section, SettingsPanel } from "./SettingsPanel";

/** What of the workspace the settings window shows: the open project, and the updater's state. */
interface Shared {
  main: string | null;
  mode: UpdateMode;
  updates: UpdateState;
}
const SHARED = "settings-window-shared";
const ASK = "settings-window-ask";

const stop = (unlisten: Promise<UnlistenFn>) => void unlisten.then((f) => f()).catch(() => {});

/** In the main window: tells the settings window what it shows of the workspace, as that changes and when it asks. */
export function useShareWithSettingsWindow(main: string | null) {
  const mode = useUpdateMode();
  const updates = useUpdates();
  const shared = useRef<Shared>({ main, mode, updates });
  useEffect(() => {
    shared.current = { main, mode, updates };
    emitTo(SETTINGS_WINDOW, SHARED, shared.current).catch(() => {});
  }, [main, mode, updates]);
  useEffect(() => {
    const unlisten = listenHere(ASK, () => void emitTo(SETTINGS_WINDOW, SHARED, shared.current).catch(() => {}));
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
      // The menu bar's items while this window is in front (settings_window.rs): a click acts on the
      // workspace as ever; a key this page let through is nothing here.
      listenHere<string>("menu", ({ payload }) => fromPassedKey() || runInMain(payload)),
      listenHere<Shared>(SHARED, ({ payload }) => {
        setMain(payload.main);
        mirrorUpdates(payload.mode, payload.updates);
      }).then((f) => {
        emitTo("main", ASK).catch(() => {});
        return f;
      }),
    ];
    return () => unlisten.forEach(stop);
  }, []);
  useCommands({
    "tab.close": () => getCurrentWindow().close(),
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
