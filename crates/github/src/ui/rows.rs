//! The keyboard on the PR and issue lists, as the old app's lists had it: ↑/↓, Home/End and
//! PgUp/PgDn land on a row as a click does (its preview), ↵ keeps its tab, Space previews it,
//! → opens it and goes on to its page, ↓ on the last row loads more, ⇧F10 opens its menu.

use gpui_kit::{
    App, Bounds, Context, FocusHandle, KeyDownEvent, Pixels, ScrollStrategy,
    UniformListScrollHandle, Window, point, px, size,
};

use crate::logic::keys::move_target;

/// Rows a PgUp/PgDn moves.
const PAGE: usize = 10;
/// A PR's or issue's row, every one as tall.
pub const ROW: f32 = 44.;

pub struct Rows {
    pub focus: FocusHandle,
    pub scroll: UniformListScrollHandle,
    /// The row the keys are on, by its page's url.
    pub cursor: Option<String>,
}

impl Rows {
    pub fn new<V>(cx: &mut Context<V>) -> Self {
        Rows {
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            cursor: None,
        }
    }

    /// Where the keys start in `urls`: the cursor, else the open row, else the first.
    pub fn at(&self, urls: &[&str], active: Option<&str>) -> Option<usize> {
        let find = |u: &str| urls.iter().position(|x| *x == u);
        self.cursor
            .as_deref()
            .and_then(find)
            .or_else(|| active.and_then(find))
            .or((!urls.is_empty()).then_some(0))
    }

    pub fn put(&mut self, urls: &[&str], ix: usize) {
        self.cursor = urls.get(ix).map(|u| u.to_string());
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
    }

    pub fn is_focused(&self, window: &gpui_kit::Window) -> bool {
        self.focus.is_focused(window)
    }

    /// Row `ix`'s right-click menu from the keyboard. One scrolled out of view is brought in
    /// first; next-frame callbacks run before that frame's draw, so the second one sees it there.
    pub fn open_menu(&self, ix: usize, window: &mut Window, cx: &mut App) {
        if let Some(row) = shown_row(&self.scroll, ix) {
            return gitviber_refs::ui::open_row_menu(row, window, cx);
        }
        self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        let scroll = self.scroll.clone();
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| {
                if let Some(row) = shown_row(&scroll, ix) {
                    gitviber_refs::ui::open_row_menu(row, window, cx);
                }
            })
        });
    }
}

/// Where row `ix` was last drawn, when all of it is in view.
fn shown_row(scroll: &UniformListScrollHandle, ix: usize) -> Option<Bounds<Pixels>> {
    let s = scroll.0.borrow();
    let list = s.base_handle.bounds();
    let top = list.origin.y + s.base_handle.offset().y + px(ROW) * ix as f32;
    let row = Bounds::new(point(list.origin.x, top), size(list.size.width, px(ROW)));
    (list.size.height > px(0.) && row.top() >= list.top() && row.bottom() <= list.bottom())
        .then_some(row)
}

/// What a key on a list does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKey {
    /// Land on this row: its preview.
    Move(usize),
    /// Keep its tab, as a double-click.
    Keep,
    Preview,
    /// Open it and go on to its page.
    Page,
    /// The next page of the list.
    More,
    /// Its right-click menu (⇧F10).
    Menu,
}

/// The key's meaning on row `at` of `n`; `more`: a next page can load.
pub fn row_key(ev: &KeyDownEvent, at: Option<usize>, n: usize, more: bool) -> Option<RowKey> {
    let at = at?;
    if gitviber_refs::ui::is_menu_key(&ev.keystroke) {
        return Some(RowKey::Menu);
    }
    let m = ev.keystroke.modifiers;
    if m.alt || m.platform || m.control || m.shift {
        return None;
    }
    let key = ev.keystroke.key.as_str();
    if key == "down" && at + 1 == n && more {
        return Some(RowKey::More);
    }
    if let Some(to) = move_target(key, at, n, PAGE) {
        return Some(RowKey::Move(to));
    }
    match key {
        "enter" => Some(RowKey::Keep),
        "space" => Some(RowKey::Preview),
        "right" => Some(RowKey::Page),
        _ => None,
    }
}

/// The workspace's tab gets focus, as → on a row does.
pub fn focus_page(
    ws: &gpui_kit::WeakEntity<gitviber_ui::workspace::Workspace>,
    window: &mut gpui_kit::Window,
    cx: &mut App,
) {
    if let Some(ws) = ws.upgrade() {
        ws.update(cx, |ws, cx| ws.focus_center(window, cx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> KeyDownEvent {
        KeyDownEvent {
            keystroke: gpui_kit::Keystroke::parse(s).unwrap(),
            is_held: false,
            prefer_character_input: false,
        }
    }

    #[test]
    fn shift_f10_opens_the_rows_menu_and_other_shifted_keys_pass() {
        assert_eq!(
            row_key(&key("shift-f10"), Some(2), 5, false),
            Some(RowKey::Menu)
        );
        assert_eq!(row_key(&key("shift-f10"), None, 0, false), None);
        assert_eq!(row_key(&key("shift-down"), Some(2), 5, false), None);
        assert_eq!(row_key(&key("down"), Some(4), 5, true), Some(RowKey::More));
        assert_eq!(
            row_key(&key("enter"), Some(1), 5, false),
            Some(RowKey::Keep)
        );
    }
}
