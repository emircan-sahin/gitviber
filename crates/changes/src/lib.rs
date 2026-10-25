//! GitViber's Changes panel: the uncommitted files with their staging and review marks, the
//! commit box, stashes and submodules, a merge or rebase in progress, and the center tabs of a
//! file's unstaged, staged, conflicted or branch-review diff and of a whole list.
//! `init` registers the panel and the tab openers.

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

mod co_authors;
mod commit;
mod conflict;
mod dialogs;
mod global;
pub mod model;
mod panel;
mod status_item;
mod store;
mod sync;
mod tabs;
mod toasts;

pub use commit::CommitBox;
pub use conflict::ConflictView;
pub use model::list::ChangesView;
pub use panel::ChangesPanel;
pub use tabs::ChangeTab;

pub(crate) use gitviber_ui::core;
use gitviber_ui::workspace::{PanelSpec, Slot, register_panel};
use gpui_kit::{App, AppContext as _};

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
    global::install(cx);
    status_item::register(cx);
}
