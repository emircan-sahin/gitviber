//! The keyboard on the PR and issue lists, as the old app's lists had it: ↑/↓, Home/End and
//! PgUp/PgDn land on a row as a click does (its preview), ↵ keeps its tab, Space previews it,
//! → opens it and goes on to its page, ↓ on the last row loads more.

use gpui_kit::{App, Context, FocusHandle, KeyDownEvent, ScrollStrategy, UniformListScrollHandle};

use crate::logic::keys::move_target;

/// Rows a PgUp/PgDn moves.
const PAGE: usize = 10;

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
}

/// The key's meaning on row `at` of `n`; `more`: a next page can load.
pub fn row_key(ev: &KeyDownEvent, at: Option<usize>, n: usize, more: bool) -> Option<RowKey> {
    let m = ev.keystroke.modifiers;
    if m.alt || m.platform || m.control || m.shift {
        return None;
    }
    let key = ev.keystroke.key.as_str();
    let at = at?;
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
