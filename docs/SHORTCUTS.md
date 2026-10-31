# Shortcuts

Back to the [README](../README.md). Hold `⌘` in the app, or press `⌘/`, to see the ones that work where you are.

## Presets

Settings → Keyboard Shortcuts → Preset picks the keys every command starts with: **VS Code**, the default, or **GitViber (classic)**, the keys GitViber had before presets. Keys you rebind stay as you set them under either. The table below lists the VS Code preset; [the classic preset](#the-classic-preset) lists where it differs.
Someone who used GitViber before presets is moved to the VS Code preset, with a note that offers the classic keys; someone who had rebound keys stays on the classic preset, with a note saying so. The note shows at each launch until you close it or click one of its buttons.

The left panel is the primary side bar (`⌘B` toggles it, `⌘0` focuses it) and the right one, the explorer and search, the secondary side bar (`⌥⌘B`).
Where a place has its own key, that key wins while focus is there: in the terminal `⌥⌘←` `⌥⌘→` focus the pane on that side, while elsewhere they switch tabs; `⌘=` `⌘−` `⌘0` size the terminal's font, a browser page or a rendered document's text instead of the interface and the side bar.

## Keys

Every shortcut below can be rebound in Settings (`⌘,`), except moving around a list or view (arrows, `Home` `End` `PgUp` `PgDn`, `↵`, `Space`, `Esc`), `⌃G`, and `⌘G` `⇧⌘G` in the terminal.
While you type, only shortcuts with `⌘`, `⌃` or an F-key apply. `⌘` with an arrow (but `⌥⌘←` `⌥⌘→`) and the `⌃` letters macOS edits with (`⌃A` `⌃E` `⌃K` and the like, `⌃⇧A` `⌃⇧E` to select) edit the text instead, and the git actions on `⌘Z` `⇧⌘Z` `⌘⌫` `⌥⌘S` `⌥⌘N` `⇧⌘⌫` don't run there.

On Linux `⌘` is Ctrl, tabs are on Alt+1–8 (Alt+0 or Alt+9 the last; they work in text fields, but not in the terminal, where Alt+digit is the shell's repeat count), Ctrl+PageDown / Ctrl+PageUp and Ctrl+Tab / Ctrl+Shift+Tab switch tabs, Changes is Ctrl+Shift+G, a new terminal Ctrl+Shift+`, link labels are Ctrl+Shift+Space, and the terminal's
`⌘D` `⌘K` `⌘W` are Ctrl+Shift+D (or Ctrl+Shift+5), K and W, since Ctrl+letter stays the shell's (split down is Ctrl+Alt+Shift+D,
zooming a pane Ctrl+Shift+Enter, maximizing Ctrl+Alt+Shift+Enter, the previous / next pane Super+Alt+← / →, copy and paste Ctrl+Shift+C / V; focusing a pane by side has no default there, the shell and the desktop use those keys, and neither has renaming a pane, since Ctrl+R is the shell's history search).
Revealing the file in the file manager, closing the other tabs, focusing the commit message or the terminal's tabs, and fetch, pull and push have no default there either.

| Keys | |
| --- | --- |
| `⌘,` | Settings |
| Hold `⌘`, or `⌘/` | Show the shortcuts that work where you are (the hold can be turned off in Settings) |
| `⇧⌘P` or `F1`, `⌘P` | Command palette, quick open |
| `⌃R` | Switch project (not in the terminal, where it is the shell's history search) |
| `↓` `↑` `Home` `End` `PgUp` `PgDn`, `↵`, `Esc` | Move through a picker or list (project, branch, worktree, Open in…), pick the row, close it with focus back where it was; with text in its search field `Home` `End` stay the field's |
| `⌘F` `⇧⌘F` | Find where focus is, find in files |
| `⌥⌘F`, or `/` in the list | Search the history |
| `⌃⇧G` | Changes; pressed again there, focus goes back where it was (History, PRs and Issues have no default key) |
| `⌃1` … `⌃8`, `⌃0` `⌃9` | Go to tab 1 to 8, the last tab; the terminal's while it has focus |
| `⌥⌘→` `⌥⌘←`, `⇧⌘]` `⇧⌘[`, `⌃⇥` `⌃⇧⇥` | Next / previous tab; the terminal's while it has focus, though in a terminal pane `⌥⌘←` `⌥⌘→` focus the pane on that side |
| `⇧⌘T` `⌥⌘T` | Reopen the closed tab, close the other tabs |
| `J` `K` | Next / previous changed file |
| `↓` `↑` `Space` `↵` | Move through the Changes list, open the file, keep its tab open; in History `↵` opens or closes the commit |
| `↓` `↑` `←` `→` `↵` `Home` `End` | Move through the explorer, close or open a folder, open a file (`→` with focus in its code, `↵` keeping its tab) |
| `⇧↓` `⇧↑`, `Esc` | Pick a range of rows in Changes or History, let the picked rows go |
| `←` `→` `Esc` | Previous / next commit in a pull request's Files changed while one is picked, all of them again |
| `V` | Mark file viewed |
| `C` | Add a review note on the selected lines, else the cursor's; in a pull request, a line comment |
| `S` `⌘⌫` | Stage or unstage, discard the open file (in the Changes list); in the explorer `⌘⌫` deletes the file |
| `⌘A` | Select every change (in the Changes list) |
| `⌘Z` `⇧⌘Z` | Undo, redo the last git action (outside text fields) |
| `F7` `⇧F7`, `⌥F5` `⇧⌥F5`, `⌥↓` `⌥↑` | Next / previous change |
| `⌥⌘S` `⌥⌘N` `⇧⌘⌫` | Stage, unstage, discard the selected lines, else the change at the cursor (next / previous change puts it there) |
| `⌥S` `⌥C` `⌥W` `⌥Z` | Split view, collapse unchanged, ignore whitespace, word wrap |
| `F12` or `⌘↵`, `⌥F12`, `⇧F12` | Go to definition (`⌘`-click too), peek it, find references (in the code view) |
| `⌃G` | Go to line (in the code view) |
| `=` `−` `0`, also with `⌘` | Zoom an image in, out, to fit (in the image view) |
| `⌘S` | Save the file you're editing |
| `F2` | Rename the file, branch or worktree (in the explorer, branch picker or worktree picker) |
| `T` `M` `⌫` | Open a terminal in the worktree, merge its branch, remove it (in the worktree picker) |
| `⌫` | Remove a project from the list (in the project list) |
| `←` `→` | Collapse, expand a folder (in the project picker, with nothing typed) |
| `⇧⌘O`, `⌥⌘R` | Open in the external app you last picked, reveal the file in Finder |
| `⌘B` `⌥⌘B` | Toggle the git panel, the right panel with the explorer |
| `⌘0` `⌘1` or `⌘E` `⇧⌘E` | Focus the git panel, the code view, the file explorer (switching the right panel to it from Search); `⌘0` `⇧⌘E` pressed again there give focus back where it was |
| `F6` `⇧F6` | Focus the next / previous panel (not from the terminal, which keeps F-keys for its programs) |
| `⌃` `` ` `` or `⌘J` | Toggle the terminal; hiding it gives focus back where it was before it showed (`⌘B` `⌥⌘B` too) |
| `⌃⇧` `` ` `` or `⌘T` | New terminal |
| `⌃⌘T` | Focus the terminal's tabs, where `←` `→` switch, `⌥←` `⌥→` move one, `⌘W` closes it and `↵`, `Esc` or `⌃⌘T` again goes back (in the terminal) |
| `⌘\` or `⌘D`, `⇧⌘D`, `⌘K` | Split right, split down, clear (in the terminal) |
| `⌘G` `⇧⌘G` | Next / previous match of the terminal's find (in the terminal) |
| `⇧⌘←` `⇧⌘→` | Previous / next terminal pane by its number, wrapping around; zoomed, the pane shown (in the terminal) |
| `⌥⌘←` `⌥⌘→` `⌥⌘↑` `⌥⌘↓`, `⇧⌘↑` `⇧⌘↓` | Focus the terminal pane on that side (in the terminal) |
| `⌘↵` | Maximize the terminal over the workspace, and back (in the terminal) |
| `⇧⌘↵` | Show only the focused terminal pane in the panel, and all of them again (in the terminal) |
| `⌘R` | Rename the focused pane of a split terminal (in the terminal; a double-click on its header too) |
| `⌘↑` `⌘↓` | Scroll to the previous / next command's prompt (in the terminal, with shell integration) |
| `⌃⇧Space` | Label the links on screen: type a label to open its link, with Shift to copy it (in the terminal) |
| `⌘L` `⌘R` `⌘[` `⌘]` | Address bar, reload, back, forward (in a browser tab) |
| `⌘F` `⌘G` `⇧⌘G` | Find in the page, next / previous match (in a browser tab) |
| `⇧⌘C` `⇧⌘M` `⌥⌘I` | Pick an element for the agent, device mode, Web Inspector (in a browser tab) |
| `⇧⌘R` | Reader mode: the page's article set for reading; again, the page as it was (in a browser tab) |
| `⌘=` `⌘−` | Zoom the interface; in the terminal, its font size (a pinch or Ctrl+scroll too); in a browser tab, the page; in a rendered document, its text |
| `⌘0` | In the terminal, a browser tab or a rendered document: back to the normal size (elsewhere it focuses the git panel) |
| `⌥⌘=` `⌥⌘−` `⌥⌘0` | Code font size |
| `⌘W` | Close tab, or the terminal pane in focus |
| `⌃⌘C` | Focus the commit message (in the Changes panel); again, back where focus was |
| `⌘↵` | Commit (in the commit message); create the tag (in its message); post the comment (in an issue's comment) |
| `⌃⇧⌘T` `⌃⇧⌘P` `⌃⌘P` | Fetch, pull, push |
| `⌘O` | Open repository |

## The classic preset

Where **GitViber (classic)** differs from the VS Code preset; every other key is the same.

| Command | VS Code | Classic |
| --- | --- | --- |
| Command palette | `⇧⌘P` `F1` | `⇧⌘P` |
| Switch project | `⌃R` | none |
| Reveal in Finder | `⌥⌘R` | none |
| Changes, History, PRs, Issues | `⌃⇧G`, none, none, none | `⌃1` `⌃2` `⌃3` `⌃4` |
| Focus the git panel | `⌘0` | `⇧⌘G` |
| Focus the code view | `⌘1` `⌘E` | `⌘E` |
| Reset the zoom | none | `⌘0` |
| Go to tab 1–8, the last tab | `⌃1` … `⌃8`, `⌃0` `⌃9` | `⌘1` … `⌘8`, `⌘9` |
| Next / previous tab | `⌥⌘→` `⌥⌘←`, `⇧⌘]` `⇧⌘[`, `⌃⇥` `⌃⇧⇥` | `⇧⌘]` `⇧⌘[`, `⌘→` `⌘←`, `⌃⇥` `⌃⇧⇥` |
| Next / previous change | `F7` `⇧F7`, `⌥F5` `⇧⌥F5`, `⌥↓` `⌥↑` | `F7` `⇧F7`, `⌥↓` `⌥↑` |
| Toggle the terminal | `⌃` `` ` `` `⌘J` | `⌘J` `⌃` `` ` `` |
| New terminal | `⌃⇧` `` ` `` `⌘T` | `⌘T` |
| Split the terminal right | `⌘\` `⌘D` | `⌘D` |
| Open a browser tab | none | `⇧⌘B` |

On Linux the classic preset keeps the views on Alt+1–4, tabs on Ctrl+1–8 and Ctrl+9, a new terminal on Ctrl+Shift+T, and leaves reopening a closed tab unbound.
