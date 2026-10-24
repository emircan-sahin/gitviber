//! The panel as data, with no GPUI in it.

pub mod commit;
pub mod list;
pub mod text;
pub mod tree;

pub use list::{Change, ChangesView, Item, Kind, Lists};
