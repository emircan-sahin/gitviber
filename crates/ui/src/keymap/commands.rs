//! Every command in one place: its id (the key a user's rebinding is stored under), title,
//! category and default chords, and the GPUI action that runs it. A chord is a canonical
//! string such as "shift+cmd+e": modifiers in ⌃⌥⇧⌘ order, then the key (see `chord`). When two
//! commands share a chord, the one listed first wins. Off macOS "cmd" is Ctrl and "ctrl" the
//! Super key.
//!
//! Feature crates handle these with `.on_action(cx.listener(|this, _: &git::Pull, window, cx| ..))`
//! on their element; a command shows in the palette and the menu bar is enabled only while
//! something in focus handles it.

use gpui_kit::Action;

use super::Category::*;
use super::Command;

macro_rules! modules {
    ($($module:ident: [$($action:ident),* $(,)?],)*) => {
        $(
            pub mod $module {
                gpui_kit::actions!($module, [$($action),*]);
            }
        )*
    };
}
modules! {
workbench: [ShowCommands, QuickOpen, OpenChange, OpenSettings, ShortcutOverlay],
    file: [OpenRepo, CloneRepo, SwitchProject, Save, Reveal, RevealInExplorer, OpenIn],
    window: [Minimize, Zoom, ToggleFullScreen, BringAllToFront],
    repo: [Refresh],
    tab: [Close, Goto1, Goto2, Goto3, Goto4, Goto5, Goto6, CloseOthers, CloseLeft, CloseRight, CloseAll, ReopenClosed, Goto7, Goto8, Last, Next, Prev, MoveLeft, MoveRight],
    view: [Changes, History, Pulls, Issues, ToggleGitPanel, ToggleExplorer, FocusGitPanel, FocusCode, ShowExplorer, FocusNextPanel, FocusPrevPanel, ZoomIn, ZoomOut, ZoomReset],
    history: [Search, Find],
    changes: [FocusMessage],
    review: [NextFile, PrevFile, ToggleViewed, Branch, GuideUncommitted, OpenAll, OpenAllStaged, OpenAllBranch, OpenAllCommit, OpenAllComparison, AddNote, CopyNotes, SendNotes],
    diff: [NextChange, PrevChange, StageChange, UnstageChange, DiscardChange, ToggleSplit, ToggleCollapse, ToggleWhitespace],
    editor: [ToggleWrap, ToggleBlame, CopyGitHubLink, OpenOnGitHub, FontZoomIn, FontZoomOut, FontZoomReset, Find, GoToDefinition, PeekDefinition, GoToReferences],
    search: [FindInFiles],
    media: [ZoomIn, ZoomOut, Fit],
    terminal: [Toggle, NewInProject, ShowAgents, ResumeConversation, New, Split, SplitDown, Clear, Close, PrevPane, NextPane, FocusLeft, FocusRight, FocusUp, FocusDown, ToggleMaximize, ZoomPane, EqualizePanes, RenamePane, FontZoomIn, FontZoomOut, FontZoomReset, PrevCommand, NextCommand, Hints, FocusTabs],
    browser: [Open, FocusAddress, Reload, Back, Forward, Inspect, Pick, ToggleDevice, ClearData, Find, FindNext, FindPrev, ZoomIn, ZoomOut, ZoomReset],
    explorer: [Rename, Delete],
    git: [SwitchBranch, NewBranch, CompareBranches, SwitchWorktree, NewWorktree, Fetch, Pull, Push, Sync, ApplyPatch, Undo, Redo, ToggleStage, Discard, Commit, RenameBranch, SelectAllChanges, CreateTag, SuggestMessage],
    worktree: [OpenTerminal, Merge, Rename, Remove],
    project: [Forget],
    github: [PostComment],
    app: [About, CheckForUpdates, InstallCli, Quit, Hide, HideOthers, ShowAll],
    help: [Readme, Shortcuts, ReportBug, CopyDiagnostics, ShowLogs, ReleaseNotes, License],
}

/// As the file manager is named where the file opens.
const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    "Reveal in Finder"
} else if cfg!(windows) {
    "Show in Explorer"
} else {
    "Show in Folder"
};

const fn cmd<A: Action + Default>(
    id: &'static str,
    title: &'static str,
    category: super::Category,
    keys: &'static [&'static str],
) -> Command {
    Command::new(id, title, category, keys, new_action::<A>)
}

fn new_action<A: Action + Default>() -> Box<dyn Action> {
    Box::new(A::default())
}

pub static COMMANDS: &[Command] = &[
    cmd::<workbench::ShowCommands>(
        "workbench.showCommands",
        "Command Palette",
        General,
        &["shift+cmd+p"],
    ),
    cmd::<workbench::QuickOpen>("workbench.quickOpen", "Open File", General, &["cmd+p"]),
    cmd::<workbench::OpenChange>("workbench.openChange", "Open Changed File", General, &[]),
    cmd::<workbench::OpenSettings>(
        "workbench.openSettings",
        "Open Settings",
        General,
        &["cmd+,"],
    ),
    // Holding ⌘ alone shows it too (ShortcutOverlay).
    cmd::<workbench::ShortcutOverlay>(
        "workbench.shortcutOverlay",
        "Show Shortcut Overlay",
        General,
        &["cmd+/"],
    ),
    cmd::<file::OpenRepo>("file.openRepo", "Open Repository", General, &["cmd+o"]),
    cmd::<file::CloneRepo>("file.cloneRepo", "Clone Repository", General, &[]),
    cmd::<file::SwitchProject>("file.switchProject", "Switch Project", General, &[]),
    // The file watcher refreshes on its own; this is for changes it can't see.
    cmd::<repo::Refresh>("repo.refresh", "Refresh", General, &[]),
    cmd::<tab::Close>("tab.close", "Close Tab", General, &["cmd+w"]),
    // The file open in the file view, typed into (lib/editor/edits).
    cmd::<file::Save>("file.save", "Save", General, &["cmd+s"]),
    cmd::<file::Reveal>("file.reveal", REVEAL_LABEL, General, &[]),
    cmd::<file::RevealInExplorer>(
        "file.revealInExplorer",
        "Reveal in Explorer View",
        General,
        &[],
    ),
    // The app last picked under "Open in"; the list of apps until there is one.
    cmd::<file::OpenIn>(
        "file.openIn",
        "Open in External App",
        General,
        &["shift+cmd+o"],
    ),
    // ⌘1–⌘9 pick tabs, as in browsers. Listed before the tabs, so a user who bound ⌘1 here keeps it.
    // Elsewhere "ctrl" is the Super key, which the OS takes; Alt+1–4 is the usual there.
    cmd::<view::Changes>("view.changes", "Focus Changes", View, &["ctrl+1"]).other(&["alt+1"]),
    cmd::<view::History>("view.history", "Focus History", View, &["ctrl+2"]).other(&["alt+2"]),
    cmd::<view::Pulls>("view.pulls", "Focus Pull Requests", View, &["ctrl+3"]).other(&["alt+3"]),
    cmd::<view::Issues>("view.issues", "Focus Issues", View, &["ctrl+4"]).other(&["alt+4"]),
    // ⌘F is Find, which searches the history list too while focus is there.
    cmd::<history::Search>("history.search", "Search History", View, &["alt+cmd+f"]),
    cmd::<view::ToggleGitPanel>("view.toggleGitPanel", "Toggle Git Panel", View, &["cmd+b"]),
    cmd::<view::ToggleExplorer>(
        "view.toggleExplorer",
        "Toggle Explorer",
        View,
        &["alt+cmd+b"],
    ),
    cmd::<view::FocusGitPanel>(
        "view.focusGitPanel",
        "Focus Git Panel",
        View,
        &["shift+cmd+g"],
    ),
    cmd::<view::FocusCode>("view.focusCode", "Focus Code View", View, &["cmd+e"]),
    // Go to Commit Message. Not ⌘G, macOS's Find Next: ⌃⌘, as fetch, pull and push,
    // which neither macOS, Monaco nor the terminal uses with C. Elsewhere ⌃ is the Super key: unbound.
    cmd::<changes::FocusMessage>(
        "changes.focusMessage",
        "Focus Commit Message",
        View,
        &["ctrl+cmd+c"],
    )
    .other(&[]),
    // The id is from when it only showed the panel; kept so a user's binding for it still applies.
    cmd::<view::ShowExplorer>(
        "view.showExplorer",
        "Focus Explorer",
        View,
        &["shift+cmd+e"],
    ),
    cmd::<view::FocusNextPanel>("view.focusNextPanel", "Focus Next Panel", View, &["f6"]),
    cmd::<view::FocusPrevPanel>(
        "view.focusPrevPanel",
        "Focus Previous Panel",
        View,
        &["shift+f6"],
    ),
    cmd::<view::ZoomIn>("view.zoomIn", "Zoom In", View, &["cmd+=", "shift+cmd+="]),
    cmd::<view::ZoomOut>("view.zoomOut", "Zoom Out", View, &["cmd+-"]),
    cmd::<view::ZoomReset>("view.zoomReset", "Reset Zoom", View, &["cmd+0"]),
    cmd::<tab::Goto1>("tab.goto1", "Go to Tab 1", Tabs, &["cmd+1"]),
    cmd::<tab::Goto2>("tab.goto2", "Go to Tab 2", Tabs, &["cmd+2"]),
    cmd::<tab::Goto3>("tab.goto3", "Go to Tab 3", Tabs, &["cmd+3"]),
    cmd::<tab::Goto4>("tab.goto4", "Go to Tab 4", Tabs, &["cmd+4"]),
    cmd::<tab::Goto5>("tab.goto5", "Go to Tab 5", Tabs, &["cmd+5"]),
    cmd::<tab::Goto6>("tab.goto6", "Go to Tab 6", Tabs, &["cmd+6"]),
    // Elsewhere ⇧Ctrl+T opens a terminal (terminal.new): unbound there.
    cmd::<tab::CloseOthers>("tab.closeOthers", "Close Other Tabs", Tabs, &["alt+cmd+t"]).other(&[]),
    cmd::<tab::CloseLeft>("tab.closeLeft", "Close Tabs to the Left", Tabs, &[]),
    cmd::<tab::CloseRight>("tab.closeRight", "Close Tabs to the Right", Tabs, &[]),
    cmd::<tab::CloseAll>("tab.closeAll", "Close All Tabs", Tabs, &[]),
    cmd::<tab::ReopenClosed>(
        "tab.reopenClosed",
        "Reopen Closed Tab",
        Tabs,
        &["shift+cmd+t"],
    )
    .other(&[]),
    cmd::<tab::Goto7>("tab.goto7", "Go to Tab 7", Tabs, &["cmd+7"]),
    cmd::<tab::Goto8>("tab.goto8", "Go to Tab 8", Tabs, &["cmd+8"]),
    cmd::<tab::Last>("tab.last", "Go to Last Tab", Tabs, &["cmd+9"]),
    // ⌘←/⌘→ skip text fields and the terminal (see runsWhileTyping); ⇧⌘] shows in the menu as it works everywhere.
    // Elsewhere Ctrl+Tab ("cmd" there); Ctrl+← / Ctrl+→ move by word.
    cmd::<tab::Next>(
        "tab.next",
        "Next Tab",
        Tabs,
        &["shift+cmd+]", "cmd+right", "ctrl+tab"],
    )
    .other(&["cmd+tab", "shift+cmd+]"]),
    cmd::<tab::Prev>(
        "tab.prev",
        "Previous Tab",
        Tabs,
        &["shift+cmd+[", "cmd+left", "ctrl+shift+tab"],
    )
    .other(&["shift+cmd+tab", "shift+cmd+["]),
    cmd::<tab::MoveLeft>("tab.moveLeft", "Move Tab Left", Tabs, &["alt+left"]).local("TabStrip"),
    cmd::<tab::MoveRight>("tab.moveRight", "Move Tab Right", Tabs, &["alt+right"])
        .local("TabStrip"),
    cmd::<review::NextFile>("review.nextFile", "Next Changed File", Review, &["j"]),
    cmd::<review::PrevFile>("review.prevFile", "Previous Changed File", Review, &["k"]),
    cmd::<review::ToggleViewed>("review.toggleViewed", "Toggle File Viewed", Review, &["v"]),
    // Changes then lists the branch's commits and uncommitted work since it left the default branch.
    cmd::<review::Branch>("review.branch", "Review Branch Against Base", Review, &[]),
    // The agent's guided review of what isn't committed yet: staged, unstaged and new files.
    cmd::<review::GuideUncommitted>(
        "review.guideUncommitted",
        "Guided Review of Uncommitted Changes",
        Review,
        &[],
    ),
    // Every file of a Changes list in one scroll (the branch's while Changes reviews it).
    cmd::<review::OpenAll>("review.openAll", "Open All Changes", Review, &[]),
    cmd::<review::OpenAllStaged>(
        "review.openAllStaged",
        "Open All Staged Changes",
        Review,
        &[],
    ),
    cmd::<review::OpenAllBranch>(
        "review.openAllBranch",
        "Open All Branch Changes",
        Review,
        &[],
    ),
    // From a file of a commit or a comparison on show: every file of it in one scroll.
    cmd::<review::OpenAllCommit>(
        "review.openAllCommit",
        "Open All Changes in This Commit",
        Review,
        &[],
    ),
    cmd::<review::OpenAllComparison>(
        "review.openAllComparison",
        "Open All Changes in This Comparison",
        Review,
        &[],
    ),
    // On the selected lines of the diff or file on show, else the cursor's (lib/review/notes).
    cmd::<review::AddNote>("review.addNote", "Add Review Note", Review, &["c"]),
    cmd::<review::CopyNotes>(
        "review.copyNotes",
        "Copy Review Notes as Prompt",
        Review,
        &[],
    ),
    cmd::<review::SendNotes>(
        "review.sendNotes",
        "Send Review Notes to Terminal",
        Review,
        &[],
    ),
    cmd::<diff::NextChange>("diff.nextChange", "Next Change", Diff, &["f7", "alt+down"]),
    cmd::<diff::PrevChange>(
        "diff.prevChange",
        "Previous Change",
        Diff,
        &["shift+f7", "alt+up"],
    ),
    // The selected lines, else the change at the cursor (which Next / Previous Change put there). With no two-key
    // chords, stage and unstage are ⌥⌘S and ⌥⌘N. ⌥⌘R is Monaco's (Toggle Regex in Find) and ⌘R, Reload
    // Window's until it moved to ⇧⌘R, names a terminal pane, so discard is ⌘⌫ (the file's) with ⇧. Not from text fields.
    cmd::<diff::StageChange>(
        "diff.stageChange",
        "Stage Change or Selected Lines",
        Diff,
        &["alt+cmd+s"],
    )
    .outside_text()
    .no_repeat(),
    cmd::<diff::UnstageChange>(
        "diff.unstageChange",
        "Unstage Change or Selected Lines",
        Diff,
        &["alt+cmd+n"],
    )
    .outside_text()
    .no_repeat(),
    cmd::<diff::DiscardChange>(
        "diff.discardChange",
        "Discard Change or Selected Lines",
        Diff,
        &["shift+cmd+backspace"],
    )
    .outside_text()
    .no_repeat(),
    cmd::<diff::ToggleSplit>(
        "diff.toggleSplit",
        "Toggle Unified / Split",
        Diff,
        &["alt+s"],
    ),
    cmd::<diff::ToggleCollapse>(
        "diff.toggleCollapse",
        "Toggle Collapse Unchanged",
        Diff,
        &["alt+c"],
    ),
    cmd::<diff::ToggleWhitespace>(
        "diff.toggleWhitespace",
        "Toggle Ignore Whitespace",
        Diff,
        &["alt+w"],
    ),
    cmd::<editor::ToggleWrap>("editor.toggleWrap", "Toggle Word Wrap", Editor, &["alt+z"])
        .in_editor(),
    cmd::<editor::ToggleBlame>("editor.toggleBlame", "Toggle Blame", Editor, &[]),
    // A permalink to the selected lines, else the file, at a commit GitHub has (lib/github/permalink).
    cmd::<editor::CopyGitHubLink>("editor.copyGitHubLink", "Copy GitHub Link", Editor, &[]),
    cmd::<editor::OpenOnGitHub>("editor.openOnGitHub", "Open on GitHub", Editor, &[]),
    cmd::<editor::FontZoomIn>(
        "editor.fontZoomIn",
        "Increase Code Font Size",
        Editor,
        &["alt+cmd+="],
    ),
    cmd::<editor::FontZoomOut>(
        "editor.fontZoomOut",
        "Decrease Code Font Size",
        Editor,
        &["alt+cmd+-"],
    ),
    cmd::<editor::FontZoomReset>(
        "editor.fontZoomReset",
        "Reset Code Font Size",
        Editor,
        &["alt+cmd+0"],
    ),
    // Searches where focus is.
    cmd::<editor::Find>("editor.find", "Find", General, &["cmd+f"]),
    // From the code too, where the editor would open its Replace on ⇧⌘F.
    cmd::<search::FindInFiles>(
        "search.findInFiles",
        "Find in Files",
        General,
        &["shift+cmd+f"],
    )
    .over_text(),
    // Where the name, import or path under the cursor is defined; ⌘-click does the same (lib/editor/definitions).
    // In a file's code and in a diff's.
    cmd::<editor::GoToDefinition>(
        "editor.goToDefinition",
        "Go to Definition",
        Editor,
        &["f12", "cmd+enter"],
    )
    .local("CodeView || DiffView")
    .over_text(),
    cmd::<editor::PeekDefinition>(
        "editor.peekDefinition",
        "Peek Definition",
        Editor,
        &["alt+f12"],
    )
    .local("CodeView || DiffView"),
    // Where it's used; ⌘-click on a definition itself does the same.
    cmd::<editor::GoToReferences>(
        "editor.goToReferences",
        "Go to References",
        Editor,
        &["shift+f12"],
    )
    .local("CodeView || DiffView"),
    // The window's zoom keys zoom the picture in the image view, as the browser's zoom its page.
    cmd::<media::ZoomIn>(
        "media.zoomIn",
        "Zoom In Image",
        Editor,
        &["=", "shift+=", "cmd+=", "shift+cmd+="],
    )
    .local("ImageView"),
    cmd::<media::ZoomOut>("media.zoomOut", "Zoom Out Image", Editor, &["-", "cmd+-"])
        .local("ImageView"),
    cmd::<media::Fit>("media.fit", "Fit Image", Editor, &["0", "cmd+0"]).local("ImageView"),
    // ⌃` and ⌘J. Elsewhere Ctrl+` and Ctrl+J.
    cmd::<terminal::Toggle>(
        "terminal.toggle",
        "Toggle Terminal",
        Terminal,
        &["cmd+j", "ctrl+`"],
    )
    .other(&["cmd+j", "cmd+`"]),
    cmd::<terminal::NewInProject>(
        "terminal.newInProject",
        "New Terminal in Project…",
        Terminal,
        &[],
    ),
    // Every agent in a terminal, needing the user first (AgentsMenu).
    cmd::<terminal::ShowAgents>("terminal.showAgents", "Show Agents", Terminal, &[]),
    // An agent's past conversations in the open worktree, resumed in a new terminal (the palette lists them).
    cmd::<terminal::ResumeConversation>(
        "terminal.resumeConversation",
        "Resume a Conversation…",
        Terminal,
        &[],
    ),
    // Off macOS Ctrl+Shift, as in Linux terminals: Ctrl+letter is the shell's and desktops take Super
    // chords (KDE: Super+D, Super+W). The ones below it only run in the terminal.
    cmd::<terminal::New>("terminal.new", "New Terminal", Terminal, &["cmd+t"])
        .other(&["shift+cmd+t"]),
    // The id is from when it only split right; kept so a user's binding for it still applies.
    cmd::<terminal::Split>(
        "terminal.split",
        "Split Terminal Right",
        Terminal,
        &["cmd+d"],
    )
    .other(&["shift+cmd+d"])
    .local("Terminal")
    .no_repeat(),
    // ⇧⌘D. Elsewhere Ctrl+Shift+D is split right, so down adds Alt.
    cmd::<terminal::SplitDown>(
        "terminal.splitDown",
        "Split Terminal Down",
        Terminal,
        &["shift+cmd+d"],
    )
    .other(&["alt+shift+cmd+d"])
    .local("Terminal")
    .no_repeat(),
    cmd::<terminal::Clear>("terminal.clear", "Clear Terminal", Terminal, &["cmd+k"])
        .other(&["shift+cmd+k"])
        .local("Terminal"),
    cmd::<terminal::Close>(
        "terminal.close",
        "Close Terminal Pane",
        Terminal,
        &["cmd+w"],
    )
    .other(&["shift+cmd+w"])
    .local("Terminal"),
    // ⇧⌘←/→ page through the panes by their numbers, wrapping; ⌥⌘arrows go by side.
    // Elsewhere Ctrl+Alt+←/→, "alt+cmd" there: the old app's "ctrl+alt" was Super+Alt.
    cmd::<terminal::PrevPane>(
        "terminal.prevPane",
        "Previous Terminal Pane",
        Terminal,
        &["shift+cmd+left"],
    )
    .other(&["alt+cmd+left"])
    .local("Terminal"),
    cmd::<terminal::NextPane>(
        "terminal.nextPane",
        "Next Terminal Pane",
        Terminal,
        &["shift+cmd+right"],
    )
    .other(&["alt+cmd+right"])
    .local("Terminal"),
    // Unbound elsewhere: Ctrl+Shift+arrows select by word in shells (PSReadLine), GNOME takes Ctrl+Alt+Shift+arrows, and Alt+arrows move by word.
    cmd::<terminal::FocusLeft>(
        "terminal.focusLeft",
        "Focus Terminal Pane Left",
        Terminal,
        &["alt+cmd+left"],
    )
    .other(&[])
    .local("Terminal"),
    cmd::<terminal::FocusRight>(
        "terminal.focusRight",
        "Focus Terminal Pane Right",
        Terminal,
        &["alt+cmd+right"],
    )
    .other(&[])
    .local("Terminal"),
    cmd::<terminal::FocusUp>(
        "terminal.focusUp",
        "Focus Terminal Pane Above",
        Terminal,
        &["shift+cmd+up", "alt+cmd+up"],
    )
    .other(&[])
    .local("Terminal"),
    cmd::<terminal::FocusDown>(
        "terminal.focusDown",
        "Focus Terminal Pane Below",
        Terminal,
        &["shift+cmd+down", "alt+cmd+down"],
    )
    .other(&[])
    .local("Terminal"),
    // Esc stays the program's (Claude Code, vim). Elsewhere Ctrl+Enter is a program's (kitty keyboard protocol), and Ctrl+Shift+Enter zooms a pane.
    cmd::<terminal::ToggleMaximize>(
        "terminal.toggleMaximize",
        "Maximize Terminal",
        Terminal,
        &["cmd+enter"],
    )
    .other(&["alt+shift+cmd+enter"])
    .local("Terminal")
    .no_repeat(),
    // ⇧⌘↵ (Ctrl+Shift+Enter elsewhere).
    cmd::<terminal::ZoomPane>(
        "terminal.zoomPane",
        "Zoom Terminal Pane",
        Terminal,
        &["shift+cmd+enter"],
    )
    .other(&["shift+cmd+enter"])
    .local("Terminal")
    .no_repeat(),
    // Only a split's panes show a name; the tab shows the repo and branch. Unbound elsewhere: Ctrl+R is
    // the shell's history search, and Ctrl+Shift+R reloads.
    // Every split of the open tab at even sizes; one split's divider does it with a double-click.
    cmd::<terminal::EqualizePanes>(
        "terminal.equalizePanes",
        "Equalize Terminal Panes",
        Terminal,
        &[],
    ),
    cmd::<terminal::RenamePane>(
        "terminal.renamePane",
        "Rename Terminal Pane",
        Terminal,
        &["cmd+r"],
    )
    .other(&[])
    .local("Terminal"),
    // The window's zoom keys size the terminal's font while it has focus, as in terminal apps.
    cmd::<terminal::FontZoomIn>(
        "terminal.fontZoomIn",
        "Increase Terminal Font Size",
        Terminal,
        &["cmd+=", "shift+cmd+="],
    )
    .local("Terminal"),
    cmd::<terminal::FontZoomOut>(
        "terminal.fontZoomOut",
        "Decrease Terminal Font Size",
        Terminal,
        &["cmd+-"],
    )
    .local("Terminal"),
    cmd::<terminal::FontZoomReset>(
        "terminal.fontZoomReset",
        "Reset Terminal Font Size",
        Terminal,
        &["cmd+0"],
    )
    .local("Terminal"),
    // The prompts shell integration marks (Ctrl+↑/↓ elsewhere, "cmd" there); a full-screen program keeps the keys.
    cmd::<terminal::PrevCommand>(
        "terminal.prevCommand",
        "Scroll to Previous Command",
        Terminal,
        &["cmd+up"],
    )
    .local("Terminal"),
    cmd::<terminal::NextCommand>(
        "terminal.nextCommand",
        "Scroll to Next Command",
        Terminal,
        &["cmd+down"],
    )
    .local("Terminal"),
    // Ctrl+Shift+Space on every platform (Ctrl+Shift+E is the explorer's here).
    cmd::<terminal::Hints>(
        "terminal.hints",
        "Show Link Hints",
        Terminal,
        &["ctrl+shift+space"],
    )
    .other(&["shift+cmd+space"])
    .local("Terminal")
    .no_repeat(),
    // Tab can't reach the tabs from a pane: the shell takes it. On them ←/→ open one, ⌥←/⌥→ move it,
    // ⌫ or ⌘W closes it, ↵ or Esc goes back in. Unbound elsewhere: Super chords are the desktop's.
    cmd::<terminal::FocusTabs>(
        "terminal.focusTabs",
        "Focus Terminal Tabs",
        Terminal,
        &["ctrl+cmd+t"],
    )
    .other(&[])
    .local("Terminal"),
    // A browser tab (features/browser), new each time; in it ⌘L, ⌘R, ⌘[ and ⌘] are a browser's, as typed in its page too.
    cmd::<browser::Open>("browser.open", "Open Browser", Browser, &["shift+cmd+b"]),
    cmd::<browser::FocusAddress>(
        "browser.focusAddress",
        "Focus Address Bar",
        Browser,
        &["cmd+l"],
    )
    .local("Browser"),
    cmd::<browser::Reload>("browser.reload", "Reload Page", Browser, &["cmd+r"]).local("Browser"),
    cmd::<browser::Back>("browser.back", "Go Back", Browser, &["cmd+["]).local("Browser"),
    cmd::<browser::Forward>("browser.forward", "Go Forward", Browser, &["cmd+]"]).local("Browser"),
    // As Safari's; where WebKit can't open it from here, the page's menu has Inspect Element.
    cmd::<browser::Inspect>(
        "browser.inspect",
        "Show Web Inspector",
        Browser,
        &["alt+cmd+i"],
    )
    .local("Browser"),
    // As Chrome's inspect-element key: picks an element to tell an agent about.
    cmd::<browser::Pick>(
        "browser.pick",
        "Pick an Element for the Agent",
        Browser,
        &["shift+cmd+c"],
    )
    .local("Browser"),
    // As Chrome's device toolbar; the last device comes back.
    cmd::<browser::ToggleDevice>(
        "browser.toggleDevice",
        "Toggle Device Mode",
        Browser,
        &["shift+cmd+m"],
    )
    .local("Browser"),
    cmd::<browser::ClearData>("browser.clearData", "Clear Browsing Data…", Browser, &[]),
    // In the page as in Safari: ⌘F finds in it, and the window's zoom keys zoom it, not the app.
    cmd::<browser::Find>("browser.find", "Find in Page", Browser, &["cmd+f"]).local("Browser"),
    cmd::<browser::FindNext>("browser.findNext", "Find Next in Page", Browser, &["cmd+g"])
        .local("Browser"),
    cmd::<browser::FindPrev>(
        "browser.findPrev",
        "Find Previous in Page",
        Browser,
        &["shift+cmd+g"],
    )
    .local("Browser"),
    cmd::<browser::ZoomIn>(
        "browser.zoomIn",
        "Zoom In Page",
        Browser,
        &["cmd+=", "shift+cmd+="],
    )
    .local("Browser"),
    cmd::<browser::ZoomOut>("browser.zoomOut", "Zoom Out Page", Browser, &["cmd+-"])
        .local("Browser"),
    cmd::<browser::ZoomReset>("browser.zoomReset", "Actual Size", Browser, &["cmd+0"])
        .local("Browser"),
    cmd::<explorer::Rename>("explorer.rename", "Rename File", Explorer, &["f2"]).local("Explorer"),
    cmd::<explorer::Delete>(
        "explorer.delete",
        "Delete File",
        Explorer,
        &["cmd+backspace"],
    )
    .local("Explorer"),
    cmd::<git::SwitchBranch>("git.switchBranch", "Switch Branch", Git, &[]),
    cmd::<git::NewBranch>("git.newBranch", "New Branch", Git, &[]),
    // The Compare screen: any two branches, tags or commits against each other.
    cmd::<git::CompareBranches>("git.compareBranches", "Compare Branches…", Git, &[]),
    cmd::<git::SwitchWorktree>("git.switchWorktree", "Switch Worktree", Git, &[]),
    cmd::<git::NewWorktree>("git.newWorktree", "New Worktree", Git, &[]),
    // With ⌃ added, as ⇧⌘T, ⇧⌘P and ⌘P reopen a tab, open the palette and a file here.
    // Elsewhere "ctrl" is the Super key, which the OS takes: unbound there.
    cmd::<git::Fetch>("git.fetch", "Fetch", Git, &["ctrl+shift+cmd+t"]).other(&[]),
    cmd::<git::Pull>("git.pull", "Pull", Git, &["ctrl+shift+cmd+p"]).other(&[]),
    cmd::<git::Push>("git.push", "Push", Git, &["ctrl+cmd+p"]).other(&[]),
    // What the remote has, then what's here.
    cmd::<git::Sync>("git.sync", "Sync (Pull, then Push)", Git, &[]),
    // A patch from Copy as Patch, a mail or a chat: previewed first, then applied to the working tree.
    cmd::<git::ApplyPatch>("git.applyPatch", "Apply Patch from Clipboard", Git, &[]),
    // Not while typing: text fields keep ⌘Z for their own undo.
    cmd::<git::Undo>("git.undo", "Undo Git Action", Git, &["cmd+z"]).outside_text(),
    cmd::<git::Redo>("git.redo", "Redo Git Action", Git, &["shift+cmd+z"]).outside_text(),
    // These act on the file open in the viewer. Discard asks first; ⌘⌫ stays line delete in text.
    // A held S would stage file after file as the open one moves on.
    cmd::<git::ToggleStage>("git.toggleStage", "Stage / Unstage Changes", Git, &["s"]).no_repeat(),
    cmd::<git::Discard>("git.discard", "Discard Changes", Git, &["cmd+backspace"]).outside_text(),
    // Local: only `local` listens for it.
    cmd::<git::Commit>("git.commit", "Commit", Git, &["cmd+enter", "ctrl+enter"])
        .local("CommitMessage")
        .over_text(),
    cmd::<git::RenameBranch>("git.renameBranch", "Rename Branch", Git, &["f2"])
        .local("BranchPicker"),
    cmd::<git::SelectAllChanges>(
        "git.selectAllChanges",
        "Select All Changes",
        Git,
        &["cmd+a"],
    )
    .local("ChangesList"),
    cmd::<git::CreateTag>(
        "git.createTag",
        "Create Tag",
        Git,
        &["cmd+enter", "ctrl+enter"],
    )
    .local("TagMessage")
    .over_text(),
    // Where "/" takes ⇧ (Turkish, German ⇧7).
    cmd::<history::Find>("history.find", "Find in History", Git, &["/", "shift+/"])
        .local("HistoryList"),
    cmd::<worktree::OpenTerminal>(
        "worktree.openTerminal",
        "Open Terminal in Worktree",
        Git,
        &["t"],
    )
    .local("WorktreePicker"),
    cmd::<worktree::Merge>("worktree.merge", "Merge Worktree Branch", Git, &["m"])
        .local("WorktreePicker"),
    cmd::<worktree::Rename>("worktree.rename", "Rename Worktree", Git, &["f2"])
        .local("WorktreePicker"),
    cmd::<worktree::Remove>(
        "worktree.remove",
        "Remove Worktree",
        Git,
        &["backspace", "delete"],
    )
    .local("WorktreePicker"),
    cmd::<project::Forget>(
        "project.forget",
        "Remove from Projects",
        General,
        &["backspace", "delete"],
    )
    .local("ProjectList"),
    cmd::<github::PostComment>(
        "github.postComment",
        "Post Comment",
        GitHub,
        &["cmd+enter", "ctrl+enter"],
    )
    .local("IssueComment")
    .over_text(),
    // Only while suggestions are set up in Settings.
    cmd::<git::SuggestMessage>("git.suggestMessage", "Suggest Commit Message", Git, &[]),
];

/// Menu bar items that aren't key commands; the palette lists them too.
pub static MENU_COMMANDS: &[Command] = &[
    cmd::<app::About>("app.about", "About GitViber", Help, &[]),
    cmd::<app::CheckForUpdates>("app.checkForUpdates", "Check for Updates", Help, &[]),
    cmd::<app::InstallCli>(
        "app.installCli",
        "Install 'gitviber' Command in PATH",
        Help,
        &[],
    ),
    cmd::<help::Readme>("help.readme", "GitViber Help", Help, &[]),
    cmd::<help::Shortcuts>("help.shortcuts", "Keyboard Shortcuts", Help, &[]),
    cmd::<help::ReportBug>("help.reportBug", "Report a Bug", Help, &[]),
    cmd::<help::CopyDiagnostics>("help.copyDiagnostics", "Copy Diagnostics", Help, &[]),
    cmd::<help::ShowLogs>("help.showLogs", "Show Logs", Help, &[]),
    cmd::<help::ReleaseNotes>("help.releaseNotes", "Release Notes", Help, &[]),
    cmd::<help::License>("help.license", "View License", Help, &[]),
];

/// The system's own commands: fixed keys, kept out of the palette and of rebinding (macOS takes
/// these chords before the app sees them). Only Quit keeps its keys elsewhere: there Ctrl+H and
/// Ctrl+M would run while typing, and Super+Ctrl+F is the desktop's.
pub static SYSTEM_COMMANDS: &[Command] = &[
    cmd::<app::Quit>("app.quit", "Quit GitViber", General, &["cmd+q"]),
    cmd::<app::Hide>("app.hide", "Hide GitViber", General, &["cmd+h"]).other(&[]),
    cmd::<app::HideOthers>("app.hideOthers", "Hide Others", General, &["alt+cmd+h"]).other(&[]),
    cmd::<app::ShowAll>("app.showAll", "Show All", General, &[]),
    cmd::<window::Minimize>("window.minimize", "Minimize", General, &["cmd+m"]).other(&[]),
    cmd::<window::Zoom>("window.zoom", "Zoom", General, &[]),
    cmd::<window::ToggleFullScreen>(
        "window.toggleFullScreen",
        "Enter Full Screen",
        General,
        &["ctrl+cmd+f"],
    )
    .other(&[]),
    cmd::<window::BringAllToFront>("window.bringAllToFront", "Bring All to Front", General, &[]),
];

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::keymap::chord::{is_reserved, runs_in_terminal, runs_while_typing};
    use crate::keymap::{Overrides, bindings_for, command};

    #[test]
    fn no_two_global_commands_share_a_default_and_macos_takes_none() {
        for mac in [true, false] {
            let mut owner: HashMap<String, &str> = HashMap::new();
            for c in COMMANDS.iter().filter(|c| c.local.is_none()) {
                for chord in bindings_for(c, &Overrides::new(), mac) {
                    assert!(!is_reserved(&chord, mac), "{chord} ({}, mac {mac})", c.id);
                    if let Some(other) = owner.insert(chord.clone(), c.id) {
                        panic!("{chord} is {other}'s and {}'s (mac {mac})", c.id);
                    }
                }
            }
        }
    }

    #[test]
    fn no_two_commands_of_one_place_share_a_default() {
        for mac in [true, false] {
            let mut owner: HashMap<(&str, String), &str> = HashMap::new();
            for c in COMMANDS {
                let Some(place) = c.local else { continue };
                for chord in bindings_for(c, &Overrides::new(), mac) {
                    if let Some(other) = owner.insert((place, chord.clone()), c.id) {
                        panic!("{chord} is {other}'s and {}'s in {place} (mac {mac})", c.id);
                    }
                }
            }
        }
    }

    /// A key meant for the code view mustn't stage or discard from the commit message or the
    /// terminal.
    #[test]
    fn stage_unstage_and_discard_stay_out_of_text_fields_and_the_terminal() {
        for id in [
            "diff.stageChange",
            "diff.unstageChange",
            "diff.discardChange",
        ] {
            let c = command(id).unwrap();
            for mac in [true, false] {
                for chord in bindings_for(c, &Overrides::new(), mac) {
                    assert!(!runs_while_typing(&chord, c, mac), "{id}: {chord}");
                    assert!(!runs_in_terminal(&chord, c, mac), "{id}: {chord}");
                }
            }
        }
        // ⌥ turns S into ß and N into a dead key; the key's letter still finds the command.
        let winner =
            |chord| crate::keymap::edit::winner(chord, &Overrides::new(), true).map(|c| c.id);
        assert_eq!(winner("alt+cmd+s"), Some("diff.stageChange"));
        assert_eq!(winner("alt+cmd+n"), Some("diff.unstageChange"));
    }

    #[test]
    fn fetch_pull_push_and_focus_commit_reach_the_app_from_fields_and_the_terminal_on_macos() {
        for (id, chord) in [
            ("git.fetch", "ctrl+shift+cmd+t"),
            ("git.pull", "ctrl+shift+cmd+p"),
            ("git.push", "ctrl+cmd+p"),
            ("changes.focusMessage", "ctrl+cmd+c"),
        ] {
            let c = command(id).unwrap();
            assert_eq!(bindings_for(c, &Overrides::new(), true), [chord], "{id}");
            assert!(bindings_for(c, &Overrides::new(), false).is_empty(), "{id}");
            assert!(runs_while_typing(chord, c, true), "{id}");
            assert!(runs_in_terminal(chord, c, true), "{id}");
        }
        // ⌘G stays macOS's Find Next.
        assert!(crate::keymap::edit::winner("cmd+g", &Overrides::new(), true).is_none());
    }
}
