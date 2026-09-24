import { invoke } from "@tauri-apps/api/core";

/**
 * The page's errors go to the app's error log (errors.rs), which also prints them on the terminal
 * running `pnpm tauri dev`: the webview's console is rarely open, so errors there went unnoticed.
 */

const consoleError = console.error.bind(console);

// WebKit's `stack` is only the frames, without the message.
const describe = (x: unknown) => {
  if (x instanceof Error) return `${x.name}: ${x.message}${x.stack ? `\n${x.stack}` : ""}`;
  if (typeof x === "string") return x;
  try {
    return JSON.stringify(x);
  } catch {
    return String(x);
  }
};

function send(source: string, parts: unknown[]) {
  try {
    invoke("log_error", { source, message: parts.map(describe).join(" ") }).catch(() => {});
  } catch {
    // Not in Tauri (the browser-only dev fixture).
  }
}

/** Logs an error the page caught itself: to the console as before, and to the error log. */
export function logError(source: string, ...parts: unknown[]) {
  consoleError(`[${source}]`, ...parts);
  send(source, parts);
}

/** What nothing caught: script errors and rejected promises, and in dev React's warnings (keys, hooks), which come through console.error. */
export function installErrorLog() {
  window.addEventListener("error", (e) => send("error", [e.error ?? e.message]));
  window.addEventListener("unhandledrejection", (e) => send("unhandled rejection", [e.reason]));
  if (!import.meta.env.DEV) return;
  console.error = (...args: unknown[]) => {
    consoleError(...args);
    send("console.error", args);
  };
}
