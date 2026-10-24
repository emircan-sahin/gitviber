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
mod sync;
mod tabs;

pub use commit::CommitBox;
pub use conflict::ConflictView;
pub use model::list::ChangesView;
pub use panel::ChangesPanel;
pub use tabs::ChangeTab;

use gitviber_ui::workspace::{PanelSpec, Slot, register_panel};
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
/// the settings. Once at startup, after gitviber-ui's init and core.
pub fn init(cx: &mut App) {
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
}
