//! Reordering the terminal tabs: dragged on the strip, or a place left or right from the tab's
//! menu and the keys (tab.moveLeft/moveRight). The order is the saved one.

use super::*;

pub(super) use gitviber_ui::widgets::{drop_edge, drop_line};

/// A tab on the move: its id, and its place when the drag began, for the drop line's side.
#[derive(Clone)]
pub(super) struct DraggedTerminal {
    pub gid: u64,
    pub from: usize,
    title: SharedString,
}

impl DraggedTerminal {
    pub fn new(gid: u64, from: usize, title: impl Into<SharedString>) -> Self {
        DraggedTerminal {
            gid,
            from,
            title: title.into(),
        }
    }
}

impl Render for DraggedTerminal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let icon = Icon::new(IconName::SquareTerminal)
            .size(px(14.))
            .text_color(cx.app_theme().muted_foreground);
        gitviber_ui::widgets::drag_chip(Some(icon.into_any_element()), self.title.clone(), cx)
    }
}

/// Moves the item at `from` to `to`, the ones between shifting over; false when nothing moves.
pub(super) fn move_to<T>(items: &mut Vec<T>, from: usize, to: usize) -> bool {
    if from == to || from >= items.len() || to >= items.len() {
        return false;
    }
    let item = items.remove(from);
    items.insert(to, item);
    true
}

impl TerminalPanel {
    /// The tab `gid` moved to place `to` among the open set's; it stays open if it was.
    pub(super) fn move_group_to(&mut self, gid: u64, to: usize, cx: &mut Context<Self>) {
        let Some(set) = self.sets.get_mut(&self.root) else {
            return;
        };
        let Some(from) = set.groups.iter().position(|g| g.id == gid) else {
            return;
        };
        if move_to(&mut set.groups, from, to) {
            self.save_soon(cx);
            cx.notify();
        }
    }

    /// The tab `gid` a place left (-1) or right (1); none past either end.
    pub(super) fn nudge_group(&mut self, gid: u64, by: isize, cx: &mut Context<Self>) {
        let Some(at) = self
            .set()
            .and_then(|s| s.groups.iter().position(|g| g.id == gid))
        else {
            return;
        };
        if let Some(to) = at.checked_add_signed(by) {
            self.move_group_to(gid, to, cx);
        }
    }

    /// The open tab a place left (-1) or right (1).
    pub(super) fn move_group(&mut self, by: isize, cx: &mut Context<Self>) {
        if let Some(gid) = self.set().and_then(|s| s.active) {
            self.nudge_group(gid, by, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tab_moves_to_its_drop_place() {
        let mut v = vec!['a', 'b', 'c', 'd'];
        assert!(move_to(&mut v, 0, 2));
        assert_eq!(v, ['b', 'c', 'a', 'd']);
        assert!(move_to(&mut v, 3, 0));
        assert_eq!(v, ['d', 'b', 'c', 'a']);
        assert!(move_to(&mut v, 1, 2));
        assert_eq!(v, ['d', 'c', 'b', 'a']);
        assert!(!move_to(&mut v, 1, 1));
        assert!(!move_to(&mut v, 4, 0));
        assert!(!move_to(&mut v, 0, 4));
        assert_eq!(v, ['d', 'c', 'b', 'a']);
        assert!(!move_to(&mut Vec::<u8>::new(), 0, 0));
    }

    #[test]
    fn the_drop_line_is_on_the_side_the_tab_lands() {
        assert_eq!(drop_edge(0, 2), Some(true));
        assert_eq!(drop_edge(3, 1), Some(false));
        assert_eq!(drop_edge(2, 2), None);
        // Each landing as its line said: right of a later tab, left of an earlier one.
        let mut v = vec![0, 1, 2, 3];
        move_to(&mut v, 0, 2);
        assert_eq!(v.iter().position(|&x| x == 0), Some(2));
        assert_eq!(v[1], 2);
        move_to(&mut v, 3, 1);
        assert_eq!(v, [1, 3, 2, 0]);
    }
}
