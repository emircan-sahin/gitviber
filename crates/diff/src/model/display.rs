//! Which rows a diff shows: all of them, or the changes with context around them and the rest
//! folded, each fold opening some of its lines a click. Split view pairs a change's removed
//! lines with its added ones, side by side.

use super::{ADDED, REMOVED, Row};
use std::collections::HashSet;

/// Lines of context kept around a change when the rest folds.
pub const CONTEXT: usize = 3;
/// Unchanged runs shorter than this stay shown when the rest fold.
pub const FOLD_MIN: usize = 3;
/// Lines a fold opens a click.
pub const FOLD_REVEAL: u32 = 20;

/// One entry of what shows, in row order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Row(u32),
    /// `count` unchanged rows from `start`, folded away.
    Gap {
        start: u32,
        count: u32,
    },
}

/// `rows` as they show: the changes with `context` lines around each, the rest folded into
/// gaps, except runs shorter than `minimum` and rows whose new line was `revealed`.
pub fn shown_rows(
    rows: &[Row],
    context: usize,
    minimum: usize,
    revealed: &HashSet<u32>,
) -> Vec<Shown> {
    let mut keep: Vec<bool> = rows
        .iter()
        .map(|r| r.k != 0 || revealed.contains(&r.n))
        .collect();
    for (i, r) in rows.iter().enumerate() {
        if r.k == 0 {
            continue;
        }
        let end = (i + context).min(rows.len() - 1);
        for k in &mut keep[i.saturating_sub(context)..=end] {
            *k = true;
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let mut end = i;
        while end < rows.len() && !keep[end] {
            end += 1;
        }
        if end > i && end - i >= minimum {
            out.push(Shown::Gap {
                start: i as u32,
                count: (end - i) as u32,
            });
        } else {
            out.extend((i..end).map(|ix| Shown::Row(ix as u32)));
        }
        if end < rows.len() {
            out.push(Shown::Row(end as u32));
        }
        i = end + 1;
    }
    out
}

/// Every row, nothing folded.
pub fn all_rows(rows: &[Row]) -> Vec<Shown> {
    (0..rows.len() as u32).map(Shown::Row).collect()
}

/// A line of the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// Unified: one row.
    Line(u32),
    /// Split: an old row on the left and a new one on the right; an unchanged row is both.
    Pair {
        old: Option<u32>,
        new: Option<u32>,
    },
    Gap {
        start: u32,
        count: u32,
    },
}

pub fn unified(shown: &[Shown]) -> Vec<Item> {
    shown
        .iter()
        .map(|s| match *s {
            Shown::Row(ix) => Item::Line(ix),
            Shown::Gap { start, count } => Item::Gap { start, count },
        })
        .collect()
}

/// Side by side: a run of changed rows pairs its k-th removed line with its k-th added one.
pub fn split(rows: &[Row], shown: &[Shown]) -> Vec<Item> {
    let mut out = Vec::with_capacity(shown.len());
    let (mut dels, mut adds): (Vec<u32>, Vec<u32>) = (vec![], vec![]);
    let flush = |out: &mut Vec<Item>, dels: &mut Vec<u32>, adds: &mut Vec<u32>| {
        for k in 0..dels.len().max(adds.len()) {
            out.push(Item::Pair {
                old: dels.get(k).copied(),
                new: adds.get(k).copied(),
            });
        }
        dels.clear();
        adds.clear();
    };
    for s in shown {
        match *s {
            Shown::Row(ix) => match rows[ix as usize].k {
                REMOVED => dels.push(ix),
                ADDED => adds.push(ix),
                _ => {
                    flush(&mut out, &mut dels, &mut adds);
                    out.push(Item::Pair {
                        old: Some(ix),
                        new: Some(ix),
                    });
                }
            },
            Shown::Gap { start, count } => {
                flush(&mut out, &mut dels, &mut adds);
                out.push(Item::Gap { start, count });
            }
        }
    }
    flush(&mut out, &mut dels, &mut adds);
    out
}

/// The new lines a click on a fold opens: its first `FOLD_REVEAL`.
pub fn reveal(rows: &[Row], start: u32, count: u32) -> impl Iterator<Item = u32> + '_ {
    let end = (start + count.min(FOLD_REVEAL)) as usize;
    rows[start as usize..end].iter().map(|r| r.n)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn gap(start: u32, count: u32) -> Shown {
        Shown::Gap { start, count }
    }
    use Shown::Row as R;
    fn none() -> HashSet<u32> {
        HashSet::new()
    }

    // As diffHunks.test.ts: gaps there name the first row's lines; here, its index.
    #[test]
    fn keeps_changes_and_context_and_folds_the_rest() {
        // A line added after the fourth of eight.
        let rows = vec![
            same(1, 1),
            same(2, 2),
            same(3, 3),
            same(4, 4),
            add(5),
            same(5, 6),
            same(6, 7),
            same(7, 8),
            same(8, 9),
        ];
        assert_eq!(
            shown_rows(&rows, 1, 1, &none()),
            vec![gap(0, 3), R(3), R(4), R(5), gap(6, 3)]
        );
        assert_eq!(shown_rows(&[same(1, 1)], 3, 1, &none()), vec![gap(0, 1)]);
        assert_eq!(shown_rows(&[], 3, 1, &none()), vec![]);
    }

    #[test]
    fn runs_shorter_than_the_minimum_stay_shown() {
        let rows = vec![
            same(1, 1),
            same(2, 2),
            del(3),
            same(4, 3),
            same(5, 4),
            same(6, 5),
            same(7, 6),
        ];
        assert_eq!(
            shown_rows(&rows, 0, 3, &none()),
            vec![R(0), R(1), R(2), gap(3, 4)]
        );
        assert_eq!(shown_rows(&rows, 0, 5, &none()), all_rows(&rows));
    }

    #[test]
    fn changes_at_the_edges_overlapping_context_and_none() {
        let rows = vec![
            add(1),
            same(1, 2),
            same(2, 3),
            del(3),
            same(4, 4),
            same(5, 5),
            same(6, 6),
            add(7),
        ];
        assert_eq!(
            shown_rows(&rows, 1, 1, &none()),
            vec![R(0), R(1), R(2), R(3), R(4), gap(5, 1), R(6), R(7)]
        );
        assert_eq!(shown_rows(&rows, 2, 1, &none()), all_rows(&rows));
        assert_eq!(
            shown_rows(&rows, 0, 1, &none()),
            vec![R(0), gap(1, 2), R(3), gap(4, 3), R(7)]
        );
    }

    #[test]
    fn deletions_keep_old_context_and_revealed_lines_open_a_fold() {
        let rows = vec![
            same(1, 1),
            same(2, 2),
            same(3, 3),
            del(4),
            del(5),
            same(6, 4),
            same(7, 5),
            same(8, 6),
        ];
        assert_eq!(
            shown_rows(&rows, 1, 1, &none()),
            vec![gap(0, 2), R(2), R(3), R(4), R(5), gap(6, 2)]
        );
        let revealed = HashSet::from([1]);
        assert_eq!(
            shown_rows(&rows, 1, 1, &revealed),
            vec![R(0), gap(1, 1), R(2), R(3), R(4), R(5), gap(6, 2)]
        );
        assert_eq!(reveal(&rows, 6, 2).collect::<Vec<_>>(), vec![5, 6]);
    }

    #[test]
    fn split_pairs_removed_with_added() {
        let rows = vec![same(1, 1), del(2), del(3), add(2), same(4, 3), add(4)];
        let items = split(&rows, &all_rows(&rows));
        assert_eq!(
            items,
            vec![
                Item::Pair {
                    old: Some(0),
                    new: Some(0)
                },
                Item::Pair {
                    old: Some(1),
                    new: Some(3)
                },
                Item::Pair {
                    old: Some(2),
                    new: None
                },
                Item::Pair {
                    old: Some(4),
                    new: Some(4)
                },
                Item::Pair {
                    old: None,
                    new: Some(5)
                },
            ]
        );
        assert_eq!(
            split(&rows, &[gap(0, 1), R(1)]),
            vec![
                Item::Gap { start: 0, count: 1 },
                Item::Pair {
                    old: Some(1),
                    new: None
                }
            ]
        );
    }
}
