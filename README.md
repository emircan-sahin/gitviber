# GitViber

A fast, readable desktop git client for reviewing what your AI agent just wrote.

- **Plain git.** Every action runs the real `git` CLI with your config, hooks, credentials and signing. No workspace, no extra refs, nothing GitViber-specific is written to your repo.
- **Whole files, not just hunks.** Unified or split diffs of the full file, word-level change highlights, collapsible unchanged regions, next/previous change.
- **Review loop.** J/K walks the changed files, V marks a file viewed; the mark clears itself when the agent edits that file again.
- **File explorer.** Read any file in the repo with VS Code-style change bars, git status marks and ignored files dimmed.
- **Merge, rebase, conflicts.** Merge or rebase from the branch picker, pull with fast-forward/merge/rebase, and resolve conflicts block by block (current / incoming / both / edit) with continue, skip and abort.
- **Pull requests (GitHub).** List, read (checks, description, conversation), review the files in the same full-file viewer, create, merge (merge/squash/rebase), check out, and resolve a conflicting PR locally.
- **Live.** A file watcher refreshes status, diffs and the tree as the agent writes, and keeps your scroll position.
- **Fast and readable.** Native scrolling over a virtualized view, VS Code-grade highlighting (Shiki, off the main thread), SF Mono / Geist Mono / JetBrains Mono, eight syntax themes.

## Stack

Tauri 2 (Rust) + React 19 + shadcn/ui-style components (Radix + Tailwind v4). Diffs are computed in Rust with `similar`; highlighting uses Shiki in a web worker. It uses the OS webview, so no Chromium is bundled.

## Develop

Requires Rust (stable), Node 22+, pnpm and git.

```sh
pnpm install
pnpm tauri dev      # run the app
pnpm typecheck      # frontend types
pnpm check          # everything CI runs: build, rustfmt, clippy, tests
pnpm tauri build    # release bundle
```

## Shortcuts

Every shortcut can be rebound in Settings (⌘,) → Keyboard Shortcuts. The defaults follow VS Code where it has an equivalent. Shortcuts without ⌘ are ignored while you type in a text field.

| Keys | Action |
| --- | --- |
| ⌘, | Settings |
| ⌘1 / ⌘2 / ⌘3 | Changes / History / PRs (left panel) |
| J / K | Next / previous changed file |
| ↓ / ↑, ↵ | Move through the Changes list once it has focus; ↵ keeps the tab open |
| V | Mark file viewed |
| F7 / ⇧F7, ⌥↓ / ⌥↑ | Next / previous change |
| ⌥S / ⌥C / ⌥Z | Split view / collapse unchanged / word wrap |
| ⌘B / ⌥⌘B | Hide or show the git panel / the file explorer |
| ⇧⌘E | Show the file explorer |
| ⌘= / ⌘− / ⌘0 | Zoom the whole interface in / out / reset |
| ⌥⌘= / ⌥⌘− / ⌥⌘0 | Code font size up / down / reset |
| ⌘W | Close tab |
| ⌘↵ | Commit (in the commit message) |
| ⌘R | Refresh |
| ⌘O | Open repository |

## GitHub sign-in

GitViber never asks for or stores a token. For pull requests it borrows the login you already have, in this order:

1. the GitHub CLI (`gh auth token`), if installed and signed in;
2. git's stored github.com credential (macOS Keychain, GitHub Desktop, Git Credential Manager).

If neither exists, the PRs tab shows how to sign in. The token is kept in memory only.

## Privacy

GitViber needs no API keys or environment variables. Network access is limited to your git remotes (`fetch/pull/push`) and, for pull requests, `api.github.com`. File access is limited to the repository you open, and text from GitHub is shown as plain text, never rendered as HTML.

## Contributing

Bug reports and small, focused PRs are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first; it covers setup, where things live, and the few rules the app depends on. Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE)
