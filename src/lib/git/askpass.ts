import type { AskPrompt } from "../api";

/** How a git or ssh prompt is asked: a field (masked for secrets), a yes/no, or a notice to dismiss. */
export type PromptForm =
  | { type: "text" | "secret"; title: string; label: string }
  | { type: "confirm"; title: string; details: string; question: string; hostKey: boolean }
  | { type: "notice"; title: string; details: string };

/** `https://me@github.com/a/b` → `github.com`. */
function hostOf(url: string) {
  return url.replace(/^[a-z+-]+:\/\//i, "").replace(/^[^@/]*@/, "").split(/[/:]/)[0];
}

// git's and OpenSSH's English wording (cmd.rs runs git in English; ssh has no translations).
export function promptForm({ text, kind }: AskPrompt): PromptForm {
  const trimmed = text.trim();
  if (kind === "none") return { type: "notice", title: "ssh", details: trimmed };
  if (kind === "confirm" || /\(yes\/no[^)]*\)\?$/.test(trimmed)) {
    const lines = trimmed.split("\n");
    const question = lines.pop() ?? "";
    const host = /authenticity of host '([^' ]+)/.exec(trimmed)?.[1];
    return { type: "confirm", title: host ? `Connect to ${host}?` : "Confirm", details: lines.join("\n").trim(), question, hostKey: !!host };
  }
  const login = /^(Username|Password) for '([^']+)'/.exec(trimmed);
  if (login) {
    const username = login[1] === "Username";
    return { type: username ? "text" : "secret", title: `Sign in to ${hostOf(login[2])}`, label: username ? "Username" : "Password or token" };
  }
  const key = /passphrase for (?:key )?'([^']+)'/i.exec(trimmed);
  if (key) return { type: "secret", title: "Unlock your SSH key", label: `Passphrase for ${key[1]}` };
  const sshLogin = /^\S*?@?([^@\s]+)'s password:$/.exec(trimmed);
  if (sshLogin) return { type: "secret", title: `Sign in to ${sshLogin[1]}`, label: "Password" };
  // Unknown words may still ask for a secret (a PIN, a one-time code): masked is the safe side.
  return { type: "secret", title: "git is asking", label: trimmed.replace(/:$/, "") };
}
