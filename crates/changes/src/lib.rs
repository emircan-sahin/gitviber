//! GitViber's Changes panel: the uncommitted files with their staging and review marks, the
//! commit box, stashes and submodules, a merge or rebase in progress, and the center tabs of a
//! file's unstaged, staged, conflicted or branch-review diff and of a whole list.
//! `init` registers the panel and the tab openers.

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

mod commit;
mod conflict;
mod dialogs;
pub mod model;
mod panel;
mod store;
mod sync;
mod tabs;

pub use commit::CommitBox;
pub use conflict::ConflictView;
pub use model::list::ChangesView;
pub use panel::ChangesPanel;
pub use tabs::ChangeTab;

use gitviber_ui::actions::changes as changes_actions;
use gitviber_ui::workspace::{PanelSpec, Slot, Workspace, register_panel};
use gpui_kit::{App, AppContext as _};
use std::sync::Arc;

/// The app's core. Tests give it one of their own: the app's sends its events across threads.
pub(crate) fn core(cx: &App) -> Arc<gitviber_core::Core> {
    #[cfg(test)]
    if let Some(c) = cx.try_global::<TestCore>() {
        return c.0.clone();
    }
    gitviber_ui::core(cx)
}

#[cfg(test)]
pub(crate) struct TestCore(pub Arc<gitviber_core::Core>);

#[cfg(test)]
impl gpui_kit::Global for TestCore {}

/// The panel, its keys, the openers of its tabs, and the diffs kept in step with the theme and
/// the settings. Once at startup, after gitviber-ui's init and core; on the first launch it
/// takes over what the old app kept.
pub fn init(cx: &mut App) {
    store::import_legacy(cx);
    gitviber_diff::init_local(cx);
    panel::bind_keys(cx);
    sync::install(cx);
    tabs::register(cx);
    register_panel(
        cx,
        PanelSpec::new("changes", Slot::Left, "Changes", |ctx, window, cx| {
            cx.new(|cx| ChangesPanel::new(ctx, window, cx))
        })
        .order(0),
    );
    // ⌃⌘C from anywhere, as in the old app: the panel shows, built if it wasn't yet, and its
    // commit message takes the focus. With focus in the panel, the panel's own handler runs.
    cx.on_action(|_: &changes_actions::FocusMessage, cx| {
        let Some(handle) = cx.active_window() else {
            return;
        };
        // The window is in the middle of dispatching the action: it can be updated after.
        cx.defer(move |cx| {
            handle
                .update(cx, |_, window, cx| {
                    let Some(ws) = Workspace::for_window(window, cx) else {
                        return;
                    };
                    ws.update(cx, |ws, cx| ws.show_panel("changes", cx));
                    let panel = Workspace::ensure_panel(&ws, "changes", window, cx)
                        .and_then(|p| p.view().downcast::<ChangesPanel>().ok());
                    if let Some(panel) = panel {
                        panel.update(cx, |p, cx| p.focus_message(window, cx));
                    }
                })
                .ok();
        });
    });
}
