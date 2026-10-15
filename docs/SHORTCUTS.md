# Shortcuts

Back to the [README](../README.md). Hold `⌘` in the app, or press `⌘/`, to see these on screen.

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
| `⇧⌘←` `⇧⌘→` | Previous / next terminal pane by its number, wrapping around; zoomed, the pane shown (in the terminal) |
| `⌥⌘←` `⌥⌘→` `⌥⌘↑` `⌥⌘↓`, `⇧⌘↑` `⇧⌘↓` | Focus the terminal pane on that side (in the terminal) |
| `⌘↵` | Maximize the terminal over the workspace, and back (in the terminal) |
| `⇧⌘↵` | Show only the focused terminal pane in the panel, and all of them again (in the terminal) |
| `⌘R` | Rename the focused pane of a split terminal (in the terminal; a double-click on its header too) |
| `⌘↑` `⌘↓` | Scroll to the previous / next command's prompt (in the terminal, with shell integration) |
| `⌃⇧Space` | Label the links on screen: type a label to open its link, with Shift to copy it (in the terminal) |
| `⇧⌘B` | Open a browser tab |
| `⌘L` `⌘R` `⌘[` `⌘]` | Address bar, reload, back, forward (in a browser tab) |
| `⌘F` `⌘G` `⇧⌘G` | Find in the page, next / previous match (in a browser tab) |
| `⇧⌘C` `⇧⌘M` `⌥⌘I` | Pick an element for the agent, device mode, Web Inspector (in a browser tab) |
| `⌘=` `⌘−` `⌘0` | Zoom the interface; in the terminal, its font size (a pinch or Ctrl+scroll too); in a browser tab, the page |
| `⌥⌘=` `⌥⌘−` `⌥⌘0` | Code font size |
| `⌘W` | Close tab, or the terminal pane in focus |
| `⌃⌘C` | Focus the commit message, from anywhere |
| `⌘↵` | Commit (in the commit message) |
| `⌃⇧⌘T` `⌃⇧⌘P` `⌃⌘P` | Fetch, pull, push |
| `⇧⌘R` | Reload the window, which ends every terminal |
| `⌘O` | Open repository |
