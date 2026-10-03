/** How a remote URL signs git in: an SSH key, or a password or token over HTTPS. */
export type RemoteKind = "ssh" | "https" | "other";

export function remoteKind(url: string | null | undefined): RemoteKind {
  const u = url?.trim() ?? "";
  if (/^https?:\/\//i.test(u)) return "https";
  // ssh://git@github.com/o/r, or scp-style git@github.com:o/r (a colon before any slash).
  if (/^ssh:\/\//i.test(u) || /^[^/\s:]+@[^/\s:]+:/.test(u)) return "ssh";
  return "other";
}

/**
 * What the signed-out screen says under its title. An SSH remote signs git in, not the GitHub API
 * that lists pull requests and issues, so the screen says that instead of leaving it to guess.
 */
export function signedOutNote(origin: string | null | undefined, subject: string) {
  if (remoteKind(origin) === "ssh") {
    return `Your remote uses SSH, which only signs git in. ${subject[0].toUpperCase()}${subject.slice(1)} come from GitHub's API, which needs a login of its own.`;
  }
  return "GitViber uses the login you already have. It never stores a token itself.";
}
