import { emitTo } from "@tauri-apps/api/event";
import { github } from "../api";
import { listenHere, OTHER_WINDOW } from "../app/settingsWindow";
import { loadGitHubAccount, loadGitHubStore, saveGitHubAccount, saveGitHubStore } from "../repo/session";
import { refetchGitHub } from "./githubCache";

/** As project `main` opens, before any GitHub call: its calls use the account picked for it. */
export const applyProjectAccount = (main: string) => Promise.all([github.setAccount(loadGitHubAccount(main)), github.allowStore(loadGitHubStore())]);

// Picked in the settings window, the workspace's views read again too (settingsWindow.ts).
const PICKED = "github-account-picked";
listenHere(PICKED, refetchGitHub).catch(() => {});

/** Picks project `main`'s account (null: gh's active one), and reads everything again as it. */
export async function pickAccount(main: string, login: string | null) {
  await github.setAccount(login);
  saveGitHubAccount(main, login);
  refetchGitHub();
  emitTo(OTHER_WINDOW, PICKED).catch(() => {});
}

/**
 * Looks for a GitHub login now, as the user asked to. `store`: in git's credential store too, which
 * the OS may guard with a password dialog; a login found there is remembered, so later sessions
 * look once by themselves. Rejects with GITHUB_NOT_CONNECTED when there is none.
 */
export async function connectGitHub(store: boolean) {
  const source = await github.connect(store);
  if (source === "git") {
    saveGitHubStore(true);
    await github.allowStore(true);
  }
  refetchGitHub();
}
