# Changelog

Notable changes to GitViber. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). A release's notes
on GitHub are its section below.

## [Unreleased]

## [0.1.0] - Unreleased

The first public release. Signed and notarized for macOS (universal: Apple Silicon and Intel),
with `.deb`, `.rpm` and AppImage builds for Linux.

### Worktrees

- Switch worktrees from the top bar, each with its change count, and keep its own terminals.
- Start one from any branch or commit, in the folder you choose, or check a pull request out
  into its own worktree.
- Rename (folder included), move, lock, reveal, remove and prune from the picker, which also
  shows changes, commits ahead and whether the branch is merged.

### Changes and diffs

- Whole-file diffs that refresh as files are saved, unified or split, with word-level
  highlights, collapsed unchanged lines, word wrap and an option to ignore whitespace.
- `J` / `K` walk the changed files and `V` marks one viewed until it changes again.
- Stage, unstage and discard files, a single change or the selected lines, from the list, the
  diff or the keyboard. Discarded versions go to the Trash and can be brought back.
- Review a branch against its base, uncommitted work included.
- Commit with co-authors, sign-off, a template, skipped hooks or amend; the draft survives
  switching tabs. Commit & Push and Sync in one step.
- Commit messages written by your own `claude -p` or `codex exec`, with a model per CLI.
- Resolve conflicts block by block, then continue, skip or abort.
- SVG files and their diffs preview as images.

### History and branches

- A colored lane graph of branches and merges, for one branch, all of them, or a comparison.
- Search commits by message, author, path, content or SHA; file history and blame.
- Undo, revert, reset, cherry-pick (onto another worktree's branch too), tag and check out from a
  commit's menu; anything that rewrites pushed commits asks first. Reword, squash, fix up, move
  or drop a commit, open the reflog, or bisect from there.
- Undo and redo the app's own git actions with `⌘Z`.
- Merge, rebase and pull (fast-forward, merge or rebase, with autostash); create, rename,
  delete and clean up merged branches; set the upstream; stashes; annotated tags.
- Fetch in the background; fetch, pull and push show progress and can be cancelled.
- Common git failures are explained, with the fix one click away.

### Code

- A file explorer with change bars, rename, create, delete and reveal.
- Quick open (`⌘P`), find in files, and Go to Definition / References across the repository.
- `⌘`-click imports, paths, `path:line` and URLs in the code view and the terminal.
- Open the worktree or a file in an installed editor or terminal.

### Terminal

- A real terminal under the diff (`⌘J`) with tabs and splits, running your login shell.

### GitHub

- Pull requests: list, read, review with line comments and replies, create, merge, close,
  reopen and check out, with CI check status. Forks push to origin and open PRs upstream.
- Issues: list, filter by label, read, open, edit, comment, close and reopen.
- Signs in with the login you already have (`gh`, or git's stored credential); nothing is saved.

### App

- Light, dark and dimmed themes, eight syntax themes, your own interface and code fonts, and
  interface scale.
- A command palette (`⇧⌘P`), a native menu bar, and every shortcut rebindable; hold `⌘` to see
  them all.
- First-run checks for a missing or old git and a missing identity; clone or initialize a
  repository from the welcome screen.
- Help → Show Logs and Copy Diagnostics for bug reports. No telemetry.

[Unreleased]: https://github.com/emircan-sahin/gitviber/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/emircan-sahin/gitviber/releases/tag/v0.1.0
