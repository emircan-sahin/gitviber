//! Small pieces the feature crates share.

mod badges;
pub mod file_icon;
mod keycaps;
pub(crate) mod list_keys;
mod pressable;
mod project_tile;
mod relative_time;
mod toggle;

pub use badges::{
    ChangeKind, CountBadge, LineCounts, Look, PathLabel, StatusDot, StatusLetter, StatusPill,
};
pub use file_icon::{FileIcon, file_icon_path, folder_icon_path};
pub use keycaps::{Keycaps, chord_kbd, command_kbd, find_option_kbd, key_hint, key_hints};
pub use list_keys::{Ends, PAGE, list_step, on_list_keys};
pub use pressable::Pressable;
pub use project_tile::{project_tile, swatch_tile};
pub use relative_time::{RelativeTime, full_date, now_unix, relative_time, repaint_next_minute};
pub use toggle::{Toggle, segmented};

pub(crate) fn init(cx: &mut gpui_kit::App) {
    keycaps::init(cx);
}
