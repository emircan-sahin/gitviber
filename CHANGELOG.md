# Changelog

Notable changes to GitViber. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). A release's notes
on GitHub are its section below.

## [Unreleased]

## [0.1.1] - 2026-09-26

### Added

- **Edit and save files** in the file view: type into a file opened from the explorer and save
  it with ⌘S. An unsaved file shows a dot on its tab and asks before closing, edits come back
  after a quit or reload, and a save asks before overwriting a file that changed on disk.
- **Paste images and files into the terminal.** ⌘V pastes a copied image (saved as a PNG) or
  copied Finder files as their paths, and files dropped on a terminal pane paste their paths
  too, so Claude Code, Codex and Gemini attach them.
- **Copy File and Copy Image** in the explorer's and Changes' right-click menus, and on either
  side of an image diff. A copy pastes as a path into a terminal, as a file into Finder or
  Slack, and as a picture into an image app.
- **Select several entries in the explorer** with ⌘-click, ⇧-click or ⇧-arrows, then copy,
  discard or delete them together.
- **Color themes for the whole app:** Nord, Catppuccin Mocha and Latte, Tokyo Night, Rosé Pine
  and Rosé Pine Dawn, and Solarized Dark and Light color the sidebar, panels, diffs and
  terminal. In Settings → Appearance, Theme is System, Light or Dark, with a dark and a light
  theme to pick; each brings its own code colors. In #9.
- **A code font weight:** Light, Regular, Medium (the new default) or Semibold, for the code
  view, diffs and the terminal.
- **Interface fonts:** Helvetica Neue and Avenir Next join the choices on macOS. In #9.
- **Open a repository from outside the app:** run `gitviber .` in a terminal, drop a folder on
  the Dock icon or use Finder's Open With. Install the command from GitViber → Install
  'gitviber' Command; Homebrew installs it for you. In #49.
- **Pull request counts** on the PRs tab's Open and All filters, as Issues has.
- **A Liquid Glass app icon** on macOS 26, with its dark and tinted versions.
- **VS Code's editing keys** in the code view: word, line and multi-cursor commands such as
  ⌥⌫, ⌥↑, ⌘D and ⌘L.

### Changed

- **New installs start on VS Code Dark+** for code colors.
- **Issue and pull request counts** show as badges, like the Changes tab's, and beside a fork's
  pane titles instead of after a long repository name.
- **The terminal lifts very dim colors** until they read, as VS Code's terminal does.
- **The shortcut overlay** leaves out commands that can't run from where the focus is.

### Fixed

- **Files dropped on the terminal** reached no pane on a Retina screen, or the wrong one when
  the interface was zoomed.
- **The code view's cursor** drifted up to two characters by a line's end when the code font
  loaded late.
- **The terminal button's tooltip** showed ⌃` instead of the shortcut set in Settings.
- **Issue counts** were hidden at the panel's default width.
- **Linux: the menu bar took keys** from the terminal and text fields: Ctrl+R reloaded the
  window, a letter binding fired while typing, and F10 opened the menu. In #23.
- **Linux: terminal copy and paste** now use Ctrl+Shift+C and Ctrl+Shift+V, pasting files and
  images too, and a middle-click pastes the selection. In #23.
- **Linux: the terminal** keeps the desktop session's environment, so `xdg-open`, browser
  logins and clipboard tools work in it, and finds the login shell without `$SHELL`. In #23.
- **Linux: Ctrl-click** selects several rows, Trash follows the freedesktop specification on
  other drives, and Show in Folder and links work from the AppImage. In #23.

## [0.1.0] - 2026-09-26

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

[Unreleased]: https://github.com/emircan-sahin/gitviber/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/emircan-sahin/gitviber/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/emircan-sahin/gitviber/releases/tag/v0.1.0
