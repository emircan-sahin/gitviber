//! The menu bar: the same commands as the keyboard and the palette. An item is enabled while
//! something in focus handles its action, shows the user's binding, and is rebuilt when the
//! bindings, the toggles it checks or the recent projects change.

use gitviber_ui::actions::{
    self as cmds, app, browser, changes, diff, editor, file, git, help, history, repo, review,
    search, tab, terminal, view, window, workbench,
};
use gitviber_ui::{Settings, UiState};
use gpui_kit::component::input as text;
use gpui_kit::{Action, App, Menu, MenuItem, OsAction, SharedString, SystemMenuType};

/// File → Open Recent's items.
#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = file, no_json)]
pub struct OpenRecent {
    pub path: SharedString,
}

gpui_kit::actions!(file, [ClearRecent]);

const MAC: bool = cfg!(target_os = "macos");

/// An item named by its command's title, or `name` when the menu says it differently.
fn item(action: impl Action, name: &str) -> MenuItem {
    MenuItem::action(SharedString::from(name.to_string()), action)
}

fn check(action: impl Action, name: &str, checked: bool) -> MenuItem {
    item(action, name).checked(checked)
}

fn sep() -> MenuItem {
    MenuItem::separator()
}

/// Two folders of the same name show their paths instead.
fn recent_menu(recent: &[String]) -> Menu {
    let name = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
    let names: Vec<String> = recent.iter().map(|p| name(p)).collect();
    let mut items: Vec<MenuItem> = recent
        .iter()
        .zip(&names)
        .map(|(path, n)| {
            let shown = if names.iter().filter(|m| *m == n).count() > 1 {
                path.clone()
            } else {
                n.clone()
            };
            MenuItem::action(
                shown,
                OpenRecent {
                    path: path.clone().into(),
                },
            )
        })
        .collect();
    if !items.is_empty() {
        items.push(sep());
    }
    items.push(item(ClearRecent, "Clear Recent"));
    Menu::new("Open Recent").items(items)
}

fn reveal_label() -> &'static str {
    cmds::command("file.reveal")
        .map(|c| c.title)
        .unwrap_or("Reveal")
}

pub fn build(cx: &App) -> Vec<Menu> {
    let s = Settings::get(cx);
    let recent = &UiState::get(cx).recent_projects;
    let show_logs = if MAC {
        "Show Logs in Finder"
    } else {
        "Show Logs"
    };
    let app_menu = Menu::new("GitViber").items([
        item(app::About, "About GitViber"),
        item(app::CheckForUpdates, "Check for Updates…"),
        sep(),
        item(workbench::OpenSettings, "Settings…"),
        item(app::InstallCli, "Install 'gitviber' Command…"),
        sep(),
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        sep(),
        item(app::Hide, "Hide GitViber"),
        item(app::HideOthers, "Hide Others"),
        item(app::ShowAll, "Show All"),
        sep(),
        item(app::Quit, "Quit GitViber"),
    ]);
    let mut file_items = vec![
        item(file::OpenRepo, "Open Repository…"),
        item(file::CloneRepo, "Clone Repository…"),
        MenuItem::submenu(recent_menu(recent)),
        sep(),
        item(workbench::QuickOpen, "Open File…"),
        item(workbench::OpenChange, "Open Changed File…"),
        sep(),
        item(terminal::New, "New Terminal"),
        sep(),
        item(file::Save, "Save"),
        item(tab::Close, "Close Tab"),
        item(tab::ReopenClosed, "Reopen Closed Tab"),
        sep(),
        item(file::Reveal, reveal_label()),
        item(file::OpenIn, "Open in…"),
    ];
    // The app menu and Window are macOS's; elsewhere Settings goes under File, About under Help.
    if !MAC {
        file_items.extend([sep(), item(workbench::OpenSettings, "Settings…")]);
        if cfg!(target_os = "linux") {
            file_items.push(item(app::InstallCli, "Install 'gitviber' Command…"));
        }
        file_items.extend([sep(), item(app::Quit, "Quit")]);
    }
    let edit = Menu::new("Edit").items([
        MenuItem::os_action("Undo", text::Undo, OsAction::Undo),
        MenuItem::os_action("Redo", text::Redo, OsAction::Redo),
        sep(),
        MenuItem::os_action("Cut", text::Cut, OsAction::Cut),
        MenuItem::os_action("Copy", text::Copy, OsAction::Copy),
        MenuItem::os_action("Paste", text::Paste, OsAction::Paste),
        MenuItem::os_action("Select All", text::SelectAll, OsAction::SelectAll),
        sep(),
        item(editor::Find, "Find"),
        item(search::FindInFiles, "Find in Files"),
    ]);
    let view_menu = Menu::new("View").items([
        item(workbench::ShowCommands, "Command Palette…"),
        sep(),
        item(view::Changes, "Changes"),
        item(view::History, "History"),
        item(view::Pulls, "Pull Requests"),
        item(view::Issues, "Issues"),
        item(review::Branch, "Review Branch Against Base"),
        item(history::Search, "Search History"),
        sep(),
        item(view::ToggleGitPanel, "Toggle Git Panel"),
        item(view::ToggleExplorer, "Toggle Explorer"),
        item(terminal::Toggle, "Toggle Terminal"),
        item(browser::Open, "Open Browser"),
        sep(),
        item(view::FocusGitPanel, "Focus Git Panel"),
        item(view::FocusCode, "Focus Code View"),
        item(view::ShowExplorer, "Focus Explorer"),
        item(changes::FocusMessage, "Focus Commit Message"),
        item(view::FocusNextPanel, "Focus Next Panel"),
        item(view::FocusPrevPanel, "Focus Previous Panel"),
        sep(),
        check(diff::ToggleSplit, "Split Diff", s.side_by_side),
        check(
            diff::ToggleCollapse,
            "Collapse Unchanged Lines",
            s.hide_unchanged,
        ),
        check(
            diff::ToggleWhitespace,
            "Ignore Whitespace",
            s.ignore_whitespace,
        ),
        check(editor::ToggleWrap, "Word Wrap", s.word_wrap),
        check(editor::ToggleBlame, "Blame", s.blame),
        sep(),
        item(view::ZoomIn, "Zoom In"),
        item(view::ZoomOut, "Zoom Out"),
        item(view::ZoomReset, "Actual Size"),
        item(editor::FontZoomIn, "Increase Code Font Size"),
        item(editor::FontZoomOut, "Decrease Code Font Size"),
        item(editor::FontZoomReset, "Reset Code Font Size"),
        sep(),
        item(repo::Refresh, "Refresh"),
        item(window::Reload, "Reload Window"),
        sep(),
        item(window::ToggleFullScreen, "Enter Full Screen"),
    ]);
    let go = Menu::new("Go").items([
        item(review::NextFile, "Next Changed File"),
        item(review::PrevFile, "Previous Changed File"),
        item(review::ToggleViewed, "Toggle File Viewed"),
        sep(),
        item(diff::NextChange, "Next Change"),
        item(diff::PrevChange, "Previous Change"),
        sep(),
        item(tab::Next, "Next Tab"),
        item(tab::Prev, "Previous Tab"),
        item(tab::Last, "Last Tab"),
    ]);
    let git_menu = Menu::new("Git").items([
        item(git::Commit, "Commit"),
        item(git::SuggestMessage, "Suggest Commit Message"),
        item(git::ToggleStage, "Stage / Unstage Changes"),
        item(git::Discard, "Discard Changes"),
        sep(),
        item(diff::StageChange, "Stage Change or Selected Lines"),
        item(diff::UnstageChange, "Unstage Change or Selected Lines"),
        item(diff::DiscardChange, "Discard Change or Selected Lines"),
        sep(),
        item(git::Fetch, "Fetch"),
        item(git::Pull, "Pull"),
        item(git::Push, "Push"),
        sep(),
        item(git::Undo, "Undo Git Action"),
        item(git::Redo, "Redo Git Action"),
    ]);
    let window_menu = Menu::new("Window").items([
        item(window::Minimize, "Minimize"),
        item(window::Zoom, "Zoom"),
        sep(),
        item(window::BringAllToFront, "Bring All to Front"),
    ]);
    let mut help_items = vec![
        item(help::Readme, "GitViber Help"),
        item(help::Shortcuts, "Keyboard Shortcuts"),
        item(workbench::ShortcutOverlay, "Show Shortcut Overlay"),
        sep(),
        item(help::ReportBug, "Report a Bug…"),
        item(help::CopyDiagnostics, "Copy Diagnostics"),
        item(help::ShowLogs, show_logs),
        item(help::ReleaseNotes, "Release Notes"),
        item(help::License, "View License"),
    ];
    if !MAC {
        help_items.extend([
            sep(),
            item(app::CheckForUpdates, "Check for Updates…"),
            item(app::About, "About GitViber"),
        ]);
    }
    let file_menu = Menu::new("File").items(file_items);
    let help_menu = Menu::new("Help").items(help_items);
    if MAC {
        vec![
            app_menu,
            file_menu,
            edit,
            view_menu,
            go,
            git_menu,
            window_menu,
            help_menu,
        ]
    } else {
        vec![file_menu, edit, view_menu, go, git_menu, help_menu]
    }
}

/// What the menu shows that can change: rebuilt only when it does, as each rebuild goes
/// through AppKit item by item.
fn inputs(cx: &App) -> (Vec<bool>, gitviber_ui::keymap::Overrides, Vec<String>) {
    let s = Settings::get(cx);
    (
        vec![
            s.side_by_side,
            s.hide_unchanged,
            s.ignore_whitespace,
            s.word_wrap,
            s.blame,
        ],
        s.keybindings.clone(),
        UiState::get(cx).recent_projects.clone(),
    )
}

pub fn install(cx: &mut App) {
    cx.set_menus(build(cx));
    let mut last = inputs(cx);
    let mut rebuild = move |cx: &mut App| {
        let now = inputs(cx);
        if now != last {
            last = now;
            cx.set_menus(build(cx));
        }
    };
    let mut again = rebuild.clone();
    cx.observe_global::<Settings>(move |cx| rebuild(cx))
        .detach();
    cx.observe_global::<UiState>(move |cx| again(cx)).detach();
}
