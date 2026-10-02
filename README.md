<p align="center">
  <img alt="GitViber" src="assets/icon.svg" width="112">
</p>

<h1 align="center">GitViber</h1>

<p align="center">
  <b>Vibe code from one app.</b><br>
  Run your agents in parallel worktrees, watch their diffs land live, browse the code, test it in
  the terminal and commit, without GitHub Desktop, an editor and three terminal windows open.
</p>

<p align="center">
  <a href="https://github.com/emircan-sahin/gitviber/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/emircan-sahin/gitviber?style=flat-square&label=release"></a>
  <a href="https://github.com/emircan-sahin/gitviber/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/emircan-sahin/gitviber/ci.yml?branch=main&style=flat-square&label=CI"></a>
  <img alt="macOS and Linux" src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey?style=flat-square">
  <a href="LICENSE"><img alt="GPL-3.0" src="https://img.shields.io/badge/license-GPL--3.0-blue?style=flat-square"></a>
</p>

![GitViber with a diff, the file explorer and the terminal open](assets/screenshot-dark.png)

<p align="center">
  <a href="https://github.com/emircan-sahin/gitviber/releases/latest"><b>Download for macOS or Linux</b></a>
  ·
  <a href="#install">Install</a>
  ·
  <a href="CHANGELOG.md">What's new</a>
</p>

## Why GitViber

Coding with agents turned the day into reading code. You start Claude Code or Codex in a few
worktrees, then juggle an editor to look at the files, GitHub Desktop to see the diffs, a pile of
terminals to run things and a browser for the pull requests. Editors like Cursor easily eat
gigabytes of RAM just to let you read.

GitViber is one window for all of it, and it's small:

- **14 MB app.** GitHub Desktop is 681 MB. No bundled Chromium, a Rust core and the system webview.
- **Idles at 0% CPU.** It wakes up when files change, not on a timer.
- **Memory it gives back.** Activity Monitor also counts memory the app is done with but WebKit
  hasn't collected yet, so the number climbs while you click around and drops again after a few
  idle minutes. Closing a terminal frees its GPU memory at once.
- **Fast on big diffs.** Diffs are computed in Rust and highlighted off the main thread.
- **Plain git underneath.** Your config, hooks, credentials and signing. Nothing of its own goes into the repo.

## Everything in one window

### Many agents, one screen

Give every agent its own worktree and switch between them from the top bar, each with its change
count, its pull request and how its checks are doing. Start one from any branch, commit or issue,
in the folder you choose, or check a pull request out into its own worktree without touching
yours. A new worktree can run a command such as `claude` in its terminal right away, and gets copies
of the ignored files `.worktreeinclude` lists, like your `.env`. Rename one along with its folder,
lock it, and remove or prune it when the work is merged.
Every worktree keeps its own terminals, so nothing gets lost when you hop between them, and a
terminal can open in another project or any folder too. A tab and its worktree show a ring while
Claude Code works there, and a dot once it finishes or asks for you, with a desktop notification if
you turned those on (Settings → Notifications, one switch per kind); clicking it brings you to that
terminal. Other agents get the dot when they ring the terminal bell or send a notification. Quit
with Claude Code, Gemini CLI or opencode still running, and the restored terminal has the command
that resumes the conversation typed at the prompt.

### Watch the work land

Diffs refresh the moment an agent saves, without losing your scroll position. Unified or split,
word-level highlights, unchanged lines folded away until you want the whole file (`⌥C`). A changed
image shows side by side, as a swipe or as an onion skin. `J` / `K` walk the changed files and `V`
marks one viewed; when the agent touches it again, the mark clears so you know to look again.

### Test it right there

A real terminal sits under the diff (`⌘J`), with tabs and splits. Run the tests, start the dev
server or talk to the agent without leaving the change you're reading. Each pane of a split has a
title bar with what runs there, and `⌘R` names it. The terminal has its own font, size, cursor and
scrollback in Settings, and `⌘=` / `⌘−` in the terminal or a pinch over it size its text.
`⌘`-click a path it prints to open the file at that line, or a folder to show it in the explorer;
a commit's SHA opens it in History, and `#123` or a link to one of the repo's pull requests or
issues opens it in its own tab.
In zsh and bash 4.4+ each command gets a mark, red when it failed, that tells how long it took,
`⌘↑` / `⌘↓` jump between them, and the last one's output copies from the right-click menu; your
dotfiles stay as they are. A command that runs past 10 seconds and ends while you're looking
elsewhere puts a dot on its tab, with a desktop notification if you turned those on.

It's made for agents too: paste a screenshot or drop files and Claude Code or Codex gets their
paths, Shift+Enter reaches them as its own key, and on a Mac a trackpad swipe scrolls Claude
Code's fullscreen view row by row, as it does vim and htop.

### Browse the code, not just the diff

A full file explorer with change bars in the gutter, quick open (`⌘P`) and rename, create and
delete from the right-click menu. Fix a line right in the file view and save it with `⌘S`. For most
days that's the editor you no longer need open.

### Commit messages from your agent

Turn it on in Settings, hit the sparkle next to the commit box, and your own `claude -p` or
`codex exec` writes the message from the diff. New pull request has the same button for the title
and description. No API key, no extra account: it uses the CLI you're already signed in to.

![Claude Code writing a commit message in GitViber](assets/commit-message.gif)

### Good-looking, and yours

Eleven color themes for the whole app, from Nord and Catppuccin to Solarized, 24 syntax themes,
your own code font, terminal font and interface scale, and on macOS a translucent window that lets
the desktop show through, blurred, if you like. Every action has a shortcut, every shortcut
can be rebound in Settings, and holding `⌘` shows them all.

<table>
  <tr>
    <td><img alt="History with the branch graph, light theme" src="assets/screenshot-light.png"></td>
    <td><img alt="Reviewing a pull request" src="assets/screenshot-pr.png"></td>
  </tr>
</table>

### And the rest of git

| | |
| --- | --- |
| **History** | Branches and merges as a colored lane graph. Undo, revert, reset, check out or tag from the right-click menu. Reword, squash, drop or reorder commits: pick several with ⌘- or ⇧-click, drag them between commits to move them or onto one to squash them; anything that rewrites pushed commits asks first |
| **Branches** | Merge, rebase, and pull with fast-forward, merge or rebase. Clean up the merged ones in one go, squash- and rebase-merged included |
| **Conflicts** | Resolve block by block (current, incoming, both, or by hand), then continue, skip or abort |
| **Pull requests** | List, read, review in the same diff viewer, create, merge and check out. Read one commit by commit: pick a commit, or a run of them, to see only what they changed. See why a check failed and copy it for the agent (GitHub) |
| **Issues** | List, read, open, edit, comment, close and reopen, or start one in its own worktree (GitHub) |

## Install

Grab the latest build from [Releases](https://github.com/emircan-sahin/gitviber/releases/latest):

| | |
| --- | --- |
| **macOS** 13 or later | `GitViber_<version>_universal.dmg`, one app for Apple Silicon and Intel, signed and notarized |
| **Linux** x86_64 | `.deb` for Debian and Ubuntu, `.rpm` for Fedora, `.AppImage` for the rest |

Or with Homebrew:

```sh
brew install --cask emircan-sahin/tap/gitviber
```

On Linux:

```sh
sudo apt install ./GitViber_*_amd64.deb          # Debian, Ubuntu
sudo dnf install ./GitViber-*.x86_64.rpm         # Fedora
chmod +x GitViber_*.AppImage && ./GitViber_*.AppImage
```

To open a repository from a terminal, run `gitviber .` (or any path inside one); a file opens in
the code view, at a line with `gitviber src/app.ts:42` or `:42:7`. Homebrew and
the Linux packages put the command on your `PATH`; otherwise use **Install 'gitviber' Command**
in the app menu (File on Linux). A folder dropped on the Dock icon opens too.

GitViber tells you when a new version is out. The macOS app and the AppImage update in place;
a `.deb` or `.rpm` install gets a link to the release to download it from. What changed is in the [changelog](CHANGELOG.md), and every release lists SHA-256
checksums in `SHA256SUMS`.

GitViber is used daily on macOS. The Linux builds are tested in CI, but the app itself is only
starting to get used there ([#35](https://github.com/emircan-sahin/gitviber/issues/35)). Windows
is untested and has no build yet.

### Requirements

- **git 2.36 or newer.** GitViber runs your own git, and says so on first launch if it's missing
  or older. On macOS, `xcode-select --install` or Homebrew's `git` will do.
- **Optional:** the [GitHub CLI](https://cli.github.com) (`gh auth login`) for pull requests and
  issues; git's stored github.com credential works too. `claude` or `codex` if you want commit
  messages written for you.

### Build from source

You need Rust (stable), Node 22+, pnpm and git.

```sh
git clone https://github.com/emircan-sahin/gitviber && cd gitviber
pnpm install
pnpm install:mac    # macOS: builds a release and copies it into /Applications
pnpm tauri build    # Linux: makes a .deb, an .rpm and an AppImage
```

On Linux, Tauri's webview also needs WebKitGTK 4.1 and a C toolchain:

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential        # Debian, Ubuntu
sudo dnf install webkit2gtk4.1-devel gcc                       # Fedora
sudo pacman -S --needed webkit2gtk-4.1 base-devel              # Arch
```

If the AppImage step fails in `linuxdeploy` (its bundled `strip` is too old for current
toolchains, e.g. on Arch), run it with `NO_STRIP=true`.

## Privacy

No telemetry, no API keys. The app talks to your git remotes and, for pull requests and issues,
GitHub. For those it borrows a login you already have, the GitHub CLI (`gh auth token`) first,
then git's stored github.com credential, and keeps the token in memory. GitViber checks GitHub
Releases for updates at launch and every few hours; turn it off in Settings → Updates. Errors, including every
error message the app shows you (a failed push's git output, say), go to a local file
(`~/Library/Logs/app.gitviber.desktop/errors.log` on macOS, Help → Show Logs) and nowhere else,
with logins in URLs and GitHub tokens blanked out. Help → Copy Diagnostics copies only the versions
of GitViber, the OS, git, `gh` and WebKit, for you to paste into a bug report. Markdown from GitHub
is cut down to GitHub's own HTML allowlist, and an image hosted outside GitHub loads only when you
click it. Commit message suggestions go wherever the command you picked sends them.

## FAQ

**Will macOS say it "can't be opened" or "is damaged"?** Not for a release: the app is signed
with a Developer ID and notarized by Apple. If it does, the download didn't come from
[Releases](https://github.com/emircan-sahin/gitviber/releases) or was changed on the way; check
it against `SHA256SUMS` and download it again. A build of your own isn't notarized, but it's
already trusted on the Mac that built it.

**How do I sign in to GitHub?** There's no sign-in of its own. Run `gh auth login` once, or have
git remember a github.com login (any HTTPS push does), and GitViber borrows it. See
[Privacy](#privacy).

**Where are the logs?** Help → Show Logs. They're at `~/Library/Logs/app.gitviber.desktop/` on
macOS and `~/.local/share/app.gitviber.desktop/logs/` on Linux. Help → Copy Diagnostics copies
the versions a bug report needs.

**Where are my settings?** In the app's own storage, never in your repositories:
`~/Library/WebKit/app.gitviber.desktop` on macOS, `~/.local/share/app.gitviber.desktop` on Linux.

**How do I uninstall it?** On macOS, drag GitViber to the Trash (or
`brew uninstall --cask --zap gitviber`, which also removes its settings, caches and logs). On
Linux, `sudo apt remove git-viber` or `sudo dnf remove git-viber`, or delete the AppImage. To
remove the settings by hand, delete the folders above and `~/Library/Caches/app.gitviber.desktop`.

## Shortcuts

<details>
<summary>Every shortcut, and how they map on Linux</summary>

<br>

Every shortcut below except moving around a list or view (arrows, `↵`, `⎋`, `⇧F10`) can be rebound in Settings (`⌘,`).
While you type, only shortcuts with `⌘`, `⌃` or an F-key apply, except `⌘←` `⌘→` and `⌃` with a letter, which edit the text.
On Linux and Windows `⌘` is Ctrl, the views are on Alt+1–4, Ctrl+Tab / Ctrl+Shift+Tab switch tabs, and the terminal's
`⌘T` `⌘D` `⌘K` `⌘W` are Ctrl+Shift+T, D, K and W, since Ctrl+letter stays the shell's (split down is Ctrl+Alt+Shift+D,
zooming a pane Ctrl+Shift+Enter, maximizing Ctrl+Alt+Shift+Enter, the previous / next pane Ctrl+Alt+← / →; focusing a pane by side has no default there, the shell and the desktop use those keys, and neither has renaming a pane, since Ctrl+R is the shell's history search).
Reopening a closed tab and closing the other tabs have no default there either, as Ctrl+Shift+T opens a terminal.

| Keys | |
| --- | --- |
| `⌘,` | Settings |
| Hold `⌘`, or `⌘/` | Show every shortcut, the ones that don't work where you are dimmed (the hold can be turned off in Settings) |
| `⇧⌘P` `⌘P` | Command palette, quick open |
| `⌘F` `⇧⌘F` | Find where focus is, find in files |
| `⌥⌘F`, or `/` in the list | Search the history |
| `⌃1` `⌃2` `⌃3` `⌃4` | Changes, History, PRs, Issues |
| `⌘1` … `⌘8`, `⌘9` | Go to tab 1 to 8, the last tab; the terminal's while it has focus |
| `⇧⌘]` `⇧⌘[`, `⌘→` `⌘←`, `⌃⇥` `⌃⇧⇥` | Next / previous tab; the terminal's while it has focus, though in a terminal pane `⌘←` `⌘→` go to the line's start and end |
| `⌥←` `⌥→` | Move the focused tab left / right, in the code view's tabs or the terminal's |
| `⇧⌘T` `⌥⌘T` | Reopen the closed tab, close the other tabs |
| `J` `K` | Next / previous changed file |
| `↓` `↑` `↵` | Move through the focused list (Changes, History, PRs, Issues), `↵` keeps the tab open; `Home` `End` `PgUp` `PgDn` too |
| `⇧↓` `⇧↑`, `Esc` | Pick a range of rows in Changes or History, let the picked rows go |
| `⇧F10` | The focused row's right-click menu |
| `V` | Mark file viewed |
| `C` | Add a review note on the selected lines, else the cursor's |
| `S` `⌘⌫` | Stage or unstage, discard the open file; in the explorer `⌘⌫` deletes the file |
| `⌘A` | Select every change (in the Changes list) |
| `⌘Z` `⇧⌘Z` | Undo, redo the last git action (outside text fields) |
| `F7` `⇧F7`, `⌥↓` `⌥↑` | Next / previous change |
| `⌥⌘S` `⌥⌘N` `⇧⌘⌫` | Stage, unstage, discard the selected lines, else the change at the cursor (next / previous change puts it there) |
| `⌥S` `⌥C` `⌥W` `⌥Z` | Split view, collapse unchanged, ignore whitespace, word wrap |
| `F12` or `⌘↵`, `⌥F12`, `⇧F12` | Go to definition (`⌘`-click too), peek it, find references (in the code view) |
| `=` `−` `0` | Zoom an image in, out, to fit (in the image view) |
| `⌘S` | Save the file you're editing |
| `F2` | Rename the file, branch or worktree (in the explorer, branch picker or worktree picker) |
| `T` `M` `⌫` | Open a terminal in the worktree, merge its branch, remove it (in the worktree picker) |
| `⇧⌘O` | Open in the external app you last picked |
| `⌘B` `⌥⌘B` | Toggle the git panel, the file explorer |
| `⇧⌘G` `⌘E` `⇧⌘E` | Focus the git panel, the code view, the file explorer |
| `F6` `⇧F6` | Focus the next / previous panel (not from the terminal, which keeps F-keys for its programs) |
| `→` `⎋` | From a file in a list to its code, and back |
| `↑` `↓` `PgUp` `PgDn` `Space` `Home` `End` | Scroll the focused code view, `←` `→` sideways |
| `⌘J` or `⌃` `` ` `` | Toggle the terminal |
| `⌘T` | New terminal |
| `⌘D` `⇧⌘D` `⌘K` | Split right, split down, clear (in the terminal) |
| `⌥⌘←` `⌥⌘→` `⌥⌘↑` `⌥⌘↓` or `⇧⌘` arrows | Focus the terminal pane on that side |
| `⌘↵` | Maximize the terminal over the workspace, and back (in the terminal) |
| `⇧⌘↵` | Show only the focused terminal pane in the panel, and all of them again (in the terminal) |
| `⌘R` | Rename the focused pane of a split terminal (in the terminal; a double-click on its header too) |
| `⌘↑` `⌘↓` | Scroll to the previous / next command's prompt (in the terminal, with shell integration) |
| `⌘=` `⌘−` `⌘0` | Zoom the interface; in the terminal, its font size (a pinch or Ctrl+scroll too) |
| `⌥⌘=` `⌥⌘−` `⌥⌘0` | Code font size |
| `⌘W` | Close tab, or the terminal pane in focus |
| `⌘↵` | Commit (in the commit message) |
| `⇧⌘R` | Reload the window, which ends every terminal |
| `⌘O` | Open repository |

</details>

## Hacking on it

Tauri 2 and Rust on the backend, React 19 with Radix and Tailwind v4 on the front. Diffs are
computed in Rust with `similar`, highlighting is Shiki in a web worker, and the UI runs in the
system webview, so there's no bundled Chromium.

```sh
pnpm tauri dev      # run the app (takes the next free port if 1420 is in use)
pnpm check          # what CI runs: build, tests, rustfmt, clippy
```

Bug reports and small, focused PRs are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) covers setup
and the few rules the app depends on. Report security issues privately, see [SECURITY.md](SECURITY.md).

## License

[GPL-3.0](LICENSE). Use it, change it, share it; a fork you ship has to stay open under the same
license.
