# Privacy

Back to the [README](../README.md).

No telemetry, no API keys, no account. Here is everything GitViber does with the network, your
disk and your logins.

## The network

- The app talks to your git remotes and, for pull requests and issues, to GitHub.
- It checks GitHub Releases for updates at launch and every few hours. Turn it off in
  Settings → Updates.
- Commit message suggestions go wherever the command you picked (`claude`, `codex`) sends them.

## Your GitHub login

GitViber has no sign-in of its own. It borrows a login you already have, the GitHub CLI
(`gh auth token`) first, then git's stored github.com credential, and keeps the token in memory.

## Logs and diagnostics

- Errors, including every error message the app shows you (a failed push's git output, say), go to
  a local file and nowhere else: `~/Library/Logs/app.gitviber.desktop/errors.log` on macOS, or
  Help → Show Logs. Logins in URLs and GitHub tokens are blanked out.
- Help → Copy Diagnostics copies only the versions of GitViber, the OS, git, `gh` and WebKit, for
  you to paste into a bug report.

## Content from the web

- Markdown from GitHub is cut down to GitHub's own HTML allowlist.
- An image hosted outside GitHub loads only when you click it.

## Obsidian

To show your vault, GitViber reads Obsidian's own list of vaults on your disk. It never touches the
vault's `.obsidian` folder.

## Browser tabs

- Tabs keep their own cookies, site storage and cache, apart from the app's, and load pages straight
  from where they're served. Your other browsers' sign-ins don't carry over.
- Settings → Browser → Clear Browsing Data empties them.

## When agents use the browser

Off until you turn it on in Settings → Browser.

- **Tabs.** An agent's tabs each keep their cookies in memory, apart from yours, unless the agent
  asks for yours.
- **Saved sessions.** Logins an agent saves with `gitviber browser session save`, their cookies and
  site storage included, are kept in the app's data folder, readable by you alone, and never printed
  or sent anywhere. Clear Browsing Data deletes them too.
- **Request log.** An agent's tab logs the requests its page makes, headers and bodies included, in
  memory only and only in agents' tabs, for its agent to read.
- **What the agent reads goes where the agent sends it.** A page's text, the elements you pick,
  console lines and requests included.
- **The skill.** GitViber writes a `gitviber-browser` skill for Claude Code and Codex only while
  agent control and its setting are on, and only for an agent that's installed. Your own skill of
  that name is never touched.
- **The socket.** Letting agents use the browser opens a socket on your Mac only, in a folder only
  you can enter, that only GitViber's own terminals have the keys to. Each pane's key reaches only
  that pane's tabs, so an agent that changes which pane it says it's in gets nowhere. Another
  program running as you can still read a pane's key from its environment (`ps -E`), as it could
  read anything else of yours.
