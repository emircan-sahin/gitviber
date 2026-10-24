//! GitViber's markdown: GitHub-flavored markdown rendered natively, for files, pull requests
//! and notes. [`MarkdownView`] shows a document; [`MarkdownBase`] says where it is, so relative
//! links and images resolve against its folder. Links to files come back as
//! [`MarkdownEvent::OpenFile`]; anchors, web links and the rest are handled here.
//!
//! Mermaid diagrams and math have no renderer yet: they show as their source in a labeled block
//! until one is set with [`set_diagram_renderer`].

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

pub mod blocks;
pub mod images;
pub mod links;
mod view;

pub use blocks::{Callout, Callouts, DiagramKind, DiagramRenderer, set_diagram_renderer};
pub use images::BoundedImageCache;
pub use links::Link;
pub use view::{MarkdownBase, MarkdownEvent, MarkdownView, is_markdown};

#[cfg(test)]
mod leak_tests;
