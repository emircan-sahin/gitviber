import { github } from "../api";
import { loadGitHubAccount, saveGitHubAccount } from "../repo/session";
import { refetchGitHub } from "./githubCache";

/** As project `main` opens, before any GitHub call: its calls use the account picked for it. */
export const applyProjectAccount = (main: string) => github.setAccount(loadGitHubAccount(main));

/** Picks project `main`'s account (null: gh's active one), and reads everything again as it. */
export async function pickAccount(main: string, login: string | null) {
  await github.setAccount(login);
  saveGitHubAccount(main, login);
  refetchGitHub();
}
