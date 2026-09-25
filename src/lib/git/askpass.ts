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

/** "git push → github.com": what is asking, so a prompt naming some other host stands out. */
export function promptContext({ label, hosts }: AskPrompt) {
  return hosts.length ? `${label} → ${hosts.join(", ")}` : label;
}

// git's and OpenSSH's English wording (cmd.rs runs git in English; ssh has no translations).
// Every pattern is anchored: the friendly forms are only for text git or ssh wrote whole.
export function promptForm(prompt: AskPrompt): PromptForm {
  const { text, kind, hosts } = prompt;
  const trimmed = text.trim();
  // OpenSSH 8.4+ puts "(user@host) " before whatever the server asks (keyboard-interactive):
  // the server's own words, which may imitate any prompt, so shown as they are.
  const server = /^\(([^\s()]+)\) /.exec(trimmed);
  if (server) return { type: "secret", title: `${server[1].replace(/^.*@/, "")} asks`, label: trimmed };
  if (kind === "none") return { type: "notice", title: "ssh", details: trimmed };
  const hostKey = /^The authenticity of host '([^' ]+)[^\n]*' can't be established\.\n[\s\S]*\(yes\/no[^)\n]*\)\?$/.exec(trimmed);
  if (kind === "confirm" || hostKey || /^[^\n]*\(yes\/no[^)\n]*\)\?$/.test(trimmed)) {
    const lines = trimmed.split("\n");
    const question = lines.pop() ?? "";
    return { type: "confirm", title: hostKey ? `Connect to ${hostKey[1]}?` : "Confirm", details: lines.join("\n").trim(), question, hostKey: !!hostKey };
  }
  // A login for another host than the command's (a hook's, a redirect's) isn't dressed up.
  const known = (host: string) => hosts.includes(host);
  const login = /^(Username|Password) for '([^'\n]+)':$/.exec(trimmed);
  if (login && known(hostOf(login[2]))) {
    const username = login[1] === "Username";
    return { type: username ? "text" : "secret", title: `Sign in to ${hostOf(login[2])}`, label: username ? "Username" : "Password or token" };
  }
  const key = /^Enter passphrase for (?:key )?'([^'\n]+)':$/.exec(trimmed);
  if (key) return { type: "secret", title: "Unlock your SSH key", label: `Passphrase for ${key[1]}` };
  const sshLogin = /^[^\s@']+@([^\s@']+)'s password:$/.exec(trimmed);
  if (sshLogin && known(sshLogin[1])) return { type: "secret", title: `Sign in to ${sshLogin[1]}`, label: "Password" };
  // Unknown words may still ask for a secret (a PIN, a one-time code): masked is the safe side.
  return { type: "secret", title: "git asks", label: trimmed };
}
