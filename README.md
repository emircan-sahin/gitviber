# GitViber

A fast, readable desktop git client for reviewing what your AI agents just wrote, one worktree or many.

![GitViber reviewing a diff, dark theme](assets/screenshot-dark.png)

- **Plain git.** Every action runs the real `git` CLI with your config, hooks, credentials and signing. No workspace, no extra refs, nothing GitViber-specific is written to your repo.
- **Whole files, not just hunks.** Unified or split diffs of the full file, word-level change highlights, collapsible unchanged regions, next/previous change.
- **Review loop.** J/K walks the changed files, V marks a file viewed; the mark clears itself when the agent edits that file again.
- **Worktrees for parallel agents.** Switch between a project's worktrees from the top bar, each with its change count. A branch that's checked out elsewhere opens that worktree instead of failing, and one that isn't can get its own worktree beside the project in one step. Worktrees and repos nested inside the project show up in Changes with their branch and are kept out of staging, so `git add` never turns them into gitlinks.
- **Terminal built in.** Your login shell in a panel under the diff (⌘J), with tabs and splits. Terminals belong to a worktree: open one in any worktree or on any branch, and switching worktrees brings its terminals back.
- **File explorer.** Read any file in the repo with VS Code-style change bars, git status marks and ignored files dimmed.
- **Merge, rebase, conflicts.** Merge or rebase from the branch picker, pull with fast-forward/merge/rebase, and resolve conflicts block by block (current / incoming / both / edit) with continue, skip and abort.
- **Pull requests (GitHub).** List, read (checks, description, conversation), review the files in the same full-file viewer, create, merge (merge/squash/rebase), check out, and resolve a conflicting PR locally.
- **Live.** A file watcher refreshes status, diffs and the tree as the agent writes, and keeps your scroll position.
- **Fast and readable.** Native scrolling over a virtualized view that renders only what's on screen, 120Hz on ProMotion displays, VS Code-grade highlighting (Shiki, off the main thread), SF Mono / Geist Mono / JetBrains Mono, eight syntax themes.
- **Light and dark.** System, Light or Dark appearance with a separate syntax theme for each; the native window chrome follows along.

![GitViber reviewing a diff, light theme](assets/screenshot-light.png)

![A GitHub pull request in GitViber: merge status, checks and changed files](assets/screenshot-pr.png)

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

| Keys | Action |
| --- | --- |
| ⌘1 / ⌘2 / ⌘3 | Changes / History / PRs (left panel) |
| J / K | Next / previous changed file |
| V | Mark file viewed |
| ⌥↓ / ⌥↑ | Next / previous change |
| ⌥S / ⌥C / ⌥Z | Split view / collapse unchanged / word wrap |
| ⌘B / ⌥⌘B | Hide or show the git panel / the file explorer |
| ⇧⌘E | Show the file explorer |
| ⌘+ / ⌘− / ⌘0 | Code font size |
| ⌘W | Close tab (in a terminal: close that pane) |
| ⌘J / ⌃` | Show or hide the terminal panel |
| ⌘T / ⌘D / ⌘K | New terminal / split / clear (terminal focused) |
| ⌘↵ | Commit |
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
