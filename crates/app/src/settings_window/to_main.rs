//! The menu bar's items for the main window, picked with Settings in front: File, the panels and
//! toggles, Go and Git. Nothing here has them, so they were greyed out (or ran in Settings, where
//! they found no workspace); now the main window comes forward and runs them. A key pressed in
//! Settings stays Settings' own: J there never moves to the next file behind it.

use gitviber_ui::actions::{
    browser, changes, diff, editor, file, git, history, repo, review, search, tab, terminal, view,
    workbench,
};
use gpui_kit::{Action, App, Div, Global, InteractiveElement as _, Stateful, Window};

use crate::menus::{ClearRecent, OpenRecent};

/// A key is being dispatched in the settings window, until the end of that update: what it
/// brings is the key's, not the menu bar's.
#[derive(Default)]
struct Keying(bool);
impl Global for Keying {}

pub(super) fn install(cx: &mut App) {
    cx.intercept_keystrokes(|_, window, cx| {
        if super::is_open_in(window, cx) {
            cx.set_global(Keying(true));
            cx.defer(|cx| cx.set_global(Keying(false)));
        }
    })
    .detach();
}

/// `action` in the main window, which comes forward; a key's goes on as before (to nothing).
fn run_in_main(action: &dyn Action, cx: &mut App) {
    if cx.try_global::<Keying>().is_some_and(|k| k.0) {
        cx.propagate();
        return;
    }
    let Some(main) = crate::workspace_view::main_window(cx) else {
        return;
    };
    let action = action.boxed_clone();
    let _ = main.update(cx, |_, window, cx| {
        window.activate_window();
        window.dispatch_action(action, cx);
    });
}

macro_rules! send_on {
    ($root:expr, $($action:ty),* $(,)?) => {
        $root$(.on_action(|a: &$action, _: &mut Window, cx: &mut App| run_in_main(a, cx)))*
    };
}

/// The settings window's root, sending the main window's menu items on.
pub(super) fn send_on(root: Stateful<Div>) -> Stateful<Div> {
    send_on!(
        root,
        // File
        file::OpenRepo,
        OpenRecent,
        ClearRecent,
        workbench::QuickOpen,
        workbench::OpenChange,
        terminal::New,
        file::Save,
        tab::ReopenClosed,
        file::Reveal,
        file::OpenIn,
        search::FindInFiles,
        // View
        workbench::ShowCommands,
        view::Changes,
        view::History,
        view::Pulls,
        view::Issues,
        review::Branch,
        history::Search,
        view::ToggleGitPanel,
        view::ToggleExplorer,
        terminal::Toggle,
        browser::Open,
        view::FocusGitPanel,
        view::FocusCode,
        view::ShowExplorer,
        changes::FocusMessage,
        view::FocusNextPanel,
        view::FocusPrevPanel,
        diff::ToggleSplit,
        diff::ToggleCollapse,
        diff::ToggleWhitespace,
        editor::ToggleWrap,
        editor::ToggleBlame,
        editor::FontZoomIn,
        editor::FontZoomOut,
        editor::FontZoomReset,
        repo::Refresh,
        // Go
        review::NextFile,
        review::PrevFile,
        review::ToggleViewed,
        diff::NextChange,
        diff::PrevChange,
        tab::Next,
        tab::Prev,
        tab::Last,
        // Git
        git::Commit,
        git::SuggestMessage,
        git::ToggleStage,
        git::Discard,
        diff::StageChange,
        diff::UnstageChange,
        diff::DiscardChange,
        git::Fetch,
        git::Pull,
        git::Push,
        git::Undo,
        git::Redo,
    )
}
