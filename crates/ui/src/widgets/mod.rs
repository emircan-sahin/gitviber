//! Small pieces the feature crates share.

mod badges;
pub mod file_icon;
mod key_hint;
mod relative_time;

pub use badges::{
    ChangeKind, CountBadge, LineCounts, Look, PathLabel, StatusDot, StatusLetter, StatusPill,
};
pub use file_icon::{FileIcon, file_icon_path, folder_icon_path};
pub use key_hint::KeyHint;
pub use relative_time::{RelativeTime, full_date, now_unix, relative_time, repaint_next_minute};
