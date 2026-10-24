//! The list's keys do what the mouse does on its rows. ↑/↓, Home/End and PgUp/PgDn walk every
//! row there is to reach (files, folders, stashes and their files): a file previews its diff as
//! a click does, a folder or a stash only takes the cursor. ⇧ with a move picks files along; ↵
//! keeps a file's tab, Space opens it, and both open or close a folder or stash; → opens a
//! folder or stash, or goes on from a file into its diff; ← closes one, or goes up to the row
//! it's in; ⇧F10 opens the row's menu.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Move {
    By(isize),
    Home,
    End,
    PageUp,
    PageDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RowKey {
    Enter,
    Space,
    Left,
    Right,
}

impl ChangesPanel {
    /// The rows the keys stop on.
    fn nav_rows(&self) -> Vec<usize> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                matches!(
                    r,
                    Row::Item(_) | Row::Review(_) | Row::Stash(_) | Row::StashFile { .. }
                )
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// What row `ix` opens, for the rows that open a tab.
    pub(super) fn row_selection(&self, ix: usize) -> Option<Selection> {
        match self.rows.get(ix)? {
            Row::Item(Item::File { change, .. }) => {
                Some(selection_of(change.kind, change.file.clone()))
            }
            Row::Review(f) => self.review_selection(f),
            Row::StashFile {
                stash, sha, file, ..
            } => Some(self.stash_file_selection(*stash, sha, file.clone())),
            _ => None,
        }
    }

    /// The row the keys act on: the one they last landed on, else the open tab's.
    pub(super) fn cursor_ix(&self, cx: &App) -> Option<usize> {
        self.cursor
            .as_ref()
            .and_then(|k| self.keys.iter().position(|x| x == k))
            .or_else(|| self.open_ix(cx))
    }

    /// The open tab's row.
    pub(super) fn open_ix(&self, cx: &App) -> Option<usize> {
        let open = self.open_key.clone().or_else(|| {
            self.workspace()
                .and_then(|ws| ws.read(cx).tabs().active_key().map(|k| k.to_string()))
        })?;
        self.nav_rows().into_iter().find(|ix| {
            self.row_selection(*ix)
                .is_some_and(|s| selection_key(&s) == open)
        })
    }

    /// The cursor onto row `ix`, in view; no tab opens.
    fn put_cursor(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.cursor = self.keys.get(ix).cloned();
        self.scroll
            .scroll_to_item(ix, gpui_kit::ScrollStrategy::Nearest);
        cx.notify();
    }

    /// The cursor onto row `ix`, which previews its file as a click does. `extend`: picks the
    /// files from the anchor to it; on a folder or a stash the picked files stay as they are.
    fn land(&mut self, ix: usize, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        let from = self.cursor_ix(cx).and_then(|i| self.keys.get(i).cloned());
        self.put_cursor(ix, cx);
        let Some(sel) = self.row_selection(ix) else {
            return;
        };
        let key = self.keys[ix].clone();
        if !extend {
            self.picked = None;
        } else if matches!(self.rows[ix], Row::Item(_) | Row::Review(_)) {
            let anchor = self
                .picked
                .as_ref()
                .map(|p| p.1.clone())
                .or(from)
                .unwrap_or(key.clone());
            self.picked = Some((range_of(&self.file_keys(), &anchor, &key), anchor));
        }
        self.open(sel, true, window, cx);
    }

    /// Rows a PgUp / PgDn moves: as many as fit, one kept for context.
    fn page_rows(&self) -> usize {
        let height = self.scroll.0.borrow().base_handle.bounds().size.height;
        ((height / px(ROW)) as usize).saturating_sub(1).max(1)
    }

    pub(super) fn nav_move(
        &mut self,
        m: Move,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let nav = self.nav_rows();
        let Some(last) = nav.len().checked_sub(1) else {
            return;
        };
        let at = self
            .cursor_ix(cx)
            .and_then(|ix| nav.iter().position(|n| *n == ix));
        let page = self.page_rows();
        let to = match (at, m) {
            (_, Move::Home) => 0,
            (_, Move::End) => last,
            // Nothing landed on yet: the first row.
            (None, _) => 0,
            (Some(i), Move::By(d)) => (i as isize + d).clamp(0, last as isize) as usize,
            (Some(i), Move::PageDown) => (i + page).min(last),
            (Some(i), Move::PageUp) => i.saturating_sub(page),
        };
        self.land(nav[to], extend, window, cx);
    }

    /// The folder row that row `ix` sits in, in the tree.
    fn parent_folder(&self, ix: usize) -> Option<usize> {
        let depth = match self.rows.get(ix)? {
            Row::Item(Item::File { depth, .. } | Item::Folder { depth, .. }) => *depth,
            _ => return None,
        };
        (0..ix).rev().find(
            |i| matches!(&self.rows[*i], Row::Item(Item::Folder { depth: d, .. }) if *d < depth),
        )
    }

    /// The next row the keys stop on after `ix`.
    fn next_nav(&self, ix: usize) -> Option<usize> {
        self.nav_rows().into_iter().find(|i| *i > ix)
    }

    fn toggle_folder(&mut self, kind: Kind, path: &str, open: bool, cx: &mut Context<Self>) {
        let key = folder_key(kind, path);
        if open {
            self.closed.remove(&key);
        } else {
            self.closed.insert(key);
        }
        self.rebuild(cx);
    }

    /// ↵, Space, ← and → on the cursor's row; false when the key isn't the row's to take.
    pub(super) fn row_key(
        &mut self,
        k: RowKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(ix) = self.cursor_ix(cx) else {
            return false;
        };
        let row = self.rows[ix].clone();
        match (row, k) {
            (
                Row::Item(Item::Folder {
                    kind, path, open, ..
                }),
                k,
            ) => match k {
                RowKey::Enter | RowKey::Space => self.toggle_folder(kind, &path, !open, cx),
                RowKey::Right if !open => self.toggle_folder(kind, &path, true, cx),
                RowKey::Right => match self.next_nav(ix) {
                    Some(next) => self.land(next, false, window, cx),
                    None => return false,
                },
                RowKey::Left if open => self.toggle_folder(kind, &path, false, cx),
                RowKey::Left => match self.parent_folder(ix) {
                    Some(up) => self.put_cursor(up, cx),
                    None => return false,
                },
            },
            (Row::Stash(i), k) => {
                let sha = self.stashes[i].sha.clone();
                let expanded = self.expanded_stash.as_deref() == Some(sha.as_str());
                match k {
                    RowKey::Enter | RowKey::Space => self.toggle_stash(sha, cx),
                    RowKey::Right if !expanded => self.toggle_stash(sha, cx),
                    RowKey::Right => match self.next_nav(ix) {
                        Some(next) => self.land(next, false, window, cx),
                        None => return false,
                    },
                    RowKey::Left if expanded => self.toggle_stash(sha, cx),
                    RowKey::Left => return false,
                }
            }
            (row, k) => {
                let Some(sel) = self.row_selection(ix) else {
                    return false;
                };
                match k {
                    RowKey::Enter => self.open(sel, false, window, cx),
                    RowKey::Space => {
                        self.picked = None;
                        self.open(sel, true, window, cx);
                    }
                    RowKey::Right => {
                        if self.open_key.as_deref() != Some(selection_key(&sel).as_str()) {
                            self.open(sel, true, window, cx);
                        }
                        if let Some(ws) = self.workspace() {
                            ws.update(cx, |ws, cx| ws.focus_center(window, cx));
                        }
                    }
                    RowKey::Left => {
                        let up = match row {
                            Row::StashFile { stash, .. } => self
                                .rows
                                .iter()
                                .position(|r| matches!(r, Row::Stash(i) if *i == stash)),
                            _ => self.parent_folder(ix),
                        };
                        match up {
                            Some(up) => self.put_cursor(up, cx),
                            None => return false,
                        }
                    }
                }
            }
        }
        cx.notify();
        true
    }

    /// A key of the list's own, taken only while the list itself has focus (not the filter or
    /// the commit box in the panel) and the row does something with it.
    pub(super) fn list_key(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        act: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) -> bool,
    ) {
        if !self.focus.is_focused(window) || !act(self, window, cx) {
            cx.propagate();
        }
    }

    /// ⇧F10: the cursor row's right-click menu. One scrolled out of view comes in first; the
    /// second frame has it drawn there.
    pub(super) fn open_row_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.cursor_ix(cx) else {
            return;
        };
        if let Some(row) = shown_row(&self.scroll, ix) {
            return gitviber_refs::ui::open_row_menu(row, window, cx);
        }
        self.scroll
            .scroll_to_item(ix, gpui_kit::ScrollStrategy::Nearest);
        let scroll = self.scroll.clone();
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| {
                if let Some(row) = shown_row(&scroll, ix) {
                    gitviber_refs::ui::open_row_menu(row, window, cx);
                }
            })
        });
        cx.notify();
    }
}

/// Where row `ix` was last drawn, when all of it is in view.
fn shown_row(
    scroll: &UniformListScrollHandle,
    ix: usize,
) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
    let s = scroll.0.borrow();
    let list = s.base_handle.bounds();
    let top = list.origin.y + s.base_handle.offset().y + px(ROW) * ix as f32;
    let row = gpui_kit::Bounds::new(
        gpui_kit::point(list.origin.x, top),
        gpui_kit::size(list.size.width, px(ROW)),
    );
    (list.size.height > px(0.) && row.top() >= list.top() && row.bottom() <= list.bottom())
        .then_some(row)
}
