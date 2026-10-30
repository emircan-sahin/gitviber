//! GitViber's shared UI layer: theme, settings, the command registry and keymap, shared
//! widgets, and the workspace contract feature crates plug into. See `workspace` for the
//! contract, `dev::run` for a feature's example binary.

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

pub mod assets;
mod backend;
pub mod colors;
pub mod dev;
pub mod fonts;
pub mod fuzzy;
pub mod keymap;
pub mod legacy;
pub mod memory;
mod notify;
pub mod open_in;
pub mod projects;
pub mod settings;
pub mod state;
pub mod store;
pub mod test_support;
pub mod theme;
pub mod widgets;
pub mod windows;
pub mod workspace;

pub use backend::{CoreEvents, core, has_core, install_core, start_core};
pub use keymap as actions;
pub use notify::notify_if_away;
pub use settings::Settings;
pub use state::UiState;
pub use theme::{ActiveAppTheme, AppTheme};
pub use workspace::{Project, Workspace};

use gpui_kit::App;

/// Everything a GitViber window needs before it opens: gpui-kit's components, the settings
/// and state files, the theme, the keymap and the workspace registry. Call once at startup,
/// on an application made `with_assets(gitviber_ui::assets::Assets)`.
pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    settings::init(cx);
    state::init(cx);
    theme::init(cx);
    keymap::init(cx);
    workspace::init(cx);
    widgets::init(cx);
}
