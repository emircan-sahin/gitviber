//! The app's integration tests, as one binary: each test binary links all of gpui.

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

// gpui-component's text fields bind their keys per platform; these are macOS's.
#[cfg(target_os = "macos")]
mod keys;
