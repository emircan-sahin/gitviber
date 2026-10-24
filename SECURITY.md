# Security

Please **do not** open a public issue for vulnerabilities.

Report them privately with [GitHub Security Advisories](https://github.com/emircan-sahin/gitviber/security/advisories/new). Include the GitViber version you're running and enough detail to reproduce.

The areas that matter most:

- **Path escapes.** File access must stay inside the opened repository (or the Obsidian vault on show).
- **Token handling.** The GitHub token is borrowed from `gh` or git's credential store and must never be written to disk, logged, or sent anywhere but `api.github.com`.
- **Rendering remote content.** Text from GitHub (PR titles, descriptions, comments) is drawn by the app itself, not shown as a web page, and nothing in it may run. Links open in the browser, and images hosted outside GitHub load only when clicked.
- **Browser tabs.** A page in a browser tab must not reach the app or your files, and an agent must reach only its own terminal pane's tabs, through that pane's key on the browser socket.

GitViber runs the real `git` CLI, so repository hooks run just as they would in your terminal. A malicious repo with hooks is the same risk here as with `git` itself.

## Supported versions

GitViber is before 1.0, so only the [latest release](https://github.com/emircan-sahin/gitviber/releases/latest)
gets security fixes. They ship as a new release, which the app offers to install.
