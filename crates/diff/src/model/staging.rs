//! Which lines staging, unstaging, discarding or stashing part of a diff takes: a change the
//! chosen lines touch goes with the chosen ones of its side and all of the other side's (the
//! lines they replace). core's lines.rs writes them.

use super::{REMOVED, Row, Side};
use std::ops::Range;

/// A run of changed lines: old lines `[start, end)` removed, new lines `[start, end)` added,
/// 1-based; an empty range sits before `start`. `rows`: where it is in the diff's rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub original: (u32, u32),
    pub modified: (u32, u32),
    pub rows: Range<u32>,
}

impl Change {
    fn side(&self, side: Side) -> (u32, u32) {
        match side {
            Side::Old => self.original,
            Side::New => self.modified,
        }
    }
}

/// The lines an action takes: removed ones by their old line, added ones by their new line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picked {
    pub removed: Vec<u32>,
    pub added: Vec<u32>,
}

impl Picked {
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.added.is_empty()
    }

    fn take(&mut self, side: Side, mine: Vec<u32>, theirs: Vec<u32>) {
        let (removed, added) = match side {
            Side::New => (theirs, mine),
            Side::Old => (mine, theirs),
        };
        self.removed.extend(removed);
        self.added.extend(added);
    }
}

pub fn changes(rows: &[Row]) -> Vec<Change> {
    let mut out = Vec::new();
    // The next old and new line.
    let (mut o, mut n) = (1, 1);
    let mut i = 0;
    while i < rows.len() {
        if rows[i].k == 0 {
            o = rows[i].o + 1;
            n = rows[i].n + 1;
            i += 1;
            continue;
        }
        let (o0, n0, first) = (o, n, i);
        while i < rows.len() && rows[i].k != 0 {
            if rows[i].k == REMOVED {
                o = rows[i].o + 1;
            } else {
                n = rows[i].n + 1;
            }
            i += 1;
        }
        out.push(Change {
            original: (o0, o),
            modified: (n0, n),
            rows: first as u32..i as u32,
        });
    }
    out
}

fn lines((a, b): (u32, u32)) -> Vec<u32> {
    (a..b.max(a)).collect()
}

/// What choosing lines `from..=to` of `side` takes. A change with none of that side's lines
/// counts when the line above it is chosen.
pub fn pick(list: &[Change], side: Side, from: u32, to: u32) -> Picked {
    let other = match side {
        Side::Old => Side::New,
        Side::New => Side::Old,
    };
    let mut out = Picked::default();
    for c in list {
        let (a, b) = c.side(side);
        let mine = if a < b {
            lines((a.max(from), b.min(to + 1)))
        } else {
            vec![]
        };
        let above = (a.max(2) - 1).max(1);
        let touched = if a < b {
            !mine.is_empty()
        } else {
            above >= from && above <= to
        };
        if touched {
            out.take(side, mine, lines(c.side(other)));
        }
    }
    out
}

/// What choosing the rows `chosen` of a unified diff takes, where both sides' lines can be
/// chosen: a change with lines chosen on both sides takes just those; with one side's only, it
/// takes them and all of the other side's, as `pick` does.
pub fn pick_rows(rows: &[Row], list: &[Change], chosen: Range<u32>) -> Picked {
    let mut out = Picked::default();
    for c in list {
        let from = c.rows.start.max(chosen.start);
        let to = c.rows.end.min(chosen.end);
        if from >= to {
            continue;
        }
        let (mut dels, mut adds) = (vec![], vec![]);
        for r in &rows[from as usize..to as usize] {
            if r.k == REMOVED {
                dels.push(r.o)
            } else {
                adds.push(r.n)
            }
        }
        match (dels.is_empty(), adds.is_empty()) {
            (false, false) => {
                out.removed.extend(dels);
                out.added.extend(adds);
            }
            (true, _) => out.take(Side::New, adds, lines(c.original)),
            (_, true) => out.take(Side::Old, dels, lines(c.modified)),
        }
    }
    out
}

/// The change at `line` of `side`: one with lines there, else one that sits right above or
/// below it.
pub fn change_at(list: &[Change], side: Side, line: u32) -> Option<usize> {
    list.iter()
        .position(|c| {
            let (a, b) = c.side(side);
            a <= line && line < b
        })
        .or_else(|| {
            list.iter().position(|c| {
                let (a, b) = c.side(side);
                a == b && (line + 1 == a || line == a)
            })
        })
}

/// The change a row is in.
pub fn change_of_row(list: &[Change], row: u32) -> Option<usize> {
    let i = list.partition_point(|c| c.rows.end <= row);
    list.get(i).filter(|c| c.rows.contains(&row)).map(|_| i)
}

/// All of a change.
pub fn whole(c: &Change) -> Picked {
    Picked {
        removed: lines(c.original),
        added: lines(c.modified),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ADDED;

    fn same(o: u32, n: u32) -> Row {
        Row {
            k: 0,
            o,
            n,
            e: vec![],
        }
    }
    fn add(n: u32) -> Row {
        Row {
            k: ADDED,
            o: 0,
            n,
            e: vec![],
        }
    }
    fn del(o: u32) -> Row {
        Row {
            k: REMOVED,
            o,
            n: 0,
            e: vec![],
        }
    }
    fn picked(removed: &[u32], added: &[u32]) -> Picked {
        Picked {
            removed: removed.to_vec(),
            added: added.to_vec(),
        }
    }
    fn spans(list: &[Change]) -> Vec<((u32, u32), (u32, u32))> {
        list.iter().map(|c| (c.original, c.modified)).collect()
    }

    // As lineStaging.test.ts: 1 kept; 2-3 replaced by 2; 4 kept; 5 added; 5 kept; 6 removed; 6 kept.
    fn rows() -> Vec<Row> {
        vec![
            same(1, 1),
            del(2),
            del(3),
            add(2),
            same(4, 3),
            add(4),
            add(5),
            same(5, 6),
            del(6),
            same(7, 7),
        ]
    }

    #[test]
    fn runs_of_changed_lines_with_where_they_sit() {
        let list = changes(&rows());
        assert_eq!(
            spans(&list),
            vec![((2, 4), (2, 3)), ((5, 5), (4, 6)), ((6, 7), (7, 7))]
        );
        assert_eq!(list[1].rows, 5..7);
        assert_eq!(spans(&changes(&[add(1), add(2)])), vec![((1, 1), (1, 3))]);
        assert_eq!(
            spans(&changes(&[del(1), same(2, 1)])),
            vec![((1, 2), (1, 1))]
        );
    }

    #[test]
    fn chosen_new_lines_take_what_they_replace_and_only_themselves_of_the_rest() {
        let list = changes(&rows());
        assert_eq!(pick(&list, Side::New, 2, 2), picked(&[2, 3], &[2]));
        assert_eq!(pick(&list, Side::New, 5, 5), picked(&[], &[5]));
        // A removal counts when the line above it is chosen.
        assert_eq!(pick(&list, Side::New, 6, 6), picked(&[6], &[]));
        assert_eq!(pick(&list, Side::New, 1, 7), picked(&[2, 3, 6], &[2, 4, 5]));
        assert_eq!(pick(&list, Side::New, 3, 3), picked(&[], &[]));
    }

    #[test]
    fn from_the_old_side_too() {
        let list = changes(&rows());
        assert_eq!(pick(&list, Side::Old, 3, 3), picked(&[3], &[2]));
        assert_eq!(pick(&list, Side::Old, 4, 4), picked(&[], &[4, 5]));
    }

    #[test]
    fn the_change_at_a_line() {
        let list = changes(&rows());
        assert_eq!(change_at(&list, Side::New, 2), Some(0));
        assert_eq!(change_at(&list, Side::New, 5), Some(1));
        // Removed lines sit between 6 and 7: either one finds them.
        assert_eq!(change_at(&list, Side::New, 6), Some(2));
        assert_eq!(change_at(&list, Side::New, 7), Some(2));
        assert_eq!(change_at(&list, Side::New, 1), None);
        assert_eq!(whole(&list[0]), picked(&[2, 3], &[2]));
        assert_eq!(
            (
                change_of_row(&list, 3),
                change_of_row(&list, 4),
                change_of_row(&list, 8)
            ),
            (Some(0), None, Some(2))
        );
    }

    #[test]
    fn unified_rows_take_both_sides_when_both_are_chosen() {
        let r = rows();
        let list = changes(&r);
        // Old line 3 and new line 2: just those.
        assert_eq!(pick_rows(&r, &list, 2..4), picked(&[3], &[2]));
        // New line 2 alone takes what it replaces.
        assert_eq!(pick_rows(&r, &list, 3..4), picked(&[2, 3], &[2]));
        // Old line 2 alone takes the new line too.
        assert_eq!(pick_rows(&r, &list, 1..2), picked(&[2], &[2]));
        // Context rows take nothing.
        assert_eq!(pick_rows(&r, &list, 0..1), picked(&[], &[]));
        assert_eq!(pick_rows(&r, &list, 0..10), picked(&[2, 3, 6], &[2, 4, 5]));
    }
}
