//! What the overview beside a diff's lines marks: where the changes and find's matches are, each
//! side on its half. Worked out from the rows and what folds, never from what's laid out, so it's
//! right before anything scrolls.

use super::display::Item;
use super::{ADDED, REMOVED, Row, Side};

/// A mark's kind; each kind is drawn on one half.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Removed,
    Added,
    /// A match on the old side's line, on the new side's.
    FoundOld,
    FoundNew,
}

impl Mark {
    /// In drawing order: the matches over the changes.
    pub const ALL: [Mark; 4] = [Mark::Removed, Mark::Added, Mark::FoundOld, Mark::FoundNew];

    pub fn side(self) -> Side {
        match self {
            Mark::Removed | Mark::FoundOld => Side::Old,
            Mark::Added | Mark::FoundNew => Side::New,
        }
    }
}

/// The marks one item of the list makes, by `Mark as usize`.
pub type ItemMarks = [bool; 4];

/// What `item` marks. `found`: the rows with a match, ascending. A match on an unchanged line is
/// on both sides; one in a fold marks the fold, whose rows are on both.
pub fn item_marks(rows: &[Row], item: &Item, found: &[u32]) -> ItemMarks {
    let mut m = [false; 4];
    let mut row = |r: u32| {
        let k = rows[r as usize].k;
        m[Mark::Removed as usize] |= k == REMOVED;
        m[Mark::Added as usize] |= k == ADDED;
        if found.binary_search(&r).is_ok() {
            m[Mark::FoundOld as usize] |= k != ADDED;
            m[Mark::FoundNew as usize] |= k != REMOVED;
        }
    };
    match *item {
        Item::Line(r) => row(r),
        Item::Pair { old, new } => {
            old.into_iter().for_each(&mut row);
            new.filter(|&n| Some(n) != old).into_iter().for_each(row);
        }
        Item::Gap { start, count } => {
            let from = found.partition_point(|&r| r < start);
            if found.get(from).is_some_and(|&r| r < start + count) {
                m[Mark::FoundOld as usize] = true;
                m[Mark::FoundNew as usize] = true;
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::display::{self, split, unified};

    fn row(k: u8, o: u32, n: u32) -> Row {
        Row { k, o, n, e: vec![] }
    }

    /// One line removed and one added between unchanged ones.
    fn rows() -> Vec<Row> {
        vec![
            row(0, 1, 1),
            row(REMOVED, 2, 0),
            row(ADDED, 0, 2),
            row(0, 3, 3),
        ]
    }

    fn marked(items: &[Item], rows: &[Row], found: &[u32]) -> Vec<ItemMarks> {
        items.iter().map(|it| item_marks(rows, it, found)).collect()
    }

    #[test]
    fn changes_go_on_their_side_and_matches_on_the_sides_their_line_is_on() {
        let rows = rows();
        let shown = display::all_rows(&rows);
        // Unified: a line an item. Matches on the unchanged first line and the removed one.
        let m = marked(&unified(&shown), &rows, &[0, 1]);
        assert_eq!(
            m,
            vec![
                [false, false, true, true],
                [true, false, true, false],
                [false, true, false, false],
                [false; 4],
            ]
        );
        // Split pairs the removed line with the added one: both changes on one item.
        let m = marked(&split(&rows, &shown), &rows, &[2]);
        assert_eq!(m, vec![[false; 4], [true, true, false, true], [false; 4]]);
    }

    #[test]
    fn a_match_in_a_fold_marks_the_fold() {
        let rows = rows();
        let fold = Item::Gap { start: 0, count: 1 };
        assert_eq!(item_marks(&rows, &fold, &[0]), [false, false, true, true]);
        assert_eq!(item_marks(&rows, &fold, &[1, 3]), [false; 4]);
    }
}
