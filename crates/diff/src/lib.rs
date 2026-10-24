//! GitViber's diff viewer, shared by Changes, History, pull requests, Compare and guided review.
//!
//! A [`DiffView`] shows one file's diff from a [`DiffSource`] (what to compare, plain data);
//! it reads both sides through core off the main thread, lays out only the lines on screen and
//! parses each side once for syntax colors. How every diff looks and what it shows are the
//! [`DiffStyle`] and [`DiffOptions`] globals, which the app sets from its theme and settings.
//! Call [`init`] once for the keys.

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

pub mod actions;
pub mod data;
mod edit;
mod highlight;
mod image;
mod list;
pub mod model;
mod paint;
pub mod source;
mod stacked;
pub mod style;
mod view;

pub use data::DiffData;
pub use image::{ImageCompare, image_format};
pub use model::{LinePos, LineRange, Side};
pub use paint::status_pill;
pub use source::{Compare, DiffSource, FileFacts};
pub use stacked::{StackedDiff, StackedEvent, StackedFile, is_media};
pub use style::{DiffColors, DiffOptions, DiffStyle, ImageMode, Layout, Whitespace};
pub use view::{DiffEvent, DiffView, LineAction, Notice, NoticeLevel};

/// The app's one Core. Until gitviber-ui hands it out, the app (or a demo) sets it once with
/// `set_core`; this is the only place the diff gets it from.
pub fn core(cx: &gpui_kit::App) -> std::sync::Arc<gitviber_core::Core> {
    cx.try_global::<AppCore>()
        .map(|c| c.0.clone())
        .expect("gitviber_diff::set_core first")
}

pub fn set_core(core: std::sync::Arc<gitviber_core::Core>, cx: &mut gpui_kit::App) {
    cx.set_global(AppCore(core));
}

struct AppCore(std::sync::Arc<gitviber_core::Core>);

impl gpui_kit::Global for AppCore {}

/// Binds the diff's keys (see `actions`).
pub fn init(cx: &mut gpui_kit::App) {
    actions::init(cx);
}
