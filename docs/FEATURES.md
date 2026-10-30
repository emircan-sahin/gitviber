# Features

Back to the [README](../README.md).

This is the full reference: every feature, with the details the
README leaves out. Keys are for macOS; [the shortcut list](SHORTCUTS.md) has the Linux ones.

- [Worktrees and agents](#worktrees-and-agents)
- [Guided review](#guided-review)
- [Diffs and review](#diffs-and-review)
- [The terminal](#the-terminal)
- [The browser](#the-browser)
- [Code and notes](#code-and-notes)
- [Commit messages from your agent](#commit-messages-from-your-agent)
- [Git](#git)
- [Looks and settings](#looks-and-settings)
- [Size, memory and speed](#size-memory-and-speed)

---

## Worktrees and agents

Give every agent its own worktree and switch between them from the top bar.

### Starting and managing worktrees

- **Switcher.** Each worktree shows its change count, its pull request and how its checks are doing.
  The top bar names a worktree by its branch and its folder, so a folder that no longer says what it
  holds shows.
- **Start from anywhere.** From any branch or commit, in the folder you choose, or from an issue. Or
  check a pull request out into its own worktree without touching yours. Settings can put new
  worktrees under a folder of your own.
- **Setup.** A new worktree can run a command such as `claude` in its terminal right away, and gets
  copies of the ignored files `.worktreeinclude` lists, like your `.env`. The submodules you have set
  up come along, copied from your clone rather than downloaded again.
- **Organize.** Rename one along with its folder, give it a color its chip and picker row wear,
  lock it, and remove or prune it when the work is merged.
- **Clean up.** One action removes every merged worktree with nothing uncommitted, branches too,
  after showing the ignored files that go with them, like `node_modules`. A locked one, or one with
  a terminal running, stays, and the toast after it offers Undo. Merging a pull request in the app
  offers to remove its worktree.
- **One branch, one worktree.** Git keeps a branch in one worktree, so picking one that another holds
  says where it is: Enter opens that worktree (with a way back), and the row's menu checks the commit
  out here detached, or starts a new branch from it. When a linked worktree holds the default branch
  and the main folder is on another, the worktree picker offers to move it back.
- **Detached commits.** Leaving a detached HEAD whose commits no branch holds asks first, and offers
  to create a branch for them.
- **Git LFS.** Switching branches, checking out a commit or creating a worktree downloads its LFS
  files first, with progress in the top bar, a Cancel button and the login prompt git-lfs may need.

### Terminals per worktree

Every worktree keeps its own terminals, so nothing gets lost when you hop between them. A terminal
can open in another project or any folder too. Drag a terminal's tab to reorder them, or use Move
Left / Move Right in its menu; the order is kept with the rest.

### Knowing when an agent needs you

- A terminal tab shows a **ring while Claude Code works** there, and a **dot once it finishes or
  asks for you**; the worktree's row in the picker gets the dot too. A notification or the Agents
  menu brings you to that terminal.
- Other agents get the dot when they ring the terminal bell or send a notification.
- Desktop notifications are off until you turn them on (Settings → Notifications, one switch per kind).
- The **Agents menu** in the top bar lists every agent in your terminals, the ones waiting for you
  first, and the Dock icon counts those. A badge on its icon pulses while an agent works and stays
  lit while one waits for you or finished out of sight.

### Resuming

Each worktree's terminals come back at launch with their splits, names and last 1,000 lines. Quit
with Claude Code, Codex, Gemini CLI or opencode still running, and the restored terminal has the
command that resumes the conversation typed at the prompt (Settings → Terminal can run it instead,
or leave it out). Quit asks first while a command runs or an agent is mid-turn. **Resume a
conversation** picks up any older Claude Code conversation in a worktree.

---

## Guided review

Your own agent CLI (Claude Code, Codex, opencode, pi, llm or a command of your own, already signed
in) reads a change first and writes a guide to it. Pick the agent, model, effort and language in
Settings → Guided Review.

- **What it covers.** A pull request (from its menu, whatever is checked out), a commit (Explain
  Commit in its right-click menu in History), or what isn't committed yet (the ✦ in the Changes
  summary or the palette, without touching the index). A pull request's review says when it was
  pushed to since.
- **The guide.** An overview, a diagram of the data and flow the change touches when there is one,
  then sections in reading order, each with its category (UI, API, security, tests…), its importance
  and a one-line check, beside its diffs. Filter them by category or importance, and mark sections
  and files reviewed as you go. Big changes work too: every changed file reaches the agent. While
  the agent runs, the guide shows the last lines it printed, for a CLI that prints its progress.
- **Find Risks.** Lists a change's concrete problems beside the guide, most severe first: wrong
  logic, edge cases, races, security and data loss. A risk that names a line shows on that line of
  the diff too.
- **Ask Agent.** Each section and the whole change start a new Claude Code session in the
  worktree's terminal with that part in its prompt box. Write your question and it's sent with it,
  or Skip to ask it there; a risk goes at once, asking the agent to look into it. The session opens
  in the worktree where the change is checked out, so the agent can fix the files; the button's
  menu pastes into the agent already running instead, or copies it. With another agent CLI picked,
  the part is copied for you to paste, and one that keeps sessions starts in a terminal. Notes from
  the agent sit on the lines they're about.

---

## Diffs and review

### Live diffs

- Diffs refresh the moment an agent saves, without losing your scroll position.
- Unified or split, with word-level highlights. Unchanged lines are folded away until you want the
  whole file (`⌥C`).
- A changed image shows side by side, as a swipe or as an onion skin, and both sides zoom and pan
  together.
- Diffs are computed in Rust and highlighted off the main thread, so big ones stay fast.

### Walking through changes

- `J` / `K` walk the changed files, `V` marks one viewed. When the agent touches it again, the mark
  clears so you know to look again.
- Changes lists files flat or as a folder tree, by name or newest first, which helps when an agent
  touches many at once.
- A folder stages or discards everything under it.

### Editing in the diff

- In an unstaged diff, edit the new side right there and save with `⌘S`. If the file changed on
  disk since, it asks before overwriting what the agent wrote.
- Stage, unstage or discard single lines, or stash just the lines you pick.

### Review notes

`C`, or a line's +, writes a note under the selected lines in Changes' and History's diffs. Notes
follow their lines through edits on disk, or say they're outdated, and the Changes panel lists them
with Copy as Prompt and Send to Terminal.

### Reviewing pull requests

Pull requests open in the same diff viewer. Read one commit by commit: pick a commit, or a run of
them, to see only what they changed. See why a check failed and copy it for the agent (GitHub).
`C` on selected lines writes a line comment.

---

## The terminal

A real terminal sits under the diff (`⌘J`), with tabs and splits. Run the tests, start the dev
server or talk to the agent without leaving the change you're reading.

### Splits and panes

- Each pane of a split has a numbered title bar with its name, or the title its program sets.
  `⇧⌘←` / `⇧⌘→` page through them, and `⌘R` names one.
- A split's tab shows a dot per pane in its agent's state, so an agent at work in another pane
  stays in sight; hover the dots for a count ("2 agents working, 1 waiting for you"), and past
  six panes they turn into a count per state. Zoomed to one pane (`⇧⌘↵`), the toolbar shows
  which one it is.
- In the terminal, `⌃⌘T` puts focus on the tab strip: `←` / `→` switch tabs, `⌥←` / `⌥→` move
  one, `⌘W` closes it, and `↵`, `Esc` or `⌃⌘T` again goes back to the pane.
- The terminal has its own font, size, line height, cursor and scrollback in Settings. `⌘=` / `⌘−`
  in the terminal, a pinch or Ctrl+scroll over it size its text.
- `⌘F` finds in the terminal and its scrollback, with match case, whole word and regex.

### Links

- `⌘`-click a path it prints to open the file at that line, or a folder to show it in the explorer.
  A path that a program like Claude Code wrapped over two rows is still one link. A file outside
  the repo (an agent's screenshot in `/tmp`, `~/notes.md`) opens read-only in a tab, an image as
  an image, and a folder outside it shows in the file manager. A bare name under a line that names
  its folder (`- p4.png` after `…/onb-c/ içinde:`) opens from that folder, and a path with spaces
  (a macOS screenshot, `~/Desktop/Ekran Resmi 2026-10-15 18.34.46.png`) is one link.
- A link to a page on this machine (`localhost:3000`) opens in a browser tab, any other in your browser.
- A commit's SHA opens it in a tab and selects it in History. `#123` or a link to one of the repo's
  pull requests or issues opens it in its own tab.
- `⌃⇧Space` puts a label on every one of those links on screen, in vim or Claude Code too: type it
  to open the link, or with Shift to copy it.

### Commands

In zsh and bash 4.4+ each command gets a mark, red when it failed; hover it to see how long it
took. fish 4 marks its own. `⌘↑` / `⌘↓` jump between them, and the last one's output copies from
the right-click menu. Your dotfiles stay as they are.

Turn on "A long command finishes" in Settings → Notifications and a command that runs past 10
seconds (or the time you pick) and ends while you're looking elsewhere puts a dot on its tab, with
a desktop notification naming the program and its subcommand (`cargo build`), never the rest of its
arguments, if you turned those on.

### Made for agents

- Paste a screenshot or drop files and Claude Code or Codex gets their paths.
- Shift+Enter reaches a program that asks for it as its own key, as Claude Code and Codex do.
- Claude Code tells the terminal its state as it changes, so its tab's ring and dot follow it the
  moment it starts, stops or asks.
- Switch the app between light and dark and a running Claude Code on its "auto" theme switches too.
- On a Mac a trackpad swipe scrolls Claude Code's fullscreen view row by row, as it does vim and htop.
- `⌘C` copies what you selected in Claude Code's fullscreen view, and a Japanese or Chinese input
  method's candidate window stays at Claude Code's input as it redraws.
- Pasting several lines into a program that would run each one as it arrives asks first.
- A program that crashed or was killed in full screen leaves no mess: the next prompt is back in
  the scrollback with the cursor shown, and moving the mouse doesn't type into it.

---

## The browser

Open a browser tab beside the code with `⇧⌘B` or View → Open Browser, and check the agent's work
where you read it. Browser tabs are macOS only for now: on Linux a tab says it can't open the page
there, and one opened from a link offers Open in your browser.

### Tabs

- The ports button beside the address lists every port your worktree's terminals serve.
- Each tab is a page of its own, with `⌘F` to find in it, `⌘=` / `⌘−` to zoom it, a switch to show
  it light or dark whatever the app is, and the Web Inspector on `⌥⌘I`.
- A page out of sight closes after a while to give its memory back, and loads again when you look,
  its picture standing in until it does. Settings → Browser sets how many stay open and for how
  long, and can reload a worktree's pages when its agent finishes.

### Device mode

`⇧⌘M` draws a phone or tablet around the page, notch and corners included, and gives the page that
device's width, pixel ratio and user agent, upright or turned. A phone or tablet also has touch: the
page finds touch events, `maxTouchPoints`, `(pointer: coarse)` and `(hover: none)` (in `matchMedia`
and in its own stylesheets, not another origin's), and your mouse reaches it as a finger, a click as
a tap. Responsive keeps the mouse, at a size an agent can set with `device`. It's still your Mac's
browser engine: `:hover` follows the real pointer, and `<meta name=viewport>` is ignored.

### Ask the agent about the page

- **Pick elements.** `⇧⌘C` picks an element (⇧-click to gather a few, then click the last). Ask what
  you want, and a new agent session starts in your worktree's terminal with the page's address,
  each element's selector, React components, styles and a picture of it, and your question, as Ask
  Agent does with a review.
- **Send errors.** The console counts the page's errors and warnings, failed requests too. Send
  Errors to Agent asks about them the same way, and a question about an element can take the page's
  errors along.

### Agents drive the browser

Off until you turn it on in Settings → Browser, on macOS. Then the agents in a terminal pane get
tabs of their own, opened in the background without taking yours. `gitviber browser help` lists
every command.

**Out of your way**

- **Never yours.** An agent's clicks, keys, hover, drag and scroll reach the page as real input,
  without moving your pointer or reaching your keyboard. A page's Copy button writes to the tab's
  own clipboard, not yours: a guard against ordinary pages (see Privacy), not a wall.
- **Your tab stays yours.** Click or type in an agent's tab and it's yours: commands that would act
  on it stop and say so, and those that only read it still run, until you press Give back to agent
  or run `gitviber browser tab release`. `tab list` shows each tab's owner.
- **Several tabs per agent.** `tab new <name> [url]` opens another, and `--tab <name>` sends a
  command to one, so agents sharing a pane never switch tabs under each other. A terminal pane may
  open 12.
- **Open a page for you.** When you ask an agent to open a page so you can try it, it runs `tab new
  <url> --show`: the page opens as your active tab, in a private session apart from your logins
  (sign in there if you need to), and is yours from the start; only Give back to agent hands it
  over. It is yours alone: no agent command reaches it, reading included, but `tab list`; its
  downloads are refused and the popups it opens are yours too. Three at a time a pane, three
  seconds apart. It works with Let agents use the browser off, and nothing else does then.
- **Hide their tabs.** Settings → Browser → Show agents' tabs, off, leaves agent tabs out of the
  tab strip. They keep running and agents' commands still work on them; a tab you took stays in
  the strip, and turning it on brings the rest back.
- **Muted for you.** Settings → Browser → Mute agents' tabs (on by default): agent tabs make no
  sound for you; the page still plays and `audio: playing` (in `tab list` and `url`) shows it.
  Tabs you open or take over are never muted.

**Keeps working, stays light**

- **While GitViber is hidden or minimized,** an agent's tabs keep drawing. Settings → Browser →
  Agent tabs drawing at once (4 to 24, 12 by default) sets how many; past it the one used longest
  ago sleeps.
- **A tab nobody touched slows down** after ten minutes (`--idle` sets it), and one unused for an
  hour closes, unless it's on show. When the Mac runs low on memory, sleeping tabs give theirs back
  at once.
- **No second browser.** The tabs are the system's WebKit, so there is nothing extra to launch.

**Logins**

- Each tab has its own cookies and storage, so two tabs can be two accounts of the app you're
  testing. `tab new --shared` uses your logins instead.
- `session save` keeps a tab's login (cookies, localStorage and sessionStorage) for the repository
  and its worktrees, and `tab new --session` or `session load` puts it into another tab or another
  agent's. `--indexeddb` adds the page's IndexedDB, for apps that keep their login there; a database
  that holds a Blob or is too big is named and left out. Saved sessions are readable by you alone.

**Acting on a page**

- **Like a person.** Clicks, typing and selects wait for their element to show, be enabled and hold
  still, and name what covers it when something does. `fill`, `select`, `check` and `uncheck` read
  the value back, and each action ends with a line of what changed: elements added or removed, a
  dialog, the address, console errors and requests.
- **Reading.** The page as a tree whose refs stay across snapshots, `find`, `get` for one element's
  text, HTML, value, attribute, styles or box, and frames of the page's origin (a CSS selector
  reaches into one as `frame=<ref|css> >> <css>`, and `wait --frame` looks there).
- **Screenshots** at the page's own size, of the viewport, one element, a clip or (`--full`) the
  whole page in up to five pictures of 2000 px, with `--refs` to draw each control's ref on the
  picture.
- **Popups and downloads.** `window.open` and `target=_blank` open as tabs of the same pane, keeping
  `window.opener`, so sign-in and payment windows that talk back work. A download lands in a folder
  of the tab's own, its path in the agent's reply. `upload` answers a file chooser.
- **DevTools for the agent.** `console` shows the page's lines, and `network` lists fetches, XHRs
  and WebSockets, each in full with headers, bodies and WebSocket frames. The log stays in memory.
- **Safe by default.** Page text comes back between marks naming its origin so an agent can tell it
  from your instructions, and tokens, keys and passwords in addresses, headers and bodies are
  hidden unless `--reveal`.

### Teaching agents the command

So agents know the command is there, and when to use it instead of your own browser, GitViber
writes a skill for them while agent control is on:

- `~/.claude/skills/gitviber-browser/SKILL.md` for Claude Code, and
  `~/.agents/skills/gitviber-browser/SKILL.md` for Codex, each only if that agent is installed
  (`~/.claude` or `~/.codex` is there).
- It holds `gitviber browser help` as this version prints it, is rewritten by each new version
  (never by an older one), and is deleted when you turn agent control or Settings → Browser →
  Teach Claude Code and Codex off, or uninstall that agent.
- A `gitviber-browser` skill of your own there is never touched.
- For other agents, Settings has the same text to paste into your `CLAUDE.md` or `AGENTS.md`.

---

## Code and notes

### The file explorer

- Change bars in the gutter, quick open (`⌘P`), and rename, create and delete from the right-click menu.
  Changing only a name's letter case reaches git too, as a rename to commit.
- Pick two files with `⌘`-click and Compare Selected from the right-click menu to see them as a diff.
- Fix a line right in the file view and save it with `⌘S`. Go to Definition, references and `⌃G`
  for a line work there too.
- Images fit the view: a pinch, `⌘`-scroll or a double-click zooms around the pointer, a drag pans,
  and `=` `−` `0` zoom in, out and back to fit. On macOS PDFs, audio and video play in the tab; on
  Linux they open in their default app.

### Folders without git

A folder that isn't a git repository opens as a project of its own (`gitviber .`, File → Open
Repository…) and stays in the projects list. The terminal, the explorer and file editing, the
Obsidian vault and agents work there as usual; quick open and search follow its `.gitignore`. The git
panel and the top bar offer Initialize Repository instead, and once `git init` has run (there, or in
a terminal) the project turns into a repository in place.

### Markdown

Markdown renders the way GitHub shows it, in files, pull requests and issues alike: Mermaid
diagrams and math, drawn by the app itself, `> [!NOTE]` alerts, and a file's frontmatter as a table.

### Obsidian vault

If you use Obsidian, your vault shows in a panel of its own beside the explorer, found on its own.
Settings → Obsidian picks which vaults show.

- Notes open in a tab the way Obsidian's reading view shows them: wikilinks that open the note they
  name, embedded notes and images, callouts, highlights, tags and properties. An embedded PDF,
  audio or video file plays in the note on macOS, and on Linux is a card that opens it in its app.
- Canvases open as their board, to pan and zoom. Images open in the media viewer, and audio, video
  and PDFs in their default app.
- Switch a note to Code to edit it and save it with `⌘S`.

---

## Commit messages from your agent

Hit the sparkle next to the commit box and your own `claude -p` or `codex exec` writes the message
from the diff; Settings → Commit Messages picks another agent CLI, or turns it off. New pull
request has the same button for the title and description. No API key, no extra account: it uses
the CLI you're already signed in to.

---

## Git

### History

- Branches and merges as a colored lane graph. Undo, revert, reset, check out or tag from the
  right-click menu.
- **Search** by message, or narrow it with `author:`, `path:` and `code:` (text a commit added or
  removed).
- **Rewrite.** Reword, squash, drop or reorder commits: pick several with ⌘- or ⇧-click, drag them
  between commits to move them or onto one to squash them. Split a commit into pieces to commit with
  line staging, or fix staged changes up into an older one. Cherry-pick several commits at once.
- Branches that point into rewritten commits can move along. Anything that rewrites pushed commits
  asks first.
- **Compare** any two commits, or one with the working tree. Open All shows every file of a commit as
  one scroll of diffs.
- **Take back.** Put a file back as a commit had it, or take back just that commit's change to it,
  or a few of its lines. `⌘Z` undoes it.
- **Find the commit that broke it.** Bisect from a commit you know was good: mark each one it
  checks out good, bad or skip until git names the first bad one.
- **Reflog.** Find a commit you lost, then look at it, branch from it or reset to it.

### Branches

- Merge (fast-forward, no fast-forward or squash), rebase, and pull with fast-forward, merge or
  rebase.
- Pin branches to the top of the picker, with the ones you checked out last under them.
- Clean up the merged ones in one go, squash- and rebase-merged included.
- **Compare** any two branches, remote branches, tags or commits on a screen of their own: the
  commits each has that the other lacks, the files changed since they parted (as a pull request shows
  them) or every difference as they stand, all in one scroll with Open All. Its merge section says
  what would conflict before you merge.

### Stashes

Apply, pop, drop or rename a stash, or take one file from it.

### Patches

Copy a commit, one of its files or a stash as a patch. Apply one from the clipboard: it shows what
it changes first, merges with your edits where it must, and `⌘Z` takes it back.

### Conflicts

Resolve block by block (current, incoming or both), or take a whole side or mark a file resolved
from the Changes list, then continue, skip or abort. A Git LFS file is never merged line by line:
you keep one side whole, and the real file comes back.

### Big repositories

Your `status.showUntrackedFiles` holds in the Changes list: `normal` shows a new folder as one row
that stages whole, `no` leaves untracked files out and says so. A big history gets a commit-graph
in the background when it has none, so its pages come up at once.

### Pull requests (GitHub)

List them, narrow it to the ones you created, are assigned, are asked to review or are mentioned in,
or to drafts and labels (the scope and drafts are remembered per repository). Read, review in the
same diff viewer, create, merge and check out. A merge or a review goes to the head you were
shown, so commits pushed since aren't merged or approved unseen. See
[Reviewing pull requests](#reviewing-pull-requests).

### Issues (GitHub)

List them, narrow it to the ones you created, are assigned or are mentioned in, or by label. Read,
open, edit, comment, close (as completed or not planned) and reopen, or start one in its own
worktree.

---

## Looks and settings

- **Themes.** Eleven color themes for the whole app, from Nord and Catppuccin to Solarized, a dark
  and a light one to follow the system, and 24 syntax themes.
- **Type and scale.** Your own interface font, code font, terminal font and interface scale.
- **Transparency.** On macOS a see-through window, with its opacity yours to set and the desktop
  blurred behind it if you like; Terminal too lets it show through the terminal.
- **Projects.** The top bar's project list has a filter, pins, folders you name, and last opened or
  your own drag order on top of them. Each project wears a color of its own on its tile, picked
  for it unless you choose one; drag a row onto a folder or header to move it, or right-click it.
- **Settings** (`⌘,`) has one search over every setting.
- **Shortcuts.** Most actions have one, every one can be rebound in Settings (`⌘,`), and holding `⌘`
  shows the ones that work where you are. A shortcut looks the same wherever it shows: keycaps in
  menus, tooltips, the palette and pickers. See [Shortcuts](SHORTCUTS.md).
- **Command palette** (`⇧⌘P`). Each command has an icon of what it does, recently used ones are
  listed under their own heading, and the foot says the keys it takes.
- **Keyboard all the way.** A focus shortcut pressed again gives focus back where it was, and every
  picker opened by a key or the palette takes the keys: arrows, Home/End, `↵` to pick, `Esc` to close.
- **Updates.** GitViber checks at launch, every few hours and when Settings → Updates opens. A newer
  version shows as a badge in the top bar: click it for the release notes and the download, then
  again to restart into it. Nothing downloads until you choose to.

---

## Size, memory and speed

Measured on an M2 Pro. Memory is the footprint Activity Monitor shows, summed over GitViber and
every process it starts, with a repository open in a window of about 1480×920 points. 0.1.17, the
last release built on a web view, was measured the same way on the same Mac.

| | 0.1.18 | 0.1.17 |
| --- | --- | --- |
| Download, Apple Silicon / Intel | 20.6 MB / 23.7 MB | 29 MB, one for both |
| Memory at launch, the repository open | 95–100 MB | 317 MB |
| With History and a file open | 104 MB | |
| Minimized or covered by another window | 52–62 MB | |
| Peak while paging through a 100,000-line file | 186 MB | 2.6 GB |
| Cold start, until the repository shows | 0.76 s | 5.4 s |
| `seq 1 2000000` in the terminal | 2.1 s | 9.9 s |
| Command palette or file finder, until it shows | about 45 ms | |
| One git call | 4 ms | 13 ms |
| History reading its log | 25 ms | 118 ms |

- **Closed is closed.** A tab, a terminal or a document gives its memory back when it closes.
  0.1.17 went from 278 MB idle to 502 MB after a session with everything closed again.
- **Hidden is light.** Minimized or covered, the window lets go of its GPU memory and takes it back
  before the next frame you see.
