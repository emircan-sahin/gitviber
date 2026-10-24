//! Tabs follow git. The open row leaving its list (staged, unstaged, discarded here) hands over
//! to the row now in its place at once, without waiting for git, so S, S, S walks down the
//! list. After each read, a change tab whose file moved lists goes with it, and one whose file
//! is gone (committed, discarded) closes. Branch review tabs follow the review the same way: onto
//! the new merge base after a rebase or another base, closed once their file is out of it.

use super::*;
use crate::model::list::{relocate, row_in_place};

impl ChangesPanel {
    /// Before `rows` of `kind` leave their list. Not conflicts: a resolved one's tab follows it
    /// into Staged.
    pub(super) fn leave(&mut self, kind: Kind, rows: &[Arc<FileChange>], cx: &mut Context<Self>) {
        if kind == Kind::Conflict {
            return;
        }
        let keys: Vec<String> = rows
            .iter()
            .map(|f| selection_key(&selection_of(kind, f.clone())))
            .collect();
        let from = self.open_key.clone().filter(|k| keys.contains(k));
        self.leaving.extend(keys);
        let Some(from) = from else { return };
        let prefix = format!("{}::", kind.name());
        let shown: Vec<String> = self
            .file_keys()
            .into_iter()
            .filter(|k| k.starts_with(&prefix))
            .collect();
        let to = row_in_place(&shown, |k| !self.leaving.contains(k), &from)
            .and_then(|k| self.selection_at(&k));
        // Nothing left in its place: the tab follows the file instead (follow_git).
        let Some(to) = to else { return };
        self.back = self.selection_at(&from).map(|f| (f, selection_key(&to)));
        self.picked = None;
        self.open_later(to, true, cx);
    }

    /// The action failed: back to the file it left, unless another row was opened meanwhile.
    pub(super) fn settle_leave(&mut self, failed: bool, cx: &mut Context<Self>) {
        let Some((from, to)) = self.back.take() else {
            return;
        };
        if failed && self.open_key.as_deref() == Some(to.as_str()) {
            self.open_later(from, true, cx);
        }
    }

    /// The change tabs whose file is elsewhere in the lists just read, or gone. A tab with
    /// edits typed in stays as it is.
    pub(super) fn follow_git(&self, cx: &mut Context<Self>) {
        let Some(ws) = self.workspace() else { return };
        let moves: Vec<(SharedString, Option<Selection>)> = ws
            .read(cx)
            .tabs()
            .tabs()
            .iter()
            .filter_map(|t| {
                let (kind, file) = match t.item.selection()? {
                    Selection::Unstaged { file } => (Kind::Unstaged, file),
                    Selection::Staged { file } => (Kind::Staged, file),
                    Selection::Conflict { file } => (Kind::Conflict, file),
                    _ => return None,
                };
                if t.item.is_dirty(cx) {
                    return None;
                }
                match relocate(&self.lists, kind, &file.path) {
                    Some((k, _)) if k == kind => None,
                    Some((k, f)) => Some((t.key.clone(), Some(selection_of(k, f)))),
                    None => Some((t.key.clone(), None)),
                }
            })
            .collect();
        self.retarget_later(&ws, moves, cx);
    }

    /// The branch review's tabs, after it was read again.
    pub(super) fn follow_review(&self, cx: &mut Context<Self>) {
        let Some(ws) = self.workspace() else { return };
        let Some((review, base)) = self
            .review
            .as_ref()
            .and_then(|r| Some((r, r.base.as_ref()?)))
        else {
            return;
        };
        let moves: Vec<(SharedString, Option<Selection>)> = ws
            .read(cx)
            .tabs()
            .tabs()
            .iter()
            .filter_map(|t| {
                let Selection::Branch {
                    base: was,
                    file,
                    fixed: false,
                    ..
                } = t.item.selection()?
                else {
                    return None;
                };
                match review.files.iter().find(|f| f.path == file.path) {
                    Some(_) if was == base => None,
                    Some(f) => Some((
                        t.key.clone(),
                        Some(Selection::Branch {
                            base: base.clone(),
                            label: review.label.clone(),
                            file: f.clone(),
                            fixed: false,
                        }),
                    )),
                    None => Some((t.key.clone(), None)),
                }
            })
            .collect();
        self.retarget_later(&ws, moves, cx);
    }

    fn retarget_later(
        &self,
        ws: &Entity<Workspace>,
        moves: Vec<(SharedString, Option<Selection>)>,
        cx: &mut Context<Self>,
    ) {
        if moves.is_empty() {
            return;
        }
        let (handle, ws) = (self.window, ws.downgrade());
        cx.defer(move |cx| {
            handle
                .update(cx, |_, window, cx| {
                    ws.update(cx, |ws, cx| retarget(ws, moves, window, cx)).ok();
                })
                .ok();
        });
    }
}

/// Each tab under a key opens its new selection in its place (or closes, for None); the
/// active tab stays the one it was.
fn retarget(
    ws: &mut Workspace,
    moves: Vec<(SharedString, Option<Selection>)>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    for (key, to) in moves {
        let Some(at) = ws.tabs().position(&key) else {
            continue;
        };
        let Some(sel) = to else {
            ws.close_tab(&key, cx);
            continue;
        };
        let new_key = SharedString::from(selection_key(&sel));
        // Open there already (both lists had it once): that tab stays, this one goes.
        if ws.tabs().get(&new_key).is_some() {
            ws.close_tab(&key, cx);
            continue;
        }
        let active = ws.tabs().active_key().cloned();
        let preview = ws.tabs().get(&key).is_some_and(|t| t.preview);
        // A preview open takes the preview tab's place, which is this one when it's preview.
        ws.open(sel, preview, window, cx);
        if ws.tabs().get(&key).is_some() {
            ws.close_tab(&key, cx);
        }
        if let Some(now) = ws.tabs().position(&new_key) {
            ws.move_tab(now, at.min(ws.tabs().len() - 1), cx);
        }
        if let Some(a) = active.filter(|a| *a != key) {
            ws.activate_tab(&a, window, cx);
        }
    }
}
