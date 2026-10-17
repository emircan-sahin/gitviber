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

---

## Worktrees and agents

Give every agent its own worktree and switch between them from the top bar.

### Starting and managing worktrees

- **Switcher.** Each worktree shows its change count, its pull request and how its checks are doing.
  The top bar names a worktree by its branch and its folder, so a folder that no longer says what it
  holds shows.
- **Start from anywhere.** From any branch, commit or issue, in the folder you choose. Or check a
  pull request out into its own worktree without touching yours.
- **Setup.** A new worktree can run a command such as `claude` in its terminal right away, and gets
  copies of the ignored files `.worktreeinclude` lists, like your `.env`.
- **Organize.** Rename one along with its folder, give it a color its chip and terminal tabs wear,
  lock it, and remove or prune it when the work is merged.
- **Clean up.** One action removes every merged worktree with nothing uncommitted, branches too,
  after showing the ignored files that go with them, like `node_modules`. Merging a pull request in
  the app offers to remove its worktree.
- **One branch, one worktree.** Git keeps a branch in one worktree, so picking one that another holds
  says where it is: Enter opens that worktree (with a way back), and the row's menu checks the commit
  out here detached, or starts a new branch from it. When a linked worktree holds the default branch
  and the main folder is on another, the worktree picker offers to move it back.

### Terminals per worktree

Every worktree keeps its own terminals, so nothing gets lost when you hop between them. A terminal
can open in another project or any folder too.

### Knowing when an agent needs you

- A tab and its worktree show a **ring while Claude Code works** there, and a **dot once it finishes
  or asks for you**. Clicking it brings you to that terminal.
- Other agents get the dot when they ring the terminal bell or send a notification.
- Desktop notifications are off until you turn them on (Settings → Notifications, one switch per kind).
- The **Agents menu** in the top bar lists every agent in your terminals, the ones waiting for you
  first, and the Dock icon counts those.

### Resuming

Quit with Claude Code, Gemini CLI or opencode still running, and the restored terminal has the
command that resumes the conversation typed at the prompt. **Resume a conversation** picks up any
older Claude Code conversation in a worktree.

---

## Guided review

Your own agent CLI (`claude` or `codex`, already signed in) reads a change first and writes a guide
to it. Pick the agent, model, effort and language in Settings → Guided Review.

- **What it covers.** A pull request (from its menu, whatever is checked out), a commit (right-click
  it in History, or Explain Commit), a branch, or what isn't committed yet (the ✦ in the Changes
  summary or the palette, without touching the index). A push to a pull request marks its review outdated.
- **The guide.** An overview, a diagram of the data and flow the change touches when there is one,
  then sections in reading order, each with its category (UI, API, security, tests…), its importance
  and a one-line check, beside its diffs. Mark sections and files reviewed as you go. Big changes
  work too: every changed file reaches the agent.
- **Find Risks.** Lists a change's concrete problems beside the guide, most severe first: wrong
  logic, edge cases, races, security and data loss.
- **Ask Agent.** Each section, each risk and the whole change start a new Claude Code session in the
  worktree's terminal with that part in its prompt box. Write your question and it's sent with it, or
  Skip to ask it there. The session opens in the worktree where the change is checked out, so the
  agent can fix the files; the button's menu pastes into the agent already running instead, or
  copies it. Notes from the agent sit on the files and lines they're about.

---

## Diffs and review

### Live diffs

- Diffs refresh the moment an agent saves, without losing your scroll position.
- Unified or split, with word-level highlights. Unchanged lines are folded away until you want the
  whole file (`⌥C`).
- A changed image shows side by side, as a swipe or as an onion skin.
- Diffs are computed in Rust and highlighted off the main thread, so big ones stay fast.

### Walking through changes

- `J` / `K` walk the changed files, `V` marks one viewed. When the agent touches it again, the mark
  clears so you know to look again.
- Changes lists files flat or as a folder tree, by name or newest first, which helps when an agent
  touches many at once.
- A folder stages, discards or stashes everything under it.

### Editing in the diff

- In an unstaged diff, edit the new side right there and save with `⌘S`.
- Stage, unstage or discard single lines, or stash just the lines you pick.

### Reviewing pull requests

Pull requests open in the same diff viewer. Read one commit by commit: pick a commit, or a run of
them, to see only what they changed. See why a check failed and copy it for the agent (GitHub). Add
review notes on selected lines with `C`.

---

## The terminal

A real terminal sits under the diff (`⌘J`), with tabs and splits. Run the tests, start the dev
server or talk to the agent without leaving the change you're reading.

### Splits and panes

- Each pane of a split has a numbered title bar with what runs there. `⇧⌘←` / `⇧⌘→` page through
  them, and `⌘R` names one.
- Zoomed to one pane (`⇧⌘↵`), the toolbar shows which one it is and a dot per pane, so an agent at
  work in another stays in sight.
- The terminal has its own font, size, cursor and scrollback in Settings. `⌘=` / `⌘−` in the
  terminal or a pinch over it size its text.

### Links

- `⌘`-click a path it prints to open the file at that line, or a folder to show it in the explorer.
  A path that a program like Claude Code wrapped over two rows is still one link. A file outside
  the repo (an agent's screenshot in `/tmp`, `~/notes.md`) opens read-only in a tab, an image as
  an image, and a folder outside it shows in the file manager. A bare name under a line that names
  its folder (`- p4.png` after `…/onb-c/ içinde:`) opens from that folder, and a path with spaces
  (a macOS screenshot, `~/Desktop/Ekran Resmi 2026-10-15 18.34.46.png`) is one link.
- A link to a page on this machine (`localhost:3000`) opens in a browser tab, any other in your browser.
- A commit's SHA opens it in History. `#123` or a link to one of the repo's pull requests or issues
  opens it in its own tab.
- `⌃⇧Space` puts a letter on every one of those links on screen, in vim or Claude Code too: type it
  to open the link, or with Shift to copy it.

### Commands

In zsh and bash 4.4+ each command gets a mark, red when it failed, that tells how long it took.
`⌘↑` / `⌘↓` jump between them, and the last one's output copies from the right-click menu. Your
dotfiles stay as they are.

Turn on "A long command finishes" in Settings → Notifications and a command that runs past 10
seconds and ends while you're looking elsewhere puts a dot on its tab, with a desktop notification
naming the program (never its arguments) if you turned those on.

### Made for agents

- Paste a screenshot or drop files and Claude Code or Codex gets their paths.
- Shift+Enter reaches them as its own key.
- On a Mac a trackpad swipe scrolls Claude Code's fullscreen view row by row, as it does vim and htop.

---

## The browser

Open a browser tab beside the code with `⇧⌘B` or the globe in the top bar, and check the agent's
work where you read it.

### Tabs

- A terminal pane's right-click menu opens the ports its dev server listens on, and the address bar
  lists every port your worktree's terminals serve.
- Each tab is a page of its own, with `⌘F` to find in it, `⌘=` / `⌘−` to zoom it, a switch to show
  it light or dark whatever the app is, and the Web Inspector on `⌥⌘I`.
- On Linux a tab shows the pages on your own machine (localhost) only, without these tools.
- A page out of sight closes after a while to give its memory back, and comes back where it was
  when you look again. Settings → Browser sets how many stay open and for how long, and can reload a
  worktree's pages when its agent finishes.

### Device mode

`⇧⌘M` draws a phone or tablet around the page, notch and corners included, and gives the page that
device's width, pixel ratio and user agent, upright or turned. Responsive takes any size you drag it
to. It's still your Mac's browser engine with a mouse: no touch events, hover still works, and
`<meta name=viewport>` is ignored.

### Ask the agent about the page

- **Pick elements.** `⇧⌘C` picks an element (⇧-click to gather a few, then click the last). Ask what
  you want, pick the model, and a new agent session starts in your worktree's terminal with the
  page's address, each element's selector, React components, styles and a picture of it, and your
  question, as Ask Agent does with a review.
- **Send errors.** The console counts the page's errors and warnings, failed requests too. Send
  Errors to Agent asks about them the same way, and a question about an element can take the page's
  errors along.

### Agents drive the browser

Off until you turn it on in Settings → Browser. Then the agents in a terminal pane get tabs of their
own, opened in the background without taking yours. `gitviber browser help` lists every command.

**Out of your way**

- **Never yours.** An agent's clicks, keys, hover, drag and scroll reach the page as real input,
  without moving your pointer or reaching your keyboard or clipboard.
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
- **No second browser.** The tabs are the system webview the app already runs in, so there is
  nothing extra to launch.

**Logins**

- Each tab has its own cookies and storage, so two tabs can be two accounts of the app you're
  testing. `tab new --shared` uses your logins instead.
- `session save` keeps a tab's login (cookies, localStorage and sessionStorage) for the repository
  and its worktrees, and `tab new --session` or `session load` puts it into another tab or another
  agent's. Saved sessions are readable by you alone.

**Acting on a page**

- **Like a person.** Clicks, typing and selects wait for their element to show, be enabled and hold
  still, and name what covers it when something does. `fill`, `select`, `check` and `uncheck` read
  the value back, and each action ends with a line of what changed: elements added or removed, a
  dialog, the address, console errors and requests.
- **Reading.** The page as a tree whose refs stay across snapshots, `find`, `get` for one element's
  text, HTML, value, attribute, styles or box, and frames of the page's origin.
- **Screenshots** at the page's own size, of the viewport, one element, a clip or (`--full`) the
  whole page in pictures of 2000 px, with `--refs` to draw each control's ref on the picture.
- **Popups and downloads.** `window.open` and `target=_blank` open as tabs of the same pane, keeping
  `window.opener`, so sign-in and payment windows that talk back work. A download lands in a folder
  of the tab's own, its path in the agent's reply. `upload` answers a file chooser.
- **DevTools for the agent.** `console` shows the page's lines, and `network` lists fetches, XHRs
  and WebSockets, each in full with headers, bodies and WebSocket frames. The log stays in memory.
- **Safe by default.** Page text comes back between marks naming its origin so an agent can tell it
  from your instructions, and tokens, keys and passwords in addresses and headers are hidden unless
  `--reveal`.

### Teaching agents the command

So agents know the command is there, and when to use it instead of your own browser, GitViber
writes a skill for them while agent control is on:

- `~/.claude/skills/gitviber-browser/SKILL.md` for Claude Code, and
  `~/.agents/skills/gitviber-browser/SKILL.md` for Codex, each only if that agent is installed
  (`~/.claude` or `~/.codex` is there).
- It holds `gitviber browser help` as this version prints it, is rewritten when an update changes
  that (never by an older version), and is deleted when you turn agent control or Settings →
  Browser → Teach Claude Code and Codex off, or uninstall that agent.
- A `gitviber-browser` skill of your own there is never touched.
- For other agents, Settings has the same text to paste into your `CLAUDE.md` or `AGENTS.md`.

---

## Code and notes

### The file explorer

- Change bars in the gutter, quick open (`⌘P`), and rename, create and delete from the right-click menu.
- Pick two files with `⌘`-click to compare them side by side.
- Fix a line right in the file view and save it with `⌘S`.

### Markdown

Markdown renders the way GitHub shows it, in files, pull requests and issues alike: Mermaid
diagrams, math, `> [!NOTE]` alerts, and a file's frontmatter as a table.

### Obsidian vault

If you use Obsidian, your vault shows in a section under the explorer's files, found on its own.
Settings → Obsidian picks which vaults show.

- Notes open in a tab the way Obsidian's reading view shows them: wikilinks that open the note they
  name, embedded notes, images and PDFs, callouts, highlights, tags and properties.
- Canvases open as their board, to pan and zoom. Images, audio, video and PDFs open in the media viewer.
- Switch a note to Code to edit it and save it with `⌘S`.

---

## Commit messages from your agent

Turn it on in Settings, hit the sparkle next to the commit box, and your own `claude -p` or
`codex exec` writes the message from the diff. New pull request has the same button for the title
and description. No API key, no extra account: it uses the CLI you're already signed in to.

---

## Git

### History

- Branches and merges as a colored lane graph. Undo, revert, reset, check out or tag from the
  right-click menu.
- **Rewrite.** Reword, squash, drop or reorder commits: pick several with ⌘- or ⇧-click, drag them
  between commits to move them or onto one to squash them. Split a commit into pieces to commit with
  line staging, or fix staged changes up into an older one. Cherry-pick several commits at once.
- Branches that point into rewritten commits can move along. Anything that rewrites pushed commits
  asks first.
- **Compare** any two commits, or one with the working tree. Open All shows every file of a commit as
  one scroll of diffs.
- **Take back.** Put a file back as a commit had it, or take back just that commit's change to it,
  or a few of its lines. `⌘Z` undoes it.

### Branches

- Merge, rebase, and pull with fast-forward, merge or rebase.
- Pin branches to the top of the picker, with the ones you checked out last under them.
- Clean up the merged ones in one go, squash- and rebase-merged included.
- **Compare** any two branches, remote branches, tags or commits on a screen of their own: the
  commits each has that the other lacks, the files changed since they parted (as a pull request shows
  them) or every difference as they stand, all in one scroll with Open All.
- A merge into the current branch says what would conflict before you make it.

### Patches

Copy a file, a few lines, a commit or a stash as a patch. Apply one from the clipboard: it shows what
it changes first, merges with your edits where it must, and `⌘Z` takes it back.

### Conflicts

Resolve block by block (current, incoming, both, or by hand) with each block's merge base a click
away, then continue, skip or abort. Or hand them to the agent in your terminal, or open them in the
merge tool your git config names.

### Pull requests (GitHub)

List them, narrow it to the ones you created, are assigned, are asked to review or are mentioned in,
or to drafts and labels (it remembers per repository). Read, review in the same diff viewer, create,
merge and check out. See [Reviewing pull requests](#reviewing-pull-requests).

### Issues (GitHub)

List them, narrow it to the ones you created, are assigned or are mentioned in, or by label. Read,
open, edit, comment, close and reopen, or start one in its own worktree.

---

## Looks and settings

- **Themes.** Eleven color themes for the whole app, from Nord and Catppuccin to Solarized, and 24
  syntax themes.
- **Type and scale.** Your own code font, terminal font and interface scale.
- **Transparency.** On macOS a see-through window, with its opacity and the desktop's blur behind it
  yours to set.
- **Shortcuts.** Every action has one, every one can be rebound in Settings (`⌘,`), and holding `⌘`
  shows them all. See [Shortcuts](SHORTCUTS.md).
