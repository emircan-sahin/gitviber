# Changelog

Notable changes to GitViber. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). A release's notes
on GitHub are its section below.

## [Unreleased]

## [0.1.10] - 2026-10-03

### Changed

- **Updates download through a faster route first.** GitViber asks a small mirror for the update and falls back to GitHub if the mirror doesn't answer. GitHub's download server can be very slow for some connections (an update that took minutes, or far longer, to download); the mirror fetches the file from GitHub itself and hands it to you at your connection's speed. Every download is still checked against GitViber's signing key, so the mirror can't serve anything unsigned.

## [0.1.9] - 2026-10-03

### Added

- **A Compare screen.** Pick a base and a compare point (branches, remote branches, tags or a commit id) and see what's ahead and behind, the files that differ (three-dot, or direct) and a **Merge into** button that says first whether the merge would conflict. Open it from the branch picker, History or the command palette. In #115.
- **Open All for a commit.** An expanded commit in History has an **Open All** button that shows every file of the commit in one scroll, like Changes does. In #112.
- **Filters on the Pull requests and Issues tabs.** Chips for created by me, assigned to me, mentions me, review requested and draft or ready, plus labels, kept per repository. In #114.
- **Compare a commit with the working tree or another commit,** from its right-click menu, or with two commits picked. Two files in the Explorer can be compared too.
- **Restore a file from a past commit, or revert its change,** including only the lines you select, with Undo.
- **Copy as Patch and Apply Patch from Clipboard.** Copy a file, selected lines, a commit or a stash as a patch, and apply one from the clipboard after a preview of what it changes.
- **Split a commit, fix staged changes up into an older commit, and cherry-pick several commits** from History. A rewrite can also move the branches stacked on it along.
- **Changes as a folder tree.** Switch Changes between a list and a tree, stage, unstage, discard or stash a whole folder, and sort by what changed most recently.
- **Stash chosen lines, take one file out of a stash, and rename a stash.** Stashing lines never touches what you've staged.
- **Edit the new side of an unstaged diff** and save it with ⌘S.
- **How long each command took.** Hover a command's dot in the terminal for its duration and exit code. A switch in Settings → Notifications (off by default) notifies you when a long command finishes while you're elsewhere, naming only the program.
- **Commit and pull request links in the terminal,** and a keyboard hint mode (⌃⇧Space) that labels every link on screen so you can open or copy one without the mouse.
- **A worktree is named by its branch and its folder,** so a folder whose name no longer matches its branch is obvious.
- **Move main back.** When a linked worktree holds the default branch while the main folder is on another, one confirmed step puts `main` back in the main folder, with Undo.

### Changed

- **Picking a branch another worktree holds** says what Enter will do, and offers to open that worktree or check the branch out detached. After the jump a notice names the folder and offers a way back.
- **Creating a worktree for the default branch** warns once, and offers a new branch instead.
- **Inside a linked worktree,** the project's row in the project list opens the project's main folder.
- **The top bar's Back to the main worktree arrow is gone.** It landed on whatever branch the main folder held.
- **Interface text uses macOS's own font smoothing,** so light text on dark themes is no thinner than in native apps.
- **Clean up's notice says Undo brings back the branches,** not the worktree folders.

### Fixed

- **A Keychain dialog kept appearing on the Pull requests and Issues tabs** when `gh` wasn't signed in, even for repositories cloned over SSH. Those tabs now show a sign-in screen with how to sign in, and only the button you press looks in git's stored logins. In #113.
- **Notification permission:** turning the switch off never asks macOS for anything, a missing permission is explained under the setting without a click, and Send Test always says what happened.
- **Merge messages and Undo labels** name the branch, not `refs/heads/…`, and a branch and a tag of one name no longer swap.
- **A conflict's merge base** shows again when git's default style joins two nearby conflicts.
- **Staging one line of several adjacent changes** no longer shuffles the lines around it.
- **Renaming the top stash** works, and a stash pushed meanwhile is never lost.
- **A merge that brings nothing in** says "Nothing to merge" instead of announcing a squash that made no commit.

## [0.1.8] - 2026-10-02

### Added

- **Mermaid diagrams, math and alerts in markdown.** A ```` ```mermaid ```` block draws its diagram
  (with the app's light or dark theme, and the error plus its source if it can't), `$x^2$` and
  `$$…$$` render as math, and GitHub's `> [!NOTE]` alerts and a file's YAML properties show as they
  do on GitHub. In #111.
- **Your Obsidian vault in the Explorer.** A collapsible Obsidian section at the bottom lists the
  vaults Obsidian knows about. Notes open rendered with wikilinks, embeds, callouts, highlights, tags
  and properties, and can be edited and saved; `.canvas` boards, `.base` files, images, audio, video
  and PDFs open too. Settings → Obsidian turns it off. In #111.
- **Squash, fix up, drop and reorder several commits.** Pick commits in History with ⌘- or
  ⇧-click (or ⇧↑/⇧↓), then squash them into one, fix them up into the oldest or drop them, or drag
  them between commits to move them and onto one to squash them. Anything that rewrites pushed commits
  asks first, and ⌘Z undoes it. In #70.
- **Open in opens the project.** The button at the right of the status bar (⇧⌘O) opens the worktree
  in your editor or terminal, at the file and line you're looking at where the editor can, and each
  app shows its own icon. In #51.
- **Notifications have their own page.** Settings → Notifications has a switch per kind (an agent
  finishes, an agent asks for you, a terminal rings, a push or pull ends), shows whether macOS allows
  them, opens System Settings when it doesn't, and sends a test. Clicking a notification brings you to
  the terminal it was about.
- **A translucent window on macOS.** Settings → Appearance → Translucency lets the desktop show
  through the title bar, the side panels and the status bar; code, diffs and the terminal stay solid.
  In #27.
- **Clean up merged worktrees together.** The worktree picker lists the ones whose branch or pull
  request is merged and that hold nothing of yours, shows the ignored files removing them would
  delete, and removes them and their branches in one step, with Undo.
- **All your agents in one menu.** The Agents menu in the top bar lists every agent in your
  terminals, the ones waiting for you first, and the Dock icon counts them.
- **A color per worktree.** Each worktree gets one, which you can change from its row's menu; it
  shows on the top bar, the picker and the terminal tabs.
- **Resume an older conversation.** Resume a conversation in a worktree's menu lists its past Claude
  Code conversations and continues the one you pick.
- **A GitHub account per repository.** With more than one account in `gh`, Settings → Git picks the
  one a repository's pull requests and issues use.
- **Pinned and recent branches.** The branch picker keeps pinned branches at the top, with the ones
  you checked out last under them.
- **Each conflict's merge base, and a way to hand it on.** A conflict block shows what both sides
  started from, and Ask agent to resolve pastes the files into the agent's terminal. A conflict
  or a diff also opens in the merge or diff tool your git config names.

### Changed

- **The worktree picker shows when a worktree was last worked in,** not when its branch last
  committed.
- **Text sent to a worktree's terminal** goes to the pane an agent is running in, when there is one.

### Fixed

- **Hiding and showing the terminal (⌘J) garbled a running program's screen.** Claude Code was left
  with stray digits and missing letters because the terminal was measured while the panel was still
  laying out; it is now sized once the layout settles.
- **Resolving a conflict in a file without a final newline** no longer adds one.

## [0.1.7] - 2026-10-02

### Added

- **Open every change in one scroll:** Open All on Changes, Staged or a branch review shows each
  file's diff one under the other, live as the agent writes. J/K step through the files, V marks
  the top one viewed, and a click on a fold opens 20 more lines. In #112.
- **Review notes on diff lines.** Select lines in any diff and press C (or right-click → Add
  Review Note) to leave a note. Review Notes in Changes lists them, copies them as a prompt with
  `path:line` and the code, or pastes them into the worktree's terminal without pressing Enter.
  A note turns outdated when the agent rewrites its lines. In #69.
- **Agents resume after a restart.** Quit with Claude Code, Gemini CLI or opencode running, and
  the restored terminal says so and types the command that resumes the conversation at the
  prompt. Settings → Terminal can run it instead, or turn it off.
- **A ring while an agent works, and a note when it's done.** A tab, a split pane and a worktree
  show a ring while Claude Code works there, and a dot with a desktop notification (if turned
  on) once it finishes or asks for you, with no setup in Claude Code.
- **Commit hooks show their output in the top bar and can be cancelled.** A failed hook offers
  committing without hooks.
- **A file over GitHub's 100 MB limit** is caught before the commit, with .gitignore or LFS
  suggested.
- **Fixes one click away:** Remove lock for an index.lock a killed git left behind, Trust this
  folder for a repository git refuses over its owner, and a plain explanation when GitHub refuses
  a push over a secret, with its page and Undo last commit.
- **Interface font weight** in Settings → Appearance.
- **A screen reader mode for the terminal** in Settings → Terminal.

### Changed

- **S stages the open file and moves to the next one,** so S, S, S works down Changes and the
  arrow keys keep working. Unstaging and discarding move on the same way.
- **Interface text is medium weight,** and text on the blue accent (Commit, Push, highlighted
  rows) is dark in the Dark and Dim themes, so both read more easily.
- **The explorer's filter finds ignored files** such as .env, and shows an ignored folder like
  node_modules as one row.
- **A new worktree starts from the current branch,** can check out a branch that already exists,
  and tracks a branch that's only on a remote.
- **The branch picker marks branches checked out in another worktree,** and switching says which
  folder switched. Undo and redo name the branch they switch to, and ask first when that would
  carry uncommitted changes along.
- **Two open files of one name show their folders** in their tabs, and every tab shows its path
  on hover.
- **Toasts stay 5 s, or 10 s with a button,** and Reduce Motion is followed by menus, dialogs,
  tooltips and toasts.

### Fixed

- **The window follows git again after a burst of changes.** A build or a `cargo clean` could
  stop it from showing branch switches, commits and file changes until a restart. Coming back
  to the window now rereads the repository, a hung git read gives up after a minute, and a
  worktree deleted from outside opens its project instead.
- **Copying from the terminal keeps working while Claude Code is running.** ⌥-drag selects past
  a program that reads the mouse, the selection stays when the pointer moves, and ⌘C copies it.
  A program that crashed no longer leaves the mouse captured at the prompt.
- **A program that floods its terminal no longer stalls every pane,** and an agent keeps running
  while the window is hidden.
- **A PR created or merged with gh in the terminal** shows up in the list and on the worktree
  badges.
- **A history search over all branches** picks up commits made in other worktrees and fetched
  ones.
- **Git calls no longer wait forever** when two programs start at the same moment.
- **VoiceOver reads the app:** icon buttons have names, the file tree, Changes and the pickers
  read as lists and trees, settings fields read their labels, and progress reads as progress.
  Keyboard focus stays in the list after staging, and faded text and control edges have enough
  contrast.

## [0.1.6] - 2026-09-28

### Added

- **A pull request's commits, by day,** in a Commits section on its page, with CI, a copyable SHA
  and the full message. Pick one, or ⇧-click for a run, to narrow Files changed to what they
  changed; ↑/↓ and ←/→ step through them. Commits pushed since you last looked are marked New.
  In #33.
- **The terminal has its own font, size, line height, cursor and scrollback** in Settings →
  Terminal, with a live sample. With the terminal focused, ⌘= ⌘− ⌘0 size its font, and so do a
  pinch or Ctrl+scroll over it.
- **Split terminal panes have a title bar** with the program's title (else its folder), the
  needs-you dot and their own split and close buttons, and the divider between panes is easier
  to see.
- **⌘R names the focused pane of a split,** and so does a double-click on its title bar. The name
  is kept with the session.

### Changed

- **Reload Window moves to ⇧⌘R,** so ⌘R pressed by habit in a terminal no longer ends the
  programs running there.
- **A branch checked out in another worktree stays in the branch list,** marked with that
  worktree's name, and picking it opens that worktree. From a linked worktree, main no longer
  disappears from the list.
- **Settings are grouped under General, Workspace and Git,** with their fields in titled cards,
  and keyboard shortcuts sit under category tabs.
- **New installs start with a 13.5 editor font and unchanged lines collapsed** in diffs. The
  terminal's font starts at 13, or at your editor's size if you had made that larger.

### Fixed

- **A trackpad swipe in a program that reads the wheel,** such as Claude Code's fullscreen view,
  vim or htop, scrolls a row for every row swiped on macOS, where a long swipe used to get
  nowhere. In #97.
- **The Linux AppImage starts when another user runs it,** as under firejail, instead of failing
  with "Permission denied".

## [0.1.5] - 2026-09-28

### Changed

- **Closed sections in Changes stay closed** across tab switches and restarts, and move below the
  open ones, just above the commit box, as the panes in Pull Requests and Issues do.

### Fixed

- **Splitting a terminal or closing one of two panes no longer breaks the window,** which 0.1.4
  did as the new zoom button appeared or went away.

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

[Unreleased]: https://github.com/emircan-sahin/gitviber/compare/v0.1.10...HEAD
[0.1.10]: https://github.com/emircan-sahin/gitviber/compare/v0.1.9...v0.1.10
[0.1.9]: https://github.com/emircan-sahin/gitviber/compare/v0.1.8...v0.1.9
[0.1.8]: https://github.com/emircan-sahin/gitviber/compare/v0.1.7...v0.1.8
[0.1.7]: https://github.com/emircan-sahin/gitviber/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/emircan-sahin/gitviber/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/emircan-sahin/gitviber/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/emircan-sahin/gitviber/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/emircan-sahin/gitviber/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/emircan-sahin/gitviber/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/emircan-sahin/gitviber/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/emircan-sahin/gitviber/releases/tag/v0.1.0
