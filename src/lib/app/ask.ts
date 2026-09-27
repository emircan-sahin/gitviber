import { ask as askDialog } from "@tauri-apps/plugin-dialog";

type Options = Exclude<Parameters<typeof askDialog>[1], string | undefined>;

/**
 * The plugin's ask, with Cancel for its "no". macOS binds Esc only to a button titled Cancel: under
 * the default "No", Esc left the dialog up and went to whatever had focus behind it (a terminal).
 */
export const ask = (message: string, options: Options) => askDialog(message, { cancelLabel: "Cancel", ...options });
