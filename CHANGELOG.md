# Changelog

Notable changes to GitViber. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). A release's notes
on GitHub are its section below.

## [Unreleased]

## [0.1.16] - 2026-10-17

### Added

- **Projects in the sidebar.** Settings → Appearance → Projects in the sidebar lists your projects at the top of the left panel, so a project is one click away instead of two. It is off until you turn it on. In #120.
- **Files under each guided review section.** In a wide guided review, every section lists its files as a tree in the navigator; a click jumps to that file's diff. In #119.
- **Downloads in your own browser tabs.** A file a tab can't show, an attachment or a `download` link is saved in your Downloads folder under a name that never replaces a file, marked as from the web, with progress, Cancel and Show in Finder under the address bar. A page may start three downloads per ten seconds and four run at once. In #127.
- **Popups open as tabs.** `window.open` and `target=_blank` in your own tab open next to it with the opener kept, so sign-in and payment windows can report back and close themselves. In #127.
- **Full-page screenshots for agents.** `gitviber browser screenshot --full` takes the whole page in pictures of at most 2000 px; fixed headers and footers show once. In #125.
- **Touch for phone and tablet sizes.** A phone or tablet device gives the page touch events, a coarse pointer and taps, for an agent's clicks and your own mouse. In #126.
- **CSS and `wait` inside frames.** `frame=<ref|css> >> <css>` reaches an element in a same-origin frame from any command, and `wait --frame` waits in one. In #122.
- **One clipboard per agent tab.** Frames of an agent tab share one copy kept apart per origin, and the page's own Copy button works once you take the tab; your real clipboard is never read or written by an agent's page. In #124.
- **IndexedDB in saved sessions.** `session save --indexeddb` keeps a page's IndexedDB with the login, up to a size cap, naming what it leaves out. Without the flag it is never read, since a page that keeps an encryption key there could make macOS ask for the keychain password. In #129.

### Changed

- **The change line says what it saw.** After an action the agent reads `→ no DOM change` instead of "no visible change", `→ canvas repainted` when a canvas drew something because of the action, and `→ canvas animating` when it was moving on its own. `wait --text` is documented to match the rendered text. In #131.
- **A tab you take lets go of the keys its agent held,** and a tab holding a key is no longer put to sleep under it.

### Fixed

- **The mouse cursor changes over a browser tab.** The page under the tab no longer sets its own arrow over the link's hand or the text's I-beam.
- **Browser tabs render at the display's refresh rate** instead of WebKit's 60 fps cap, which held games to 60 on a 120 Hz screen.
- **Reload works on a page that never loaded,** such as a dev server that wasn't up yet; hard reload bypasses the cache there too.
- **A tab handed back to you drops the network log.** The script that records every request stops and its log is deleted when an agent's pane closes. In #130.

## [0.1.15] - 2026-10-15

### Added

- **Open a page for yourself from an agent.** `gitviber browser tab new <url> --show` opens the page as a tab of yours: in the strip, active, in a private session apart from your logins. It works even with agent control off, and the agent can do nothing to such a tab but list it until you press Give back to agent. A pane may show three at a time, three seconds apart.
- **Hide agents' tabs.** Settings → Browser → Agents → Show agents' tabs takes them out of the tab strip; they keep running and answering their agents, and a tab you took stays where it is.
- **Mute agents' tabs.** Settings → Browser → Agents → Mute agents' tabs keeps their pages quiet while they keep playing, so an agent's page that needs sound still works; `tab list` and `url` say `audio: playing` while a page makes sound. Tabs you open or take are never muted.
- **⌘-click opens more of what a terminal prints.** A file outside the repo (an absolute or `~` path) opens read-only in a tab and a folder outside it in the file manager, a bare name under a line naming its folder opens from there, a localhost link opens in a browser tab, and a folder in the repo opens in the explorer.

### Fixed

- **A path a program wrapped itself is one link.** Claude Code cuts a long path at the last column and goes on a row down; such rows are read as one line, and an absolute path that stops at a space in a folder or file name (a macOS screenshot) is offered in its longer readings.

## [0.1.14] - 2026-10-15

### Added

- **Several browser tabs per agent.** `gitviber browser tab new <name> [url]` opens another tab for a terminal pane's agents, `tab list` shows each one with its state and page, and `tab close` closes it; `--tab <name>` sends a command to one, so agents sharing a pane never switch tabs under each other. A tab an agent uses keeps drawing at full speed out of sight; one left alone for ten minutes (or its `--idle` minutes) sleeps, and one unused for an hour closes, unless it's on show.
- **Agent tabs keep drawing while GitViber is hidden or minimized.** Settings → Browser → Agent tabs drawing at once (4 to 24, 12 unless you choose) sets how many; past it the one used longest ago sleeps, and when the Mac runs low on memory, agents' sleeping tabs give theirs back at once.
- **Each agent tab signs in on its own.** Every tab has its own cookies and storage, so two tabs can be two accounts of the app you're testing; `tab new --shared` uses your logins instead. `session save` keeps a tab's login (cookies, localStorage and sessionStorage) for the repository and its worktrees, and `tab new --session` or `session load` puts it into another tab. Saved sessions are readable by you alone, never printed, and Clear Browsing Data deletes them.
- **The page's requests, for the agent.** `gitviber browser network` lists an agent tab's fetches, XHRs, WebSockets and other loads, filtered by failure, type, text, method or status, and `network <id>` shows one in full: headers, bodies and WebSocket frames. The log stays in memory, and only agents' tabs keep one.
- **Agents act on a page as a person could.** Clicks, typing and selects wait for their element to show, be enabled and hold still, and name what covers it when something does; `fill`, `select` and the new `check` / `uncheck` read the value back. Each action ends with one line of what changed: elements added or removed, a dialog, the address, console errors and requests. `wait` takes `--url` and `--fn`, and says what still wasn't so when it times out.
- **Agents use real mouse and keyboard input.** Clicks, hovers, drags, scrolls and keys reach the page as a person's would, trusted and with CSS `:hover`, without moving your pointer or touching your keyboard: `dblclick`, `rightclick`, `drag`, `key down` / `key up`, `press --hold`, and `type` that your keyboard layout doesn't change. An agent's keys and clipboard never reach your terminal or your clipboard.
- **Take a tab from the agent.** Press or type in an agent's tab and it's yours: commands that would act on it stop and say so, those that only read it still run, and Give back to agent in its address bar, or `gitviber browser tab release`, hands it back. `tab list` shows each tab's owner.
- **Agents upload files.** `upload` clicks the page's file input or its own upload button and answers the chooser that opens with the files named, which must be there.
- **Popups open as tabs of their own.** `window.open` and `target=_blank` in an agent's tab open a tab of the same pane, named for its opener (`t1-popup1`), in the opener's cookies and keeping `window.opener`, so sign-in and payment windows that talk back to the page work. Only a popup closes itself with `window.close()`; one past the 12 tabs a pane may open is blocked, its opener staying where it was, and the agent is told why. Each reply ends with what happened to the pane's tabs meanwhile, one `→` line each.
- **Agents get the files a page downloads.** An agent's tab saves its downloads in a folder of its own, two of one name keeping both: `wait --download` waits for one and prints its path, size and type, `downloads` lists the tab's files, and each download is told once, to that wait or in the pane's next reply.
- **Agents read a page in parts.** An element keeps its ref across snapshots, and a ref whose element left fails as stale instead of reaching another one. `find` searches the page for a text or a pattern, `get` reads one element's text, HTML, value, attribute, styles or box, and `snapshot` takes one element's part, each ref's box, or writes it all to a file.
- **Agents work inside a page's frames.** `snapshot`, `find`, `get` and every action go on into a frame of the page's origin, whose elements take refs as any others; a frame of another origin is named with its origin, as one the agent can't read. `console` and `network` include the frames' lines and requests, each naming the frame it came from.
- **Screenshots at the page's own size.** One pixel per CSS pixel unless `--scale` says otherwise, 2000 px on the longest side at most, of the viewport, one element or a `--clip` of it, as PNG or JPEG; `--refs` draws each control's ref on the picture.
- **Page text is marked as the page's.** What a page wrote comes back between marks naming its origin, so an agent can tell it from your instructions. Tokens, keys and passwords in addresses and headers are hidden unless `--reveal`, and `fill --secret NAME` types a value from the environment that no later reply shows.
- **GitViber teaches Claude Code and Codex to use its browser.** With agent control on, Settings → Browser → Teach Claude Code and Codex writes a `gitviber-browser` skill for each agent that's installed: when to use GitViber's browser and when your own, how to work with it, and every command. It follows each update, goes when you turn it off, and a skill of your own by that name is never touched. Other agents get the same text to paste into `CLAUDE.md` or `AGENTS.md`.

### Changed

- **A dialog left open on an agent's page is named on every reply** until it's answered, and commands that need the page's scripts fail at once instead of waiting.
- **`console` shows the current page's lines,** with `--all` for every page it kept and `--level` for errors or warnings; `eval` gives `undefined`, `NaN`, `Infinity` and big integers as themselves.
- **`gitviber browser help` groups its commands,** and a terminal pane opens 12 browser tabs at most.

### Fixed

- **An agent reaches only its own pane's browser tabs.** Each terminal pane gets a key of its own, so an agent that claims another pane's number can't drive its tabs or sessions.
- **A browser tab its agent closed doesn't come back with `⇧⌘T`,** and one you reopen is yours, not the agent's.
- **A command on a tab that closed to save memory waits for its page to load again,** instead of finding it empty.
- **Agents' clicks and typing work on more pages:** fields with a maximum length or a date format of their own, web components' fields, SVG, links inside headings and elements drawn with `display: contents`.
- **A page that keeps crashing stops,** on its third crash in five minutes, with why, instead of reloading again and again; an agent's commands there say so, and `reload` or `open` tries again.

## [0.1.13] - 2026-10-09

### Added

- **Ask Agent from a guided review.** Each section, each risk and the whole change start a new Claude Code session in the worktree's terminal with that part of the review in its prompt box: write your question and it's sent with it, or Skip to ask it there. The session opens in the worktree where the change is checked out, so the agent can fix the files; the button's menu pastes into the agent already running instead, or copies it.
- **Find Risks** lists a change's concrete problems beside its guided review, most severe first: wrong logic, edge cases, races, security and data loss.
- **A guided review of what isn't committed yet,** from the ✦ in the Changes summary or the palette: staged, unstaged and new files, without touching the index.
- **Guided Review from a pull request's menu,** whatever is checked out; a push to the pull request marks the review outdated.
- **A browser in a tab.** `⇧⌘B` or the globe in the top bar opens a web page beside the code: an address bar that takes `localhost:3000` as typed, back, forward and reload, the Web Inspector on `⌥⌘I`, and its own cookies and storage, apart from the app's. On Linux a tab shows localhost pages only.
- **Open a dev server's port from the terminal.** A terminal pane's right-click menu lists the ports its programs listen on, and the address bar lists every port the worktree's terminals serve.
- **Device mode (`⇧⌘M`).** Eleven phones and tablets drawn around the page, notch, corners and bezel included, upright or turned, or Responsive at any size you drag it to; the page gets the device's width, pixel ratio and user agent.
- **Pick an element for the agent (`⇧⌘C`).** Click an element on the page, or ⇧-click a few first, and ask your question beside it: a new agent session starts in the worktree's terminal with the page's address and device, each element's selector, React components, styles and picture, and the question sent, or waiting in its prompt box with Skip. The page's errors can go along.
- **The page's console.** A badge counts the page's errors and warnings, failed requests included; the panel lists them, and Send Errors to Agent asks about them the same way.
- **A model to ask with.** Ask Agent's question box, in a review or a browser tab, picks the model Claude Code starts with; each remembers its last, and starts from Guided Review's.
- **Find in the page, zoom it, and show it light or dark.** `⌘F` with `⌘G` / `⇧⌘G`, `⌘=` / `⌘−` / `⌘0` from 50% to 300%, and a switch for the page's `prefers-color-scheme`; each tab keeps its zoom and scheme.
- **Agents can use the browser.** Turn it on in Settings → Browser, and each terminal pane's agent gets a tab of its own, opened in the background: `gitviber browser` opens pages, reads them as a tree it can act on, clicks, types, waits, takes screenshots, reads the console and shows the page as a device. `gitviber browser help` lists every command; Settings has a note to paste into `CLAUDE.md` or `AGENTS.md`.
- **Pages give their memory back.** A page out of sight closes after a while and comes back where it was, its picture standing in until it loads; Settings → Browser sets how many stay open and for how long, can reload a worktree's pages when its agent finishes, and clears the browsing data.
- **Numbered terminal panes.** Each pane of a split shows its number in its title bar, counted row by row from the top as they're drawn, the focused one's in the accent color. The number shows over the pane for a moment as the keys move to it.
- **A pager while one pane is zoomed.** With a split's pane filling the panel (`⇧⌘↵`), the terminal's toolbar shows which pane it is (‹ 2 / 3 ›) and a dot for each, so an agent working in another pane stays in sight. Click an arrow or a dot to show another pane, still zoomed.
- **A dot per pane on a split terminal's tab,** colored by the state of the agent in it, the focused pane's larger. The tab's tooltip lists each pane by number with what runs there.

### Changed

- **`⇧⌘←` / `⇧⌘→` page through a split's panes by number,** wrapping around from the last to the first. `⌥⌘` arrows, and `⇧⌘↑` / `⇧⌘↓`, still move to the pane on that side.
- **The Agents menu groups agents by terminal tab,** the tab with an agent that needs you first, and shows which pane of a split each one runs in (2 / 3) with the pane's name.
- **Explain Commit and Guided Review show for everyone,** not only with commit message suggestions turned on.
- **The Updates page checks for an update when it opens,** while automatic checks are on.
- **Esc in an empty explorer search goes back to the files,** after the first Esc clears it.

### Fixed

- **A merge tool that fails leaves no copies behind** in the temp folder.
- **Inline code on a see-through window** is a tint of the text instead of a white chip on light themes.

## [0.1.12] - 2026-10-08

### Fixed

- **Dragging a sidebar's edge follows the pointer.** After a sidebar was closed and reopened, a drag could pull it back to its old width mid-move, so it jumped left and right. In #118.

## [0.1.11] - 2026-10-08

### Added

- **Explain Commit and Guided Review.** Right-click a commit in History, or press ✦ in a branch review, and your own agent CLI writes a guide to the change: an overview, a diagram of the data and flow it touches when there is one, then sections in reading order, each with its category (UI, API, security, tests…), its importance and a one-line check, beside its diffs. Mark sections and files reviewed as you go; notes from the agent sit on the files and lines they're about. Big changes work too: every changed file reaches the agent. In #117.
- **A Guided Review page in Settings,** to run reviews with their own agent, model and effort, and in your language. Commit messages and pull requests stay in English.
- **Reasoning effort for suggestions.** Commit messages, pull request descriptions and guided reviews take an effort level (medium by default) next to the model.
- **Settings in a window of its own.** Open it from the Settings dialog, park it beside the app and every change applies live.
- **Window opacity and Background blur,** in Settings → Appearance: the whole window turns see-through over the desktop, sharp or blurred, while text, dialogs and the terminal stay solid. Each theme stops where its text stays readable.
- **CI in the commit header.** A badge sums up a commit's checks, and a click lists each one: what a running check waits on and since when, and why a failed one failed.
- **Click an author or label in a row to filter by it,** in History, pull requests and issues. The row's menu does the same from the keyboard.
- **Agent dots in color.** A working agent pulses orange, one that finished while you were away is blue, one asking a question is red, on terminal tabs, panes, the agents list and the worktree picker.
- **`gitviber .` in the built-in terminal** works with nothing installed (macOS).
- **Ahead and behind in the branch picker,** plus "local only" and "upstream gone" for each local branch.
- **Keys for Git:** Focus Commit Message (⌃⌘C), Fetch (⌃⇧⌘T), Pull (⌃⇧⌘P) and Push (⌃⌘P).
- **Equalize terminal panes** by double-clicking a divider, or from the command palette.
- **Drop a folder on the window** to open it as a project, or a file from a repository to open it there.
- **The window keeps its size and place,** and its title names the repository and branch.

### Changed

- **Quitting asks first while an agent is working or a command is running,** naming what would stop. A second ⌘Q quits; a switch in Settings → Terminal turns the question off.
- **Suggestions from Claude Code start faster:** they skip MCP servers, skills and the saved session, run Sonnet by default, and get only read-only tools.
- **The sidebar's tabs keep their place.** Switching between Changes, History, pull requests and issues keeps each one's filters, loaded pages, open commit and scroll.
- **Relative times stay current,** and show the full date on hover; long titles show in full on hover.
- **Find the Bad Commit asks before it checks out commits,** and the bisect buttons say what each one means.
- **Projects whose folder is gone sink to the bottom** of the project list.

### Fixed

- **The sidebars no longer shrink by themselves.** They and the terminal keep their width in pixels through window resizes, display changes and restarts; double-clicking a sidebar's edge fits it to its header, and a narrow sidebar scrolls only its tab row. In #118.
- **The ⌘ shortcuts overlay** no longer stays up after the Mission Control gesture, and opens again afterwards.
- **A drag cut short by Mission Control or ⌘Tab** ends at the next mouse move instead of dropping on the next click.
- **Squash and Reword keep your message** when a warning is cancelled or the rewrite fails.
- **Renaming a stash while another is being stashed** can no longer drop the other one.
- **Move main back run twice at once** can no longer undo itself.
- **An agent that left a background shell running** shows as finished, not working, and its finish is notified.
- **Inline code in documents and descriptions** is visible on light themes.

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

[Unreleased]: https://github.com/emircan-sahin/gitviber/compare/v0.1.16...HEAD
[0.1.16]: https://github.com/emircan-sahin/gitviber/compare/v0.1.15...v0.1.16
[0.1.15]: https://github.com/emircan-sahin/gitviber/compare/v0.1.14...v0.1.15
[0.1.14]: https://github.com/emircan-sahin/gitviber/compare/v0.1.13...v0.1.14
[0.1.13]: https://github.com/emircan-sahin/gitviber/compare/v0.1.12...v0.1.13
[0.1.12]: https://github.com/emircan-sahin/gitviber/compare/v0.1.11...v0.1.12
[0.1.11]: https://github.com/emircan-sahin/gitviber/compare/v0.1.10...v0.1.11
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
