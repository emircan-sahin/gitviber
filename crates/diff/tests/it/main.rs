//! The diff's integration tests, as one binary: each test binary links all of gpui, so five
//! of them cost five links and five copies in target/.

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

mod annotations;
mod leaks;
mod review_fixes;
mod stacked;
mod stage;
