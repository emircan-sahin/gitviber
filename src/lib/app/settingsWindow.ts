import { emitTo, type EventCallback, listen } from "@tauri-apps/api/event";

// Settings in a window of its own beside the workspace (settings_window.rs, SettingsWindow.tsx).
// Its page is settings.html?settings=<section>.

/** The section the settings window opened on; null in the main window. */
export const SETTINGS_WINDOW_SECTION = new URLSearchParams(location.search).get("settings");
export const IN_SETTINGS_WINDOW = SETTINGS_WINDOW_SECTION !== null;

export const SETTINGS_WINDOW = "settings";
const HERE = IN_SETTINGS_WINDOW ? SETTINGS_WINDOW : "main";
/** The other of the two windows, for what one tells the other; no one hears it while the settings window is closed. */
export const OTHER_WINDOW = IN_SETTINGS_WINDOW ? "main" : SETTINGS_WINDOW;

/** Listens for what is sent to this window: Tauri hands an `emitTo` to every plain `listen`, in any window. */
export const listenHere = <T>(event: string, handler: EventCallback<T>) => listen(event, handler, { target: { kind: "WebviewWindow", label: HERE } });

/** Has the main window run a menu bar item as if picked there: the workspace and its commands live in it. */
export const runInMain = (id: string) => void emitTo("main", "menu", id).catch(() => {});
