//! The panel's steps from the palette, the menu bar and keys: Continue, Skip and Abort, Stash,
//! Pop, Commit & Push and Amend. Each does what its button does, and says why when it can't:
//! the same reason that disables or hides the button.

use super::*;
use crate::commit::Then;

/// A command run by the panel, from outside it.
pub(crate) type Act = fn(&mut ChangesPanel, &mut Window, &mut Context<ChangesPanel>);

impl ChangesPanel {
    /// `act` now, or once the first read is in: a panel built for the command lists nothing yet.
    pub(crate) fn once_read(&mut self, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        if self.loaded {
            act(self, window, cx);
        } else {
            self.after_read.push(act);
        }
    }

    /// The commands that waited for the read that just landed.
    pub(super) fn run_after_read(&mut self, cx: &mut Context<Self>) {
        let acts = std::mem::take(&mut self.after_read);
        if acts.is_empty() {
            return;
        }
        let (handle, weak) = (self.window, cx.weak_entity());
        cx.defer(move |cx| {
            handle
                .update(cx, |_, window, cx| {
                    weak.update(cx, |this, cx| {
                        for act in acts {
                            act(this, window, cx);
                        }
                    })
                    .ok();
                })
                .ok();
        });
    }

    fn not_run(&self, verb: &str, why: String, cx: &mut App) {
        self.toast(Toast::info(why).title(format!("{verb} not run")), cx);
    }

    /// What stops the banner's `which` ("continue", "skip", "abort").
    pub(super) fn operation_blocked(&self, which: &str) -> Option<String> {
        let Some(op) = self.operation.as_ref().filter(|o| o.kind != "bisect") else {
            return Some("No merge or rebase is in progress.".into());
        };
        if self.busy {
            return Some("Wait for the action running to finish.".into());
        }
        let left = self.lists.conflicted.len();
        match which {
            "continue" if left > 0 => Some(format!(
                "Resolve the {left} {} left first.",
                if left == 1 { "conflict" } else { "conflicts" }
            )),
            "skip" if op.kind != "rebase" || op.split => {
                Some("Only a rebase skips a commit.".into())
            }
            _ => None,
        }
    }

    pub(crate) fn operation_command(
        &mut self,
        which: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.operation_blocked(which) {
            Some(why) => {
                let verb = which[..1].to_uppercase() + &which[1..];
                self.not_run(&verb, why, cx)
            }
            None => self.operation(which, window, cx),
        }
    }

    /// What stops stashing every change; the Stashes section's button shows only without it.
    pub(super) fn stash_blocked(&self) -> Option<String> {
        if let Some(op) = &self.operation {
            return Some(format!("Finish or abort the {} first.", op.kind));
        }
        (self.lists.total() == 0).then(|| "There are no changes to stash.".into())
    }

    pub(crate) fn stash_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.stash_blocked() {
            Some(why) => self.not_run("Stash", why, cx),
            None => self.stash_dialog(vec![], window, cx),
        }
    }

    pub(crate) fn pop_latest_stash(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        match self.stashes.first() {
            Some(s) => self.apply_stash(s.sha.clone(), true, cx),
            None => self.not_run("Pop", "There are no stashes.".into(), cx),
        }
    }

    pub(crate) fn commit_and_push(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit
            .update(cx, |b, cx| b.commit(Some(Then::Push), window, cx));
    }

    pub(crate) fn toggle_amend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let on = !self.commit.read(cx).amending();
        self.commit
            .update(cx, |b, cx| b.toggle_amend(on, window, cx));
    }
}
