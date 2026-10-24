//! What an owner draws between a diff's lines: a pull request's review thread, a guided review's
//! note. Each sits under the line it's anchored to, on the side its line number counts; the
//! list keeps its room (declared, then as measured) and the line at the top stays put as rooms
//! come and go.

use crate::model::display::Item;
use crate::model::{ADDED, LinePos, REMOVED, Row, Side};
use gpui_kit::{AnyElement, App, Pixels, SharedString, Window};
use std::collections::HashMap;
use std::rc::Rc;

pub type RenderAnnotation = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

/// One block under a line.
#[derive(Clone)]
pub struct Annotation {
    /// Stable across updates: its measured height is kept under it.
    pub id: SharedString,
    /// The line it goes under: a new line on the new side, an old one on the old side (a
    /// comment on a removed line, or on the old version of an unchanged one).
    pub pos: LinePos,
    /// Its height until it's laid out; then the list follows what it measures.
    pub height: Pixels,
    pub render: RenderAnnotation,
}

impl Annotation {
    pub fn new(
        id: impl Into<SharedString>,
        pos: LinePos,
        height: Pixels,
        render: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        Annotation {
            id: id.into(),
            pos,
            height,
            render: Rc::new(render),
        }
    }
}

/// The row an annotation goes under: a new line's row on the new side, an old line's (removed
/// or unchanged) on the old side.
pub fn row_of(rows: &[Row], pos: LinePos) -> Option<usize> {
    rows.iter().position(|r| match pos.side {
        Side::New => r.k != REMOVED && r.n == pos.line,
        Side::Old => r.k != ADDED && r.o == pos.line,
    })
}

/// The unchanged lines (new numbers) a fold must open so an annotation on one shows.
pub fn lines_to_open(rows: &[Row], notes: &[Annotation]) -> Vec<u32> {
    notes
        .iter()
        .filter_map(|a| row_of(rows, a.pos))
        .filter(|&r| rows[r].k == 0)
        .map(|r| rows[r].n)
        .collect()
}

/// Which item each annotation goes under: item index → annotation indexes, in their order.
/// One on a folded or missing line goes nowhere.
pub fn place(rows: &[Row], items: &[Item], notes: &[Annotation]) -> HashMap<usize, Vec<usize>> {
    let mut item_of_row = vec![usize::MAX; rows.len()];
    for (ix, item) in items.iter().enumerate() {
        match *item {
            Item::Line(r) => item_of_row[r as usize] = ix,
            Item::Pair { old, new } => {
                for r in old.into_iter().chain(new) {
                    item_of_row[r as usize] = ix;
                }
            }
            Item::Gap { .. } => {}
        }
    }
    let mut out: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, a) in notes.iter().enumerate() {
        if let Some(ix) = row_of(rows, a.pos)
            .map(|r| item_of_row[r])
            .filter(|ix| *ix != usize::MAX)
        {
            out.entry(ix).or_default().push(i);
        }
    }
    out
}

/// The key an item's annotations are measured under: their ids, so a new or removed one is
/// measured afresh.
pub fn measure_key(notes: &[Annotation], placed: &[usize]) -> SharedString {
    placed
        .iter()
        .map(|&i| notes[i].id.as_ref())
        .collect::<Vec<_>>()
        .join("\u{1f}")
        .into()
}

/// The room an item's annotations take: as measured, else as declared.
pub fn extra(
    notes: &[Annotation],
    placed: &[usize],
    measured: &HashMap<SharedString, Pixels>,
) -> Pixels {
    measured
        .get(&measure_key(notes, placed))
        .copied()
        .unwrap_or_else(|| {
            placed
                .iter()
                .fold(Pixels::ZERO, |h, &i| h + notes[i].height)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::display;
    use gpui_kit::{IntoElement as _, div, px};

    fn row(k: u8, o: u32, n: u32) -> Row {
        Row { k, o, n, e: vec![] }
    }
    fn note(id: &str, side: Side, line: u32) -> Annotation {
        Annotation::new(id.to_string(), LinePos { side, line }, px(40.), |_, _| {
            div().into_any_element()
        })
    }

    #[test]
    fn annotations_go_under_their_side_of_the_line() {
        let rows = vec![
            row(0, 1, 1),
            row(REMOVED, 2, 0),
            row(ADDED, 0, 2),
            row(0, 3, 3),
        ];
        let items = display::unified(&display::all_rows(&rows));
        let notes = vec![
            note("new2", Side::New, 2),
            note("old2", Side::Old, 2),
            note("old3", Side::Old, 3),
            note("gone", Side::New, 9),
        ];
        let placed = place(&rows, &items, &notes);
        assert_eq!(placed.get(&2), Some(&vec![0]));
        assert_eq!(placed.get(&1), Some(&vec![1]));
        assert_eq!(placed.get(&3), Some(&vec![2]));
        assert_eq!(placed.len(), 3);
        // Split: a removed line and its added pair share an item.
        let split = display::split(&rows, &display::all_rows(&rows));
        assert_eq!(place(&rows, &split, &notes[..2]).get(&1), Some(&vec![0, 1]));
        // An unchanged line under a note opens from its fold.
        assert_eq!(lines_to_open(&rows, &notes), vec![3]);
        assert_eq!(extra(&notes, &[0, 1], &HashMap::new()), px(80.));
    }
}
