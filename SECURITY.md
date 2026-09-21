# Security

Please **do not** open a public issue for vulnerabilities.

Report them privately with [GitHub Security Advisories](https://github.com/emircan-sahin/gitviber/security/advisories/new). Include the GitViber version you're running and enough detail to reproduce.

The areas that matter most:

- **Path escapes.** File access must stay inside the opened repository.
- **Token handling.** The GitHub token is borrowed from `gh` or git's credential store and must never be written to disk, logged, or sent anywhere but `api.github.com`.
- **Rendering remote content.** Text from GitHub (PR titles, descriptions, comments) must never be rendered as raw HTML or run as script. Markdown is rendered through a sanitizing allowlist, and links open in the browser, never in the app.

GitViber runs the real `git` CLI, so repository hooks run just as they would in your terminal. A malicious repo with hooks is the same risk here as with `git` itself.
