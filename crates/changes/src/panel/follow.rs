//! Tabs follow git. The open row leaving its list (staged, unstaged, discarded here) hands over
//! to the row now in its place at once, without waiting for git, so S, S, S walks down the
//! list. After each read, a change tab whose file moved lists goes with it, in its place and
//! with nothing left to reopen, and one whose file is gone (committed, discarded) closes. Branch
//! review tabs follow the review the same way: onto the new merge base after a rebase or another
//! base, closed once their file is out of it. A tab with edits typed in stays as it is.

use super::*;
use crate::model::list::{relocate, row_in_place};

/// Rows one git action is taking out of their list.
pub(super) struct Leaving {
    id: u64,
    keys: Vec<String>,
    /// The row left and the one opened in its place, to go back to if the action fails.
    back: Option<(Selection, String)>,
    /// Done when this many reads had started: a read begun after it shows the rows gone. One
    /// already on its way may not, and a status read while it runs doesn't let them back.
    done: Option<u64>,
}

impl ChangesPanel {
    /// Before `rows` of `kind` leave their list, for the action `run_leaving` is given the
    /// number of. Not conflicts: a resolved one's tab follows it into Staged.
    pub(super) fn leave(
        &mut self,
        kind: Kind,
        rows: &[Arc<FileChange>],
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        if kind == Kind::Conflict {
            return None;
        }
        let keys: Vec<String> = rows
            .iter()
            .map(|f| selection_key(&selection_of(kind, f.clone())))
            .collect();
        let from = self.open_key.clone().filter(|k| keys.contains(k));
        self.leaves += 1;
        let id = self.leaves;
        self.leaving.push(Leaving {
            id,
            keys,
            back: None,
            done: None,
        });
        let from = from?;
        let prefix = format!("{}::", kind.name());
        let shown: Vec<String> = self
            .file_keys()
            .into_iter()
            .filter(|k| k.starts_with(&prefix))
            .collect();
        let to = row_in_place(&shown, |k| !self.is_leaving(k), &from)
            .and_then(|k| self.selection_at(&k));
        // Nothing left in its place: the tab follows the file instead (follow_git).
        let Some(to) = to else { return Some(id) };
        let back = self.selection_at(&from).map(|f| (f, selection_key(&to)));
        if let Some(l) = self.leaving.last_mut() {
            l.back = back;
        }
        self.picked = None;
        self.open_later(to, true, cx);
        Some(id)
    }

    pub(super) fn is_leaving(&self, key: &str) -> bool {
        self.leaving.iter().any(|l| l.keys.iter().any(|k| k == key))
    }

    /// The action `leaving` is done. Failed, its rows stay, and the tab goes back to the file it
    /// left, unless another row was opened meanwhile.
    pub(super) fn settle_leave(
        &mut self,
        leaving: Option<u64>,
        failed: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(at) = leaving.and_then(|id| self.leaving.iter().position(|l| l.id == id)) else {
            return;
        };
        if !failed {
            self.leaving[at].done = Some(self.reads);
            return;
        }
        let Some((from, to)) = self.leaving.remove(at).back else {
            return;
        };
        if self.open_key.as_deref() == Some(to.as_str()) {
            self.open_later(from, true, cx);
        }
    }

    /// Read `seq` landed: the rows of actions done before it began are out of the lists.
    pub(super) fn read_landed(&mut self, seq: u64) {
        self.leaving.retain(|l| l.done.is_none_or(|d| seq <= d));
    }

    /// The change tabs whose file is elsewhere in the lists just read, or gone.
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

/// Each tab under a key shows its new selection in its place, or closes for None; one with
/// edits typed in stays, which a new view would lose (`retarget_tab` refuses it).
fn retarget(
    ws: &mut Workspace,
    moves: Vec<(SharedString, Option<Selection>)>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    for (key, to) in moves {
        match to {
            Some(sel) => {
                ws.retarget_tab(&key, sel, window, cx);
            }
            None if ws.tabs().get(&key).is_some_and(|t| !t.item.is_dirty(cx)) => {
                ws.close_tab(&key, cx);
            }
            None => {}
        }
    }
}
