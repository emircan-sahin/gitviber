//! GitViber's markdown: GitHub-flavored markdown rendered natively, for files, pull requests
//! and notes. [`MarkdownView`] shows a document; [`MarkdownBase`] says where it is, so relative
//! links and images resolve against its folder ([`MarkdownView::read_images`]: images from a
//! revision instead of the disk). Links to files come back as
//! [`MarkdownEvent::OpenFile`]; anchors, web links and the rest are handled here.
//!
//! Mermaid diagrams and `$…$` / `$$…$$` math are drawn natively (diagrams.rs); a diagram or a
//! formula that doesn't parse shows as its source under a label.
//!
//! Find: [`MarkdownView::find`] marks a [`FindQuery`]'s matches in the rendered text,
//! [`MarkdownView::select_match`] / [`MarkdownView::step_match`] mark the current one and reveal
//! it; [`FindBar`] is the box a host puts over its page.

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

pub mod blocks;
pub mod diagrams;
pub mod find;
pub mod images;
pub mod links;
mod view;

pub use blocks::{Callout, Callouts, DiagramKind};
pub use find::{FindBar, FindBarEvent, FindQuery};
pub use images::BoundedImageCache;
pub use links::Link;
pub use view::{FindChanged, MarkdownBase, MarkdownEvent, MarkdownView, ReadImage, is_markdown};

#[cfg(test)]
mod leak_tests;
