# Changelog

Notable changes to GitViber. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). A release's notes
on GitHub are its section below.

## [Unreleased]

## [0.1.4] - 2026-09-27

### Changed

- **⇧⌘↵ zooms the focused pane inside the terminal panel,** keeping the code view and side panels
  in sight, with a button beside Maximize while a tab has more than one pane. ⌘↵ still covers the
  workspace, and the two combine.

## [0.1.3] - 2026-09-27

### Added

- **Split the terminal down as well as right** with ⇧⌘D or the new button beside Split right, to
  build a grid of panes. Drag the dividers to size them; the layout comes back with the session.
- **Move between terminal panes by side** with ⌥⌘ or ⇧⌘ and an arrow. The panes
  other than the focused one dim; Settings → Terminal sets how much, or turns it off.
- **Maximize the terminal** over the whole workspace with ⌘↵ or the toolbar button, or zoom the
  focused pane alone with ⇧⌘↵. The same key or the button brings the workspace back,
  and so do hiding the terminal and opening a file.

### Changed

- **Terminal rows take the font's own height:** a short pane fits a sixth more rows, so Claude
  Code keeps its usage lines and its header.
- **⌥⌘← and ⌥⌘→** focus the pane on that side rather than the previous or next one, which in a
  single row is the same pane. Previous and Next Terminal Pane stay in the palette.

### Fixed

- **⌘1–⌘9 reach every terminal tab** while Claude Code or another program sits idle in the
  terminal; a tab opened since the workspace last redrew was out of reach.
- **A terminal pane you left no longer shows a typing cursor** after switching tabs, splitting
  or zooming, and programs like Claude Code hear that focus left.
- **Closing a dialog puts focus back where it was:** after ⌘, from the terminal you can type
  straight away, and Esc in the palette returns to the file or list you were in.

## [0.1.2] - 2026-09-27

### Added

- **Each worktree's pull request and checks** in the worktree picker and the top bar; the PR's
  number opens it in the app. In #101.
- **Start work on an issue in its own worktree** from the issue view's Start in a worktree…: the
  branch is named after the issue, `{issue}` in the Run command is its number, and a pull
  request from that branch gets `Closes #N` in its body. In #100.
- **Run a command in a new worktree's terminal**, such as `claude`, from New worktree. It's typed
  at the shell's first prompt and remembered for the repository. In #99.
- **`.worktreeinclude`:** a new worktree gets copies of the ignored files it lists, and New
  worktree says how many. In #95.
- **A terminal that needs you** gets a dot on its tab when it rings or a program like Claude Code
  or Codex sends a notification, as do its worktree in the picker and the hidden terminal's top
  bar button. With Notify in the background on (Settings → Git), the OS shows it too. In #90.
- **Shell integration** for zsh and bash 4.4+, with your dotfiles untouched (fish 4 marks its
  own): a dot beside each command, red when it failed, ⌘↑ / ⌘↓ to jump between them, and the
  last one's output to copy or select from a pane's new right-click menu. Settings → Terminal
  turns it off. In #105.
- **A terminal in another project** or any folder, from the menu beside the terminal's +, a
  project's row in the project switcher, or New Terminal in Project… in the palette. Its tab
  shows that project's branch. In #110.
- **Terminal path links** say what ⌘-click does on hover, select the opened file in the explorer,
  open the explorer on a folder, and reach ignored files and shortened `…/a/b.ts` paths. In #109.
- **See why a check failed:** Show failure reads its output, its annotations (each `path:line`
  opens in the code view) and a GitHub Actions job's log tail, and Copy for agent puts it all in
  one block for the terminal. In #102.
- **Pull request titles and descriptions from your agent CLI:** New pull request gets the commit
  box's ✦ button, which reads the branch's commits, its diff and the repository's pull request
  template. In #98.
- **Squash- and rebase-merged branches** whose upstream was deleted count as merged when their
  changes are in the default branch, so Clean up takes them too, checking each again before it
  deletes. In #96.
- **Links to GitHub:** Copy GitHub Link and Open on GitHub, for a file or its selected lines, in
  the code view's and the explorer's right-click menus and the palette. A link points at the
  newest commit origin has, so it keeps showing the same code. In #103.
- **`gitviber src/app.ts:42`** (or `:42:7`) opens the file's repository with the file at that
  line and column, and a file without a line opens in the code view. In #107.
- **Swipe and Onion skin** for a changed image or SVG, beside 2-up. In #108.
- **Hyperlinks that programs print** (OSC 8) open with ⌘-click like detected links and show
  their target on hover; a file link opens only inside the repository. In #93.
- **Programs copy to the clipboard** with OSC 52, as tmux and neovim do, from the terminal pane
  in use only; they can't read it. In #106.
- **Shift+Enter** reaches Claude Code, Codex, neovim and fish 4 as its own key, through the kitty
  keyboard protocol they turn on. In #89.
- **macOS: Option as Meta** in Settings → Terminal, Left ⌥ or Both ⌥, so the shell and agents get
  ⌥ keys like readline's ⌥B and ⌥F. It's off by default. In #91.
- **Rename terminal tabs** with a double-click or the tab's new right-click menu, which also
  splits, clears, and kills the terminal or the others. In #104.
- **⌘Home, ⌘End, ⌘PgUp and ⌘PgDn** scroll the terminal's history.
- **Reveal in Explorer View** from a file tab's right-click menu or the palette.
- **`#123` and `@mentions` in a commit message** link to GitHub.

### Changed

- **Scrolling a terminal while agents print** no longer stalls every 2 s: the session save stores
  a busy pane when it goes quiet or every 30 s, one pane at a time. Part of #97.
- **A split or restored terminal** starts in the folder its shell moved to, not where it opened,
  and relative paths link from there, following a `cd`. In #94.
- **The terminal asks first** before closing a tab or pane, or killing the others, while a
  command runs there, and before pasting several lines into a program that would run each one.
- **While the terminal has focus,** ⌘1–⌘9 and next / previous tab switch its tabs, ⌥← / ⌥→ move
  them, and ⌘A selects its text.
- **Resizing a terminal with a long history** rewraps it once the drag settles.
- **Removing a worktree** says how many terminals run in it.
- **The explorer sorts numbers by value,** `file2` before `file10`, and New File and New Folder
  take a path like `src/a.ts`, making its folders.
- **Updates are checked for** as the app opens, rather than 10 s later.

### Fixed

- **Editing a file** now behaves as in VS Code: brackets and quotes close and Enter indents, ⌘/
  comments lines out instead of opening the shortcut overlay, the matching bracket lights up
  (⇧⌘\ jumps to it), the cursor's line is highlighted, and ⌥Z wraps instead of typing Ω.
- **Quitting** with ⌘Q, the close button or an update's relaunch now saves the terminals' output
  and unsaved edits first; the app could end before they were saved.
- **Emoji and other wide characters** take two cells in the terminal, so programs draw in line and
  the cursor no longer lands a cell off. In #92.
- **A paste into a busy program** froze the window and the other terminal panes.
- **⌘K** cut half the screen out from under a running program such as Claude Code or vim; it now
  leaves it alone.
- **Esc on a confirmation** reached the terminal behind it instead of cancelling.
- **A terminal whose folder is gone** starts in the nearest folder left and says so, and a shell
  that exits at once leaves its pane up with its exit code or signal.
- **A terminal pane closing** pulled focus from wherever you were typing; closed from the panel's
  button, focus now moves on to the next terminal.
- **Diffs that change no text** say what changed: the final newline, the file mode, an empty new
  or deleted file, or a pure rename or copy. A symlink diffs as the path it points to.
- **Next change** got stuck when the last changes shared one screen.
- **UTF-16 files** with a byte order mark show as text, read-only, instead of binary, and saving
  over a file that turned UTF-16 or non-UTF-8 on disk asks first.
- **Unstaging renames and copies:** a rename's old path is unstaged too, a copy's source keeps
  its staged edits, and lines of a renamed file unstage against its old path.
- **Stage, unstage and discard** failed on a very large number of files at once.
- **Mark resolved** asks first when a file still has conflict markers or can't be checked.
- **The commit box** starts from the message a squash merge or `cherry-pick -n` prepared, and a
  multi-line paste into Summary fills the description too.
- **Submodules:** one with changes inside says so and Discard leaves it out, Discard brings back a
  deleted one, and removing a worktree with submodules asks to force instead of failing.
- **A branch deleted on the remote** says so and offers Publish instead of a failing Pull.
- **A checked-out fork pull request** pulls from the fork and no longer offers Publish or New pull
  request.
- **During a rebase** the top bar names the branch being rebased, not detached HEAD.
- **Branch names:** the dialogs show what git will make of a typed name and block one that's taken
  (on macOS, one differing only in case too); they, Set upstream and Lock stay open when git
  refuses.
- **Merge and rebase from the branch picker** offer Retry with autostash when changes are in the
  way.
- **Merging a pull request** uses a method the repository allows, and the menu lists only those.
- **A refused force push** explains why and offers a pull.
- **A 403 from SAML single sign-on** says where to authorize the token.
- **A new origin URL** updates the GitHub account and lists without a reload, and a failed remote
  read no longer empties the GitHub views.

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

[Unreleased]: https://github.com/emircan-sahin/gitviber/compare/v0.1.4...HEAD
[0.1.4]: https://github.com/emircan-sahin/gitviber/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/emircan-sahin/gitviber/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/emircan-sahin/gitviber/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/emircan-sahin/gitviber/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/emircan-sahin/gitviber/releases/tag/v0.1.0
