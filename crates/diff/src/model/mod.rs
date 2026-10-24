//! The diff as data, with no GPUI in it: both sides' text, core's rows, which of them show, and
//! which lines a staging action takes. The view only draws what this says.

pub mod display;
pub mod emphasis;
pub mod find;
pub mod indent;
pub mod notes;
pub mod staging;
pub mod svg;
pub mod text;

pub use display::{Item, Shown};
pub use staging::{Change, Picked};
pub use text::SideText;

/// core's row: `k` 0 unchanged, 1 added, 2 removed; `o` / `n` 1-based lines (0: not on that
/// side); `e` word changes in UTF-16 units of the line.
pub type Row = gitviber_core::diff::Row;

/// Which side of a diff a line is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    /// The original: the index, HEAD, a commit's parent, a range's base.
    Old,
    /// The modified side.
    New,
}

/// A line of one side, 1-based.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LinePos {
    pub side: Side,
    pub line: u32,
}

/// Lines `start..=end` (1-based) of one side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LineRange {
    pub side: Side,
    pub start: u32,
    pub end: u32,
}

impl LineRange {
    pub fn single(pos: LinePos) -> Self {
        LineRange {
            side: pos.side,
            start: pos.line,
            end: pos.line,
        }
    }
}

pub const ADDED: u8 = 1;
pub const REMOVED: u8 = 2;

/// The row's line on the side it shows: its old line when removed, else its new one.
pub fn row_pos(r: &Row) -> LinePos {
    if r.k == REMOVED {
        LinePos {
            side: Side::Old,
            line: r.o,
        }
    } else {
        LinePos {
            side: Side::New,
            line: r.n,
        }
    }
}

/// Lines added and removed.
pub fn counts(rows: &[Row]) -> (u32, u32) {
    rows.iter().fold((0, 0), |(a, d), r| match r.k {
        ADDED => (a + 1, d),
        REMOVED => (a, d + 1),
        _ => (a, d),
    })
}
