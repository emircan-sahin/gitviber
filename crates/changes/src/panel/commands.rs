//! The panel's steps from the palette, the menu bar and keys: Continue, Skip and Abort, Stash,
//! Pop, Commit & Push and Amend. Each does what its button does, and says why when it can't:
//! the same reason that disables or hides the button.

use super::*;
use crate::commit::Then;

/// A command run by the panel, from outside it.
pub(crate) type Act = fn(&mut ChangesPanel, &mut Window, &mut Context<ChangesPanel>);

/// A merge, rebase, cherry-pick, revert or am stopped for Continue and Abort; a bisect has its
/// own bar.
pub(crate) fn has_steps(kind: &str) -> bool {
    kind != "bisect"
}

/// Only a rebase skips a commit; a split's pieces are committed instead.
pub(crate) fn skips(kind: &str, split: bool) -> bool {
    kind == "rebase" && !split
}

/// "continue" as a toast's title starts it: "Continue".
pub(super) fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

impl ChangesPanel {
    /// `act` now, or once the first read is in: a panel built for the command lists nothing yet.
    /// `verb` names it in the toast if that read never comes.
    pub(crate) fn once_read(
        &mut self,
        verb: &'static str,
        act: Act,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.loaded {
            act(self, window, cx);
        } else {
            self.after_read.push((verb, act));
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
                        for (_, act) in acts {
                            act(this, window, cx);
                        }
                    })
                    .ok();
                })
                .ok();
        });
    }

    /// The commands waiting for a read that won't come, each told why.
    pub(super) fn drop_after_read(&mut self, why: &str, cx: &mut App) {
        for (verb, _) in std::mem::take(&mut self.after_read) {
            self.not_run(verb, why.into(), cx);
        }
    }

    fn not_run(&self, verb: &str, why: String, cx: &mut App) {
        self.toast(Toast::info(why).title(format!("{verb} not run")), cx);
    }

    /// The action this panel runs, or one on the repo: a git step now would race it.
    fn busy_reason(&self, cx: &App) -> Option<String> {
        if self.busy {
            return Some("Wait for the action running to finish.".into());
        }
        let running = gitviber_refs::running(self.ctx.project.as_ref()?, cx)?;
        Some(format!("Wait for {running} to finish."))
    }

    /// What stops the banner's `which` ("continue", "skip", "abort").
    pub(super) fn operation_blocked(&self, which: &str, cx: &App) -> Option<String> {
        let Some(op) = self.operation.as_ref().filter(|o| has_steps(&o.kind)) else {
            return Some("No merge or rebase is in progress.".into());
        };
        if let Some(why) = self.busy_reason(cx) {
            return Some(why);
        }
        let left = self.lists.conflicted.len();
        match which {
            "continue" if left > 0 => Some(format!(
                "Resolve the {left} {} left first.",
                if left == 1 { "conflict" } else { "conflicts" }
            )),
            "skip" if !skips(&op.kind, op.split) => Some("Only a rebase skips a commit.".into()),
            _ => None,
        }
    }

    pub(crate) fn operation_command(
        &mut self,
        which: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.operation_blocked(which, cx) {
            Some(why) => self.not_run(&capitalized(which), why, cx),
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
        match self.busy_reason(cx).or_else(|| self.stash_blocked()) {
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

    /// Apply or Pop, from a stash's row, its menu or the palette.
    pub(super) fn apply_stash_blocked(&self, pop: bool, cx: &mut App) -> bool {
        let Some(why) = self.busy_reason(cx) else {
            return false;
        };
        self.not_run(if pop { "Pop" } else { "Apply" }, why, cx);
        true
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
