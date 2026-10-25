//! The message being written and the commit: summary and description, Amend (which swaps in
//! HEAD's message and keeps the draft aside), co-authors, sign-off, skipping hooks once, and ✦
//! asking the user's own agent CLI for a message. Nothing staged commits everything. The commit
//! runs as the top bar's activity, so hooks that lint or test for minutes can be cancelled.

use crate::co_authors::CoAuthorPicker;
use crate::model::commit::{Facts, SUMMARY_LIMIT, plan};
use crate::model::list::{files, left_out, stageable};
use crate::model::text::{
    SUGGEST_PROMPT, author_name, command_line, parse_suggestion, paste_message, plain_line,
    program_of,
};
use crate::model::{Kind, Lists};
use crate::store::{self, Draft};
use crate::toasts::{git_error, tracked, with_undo};
use gitviber_core::api;
use gitviber_core::git::{CANCELLED_STASHED, CommitOptions, HeadMessage};
use gitviber_core::network::CANCELLED;
use gitviber_core::suggest::{Kind as SuggestKind, Scope};
use gitviber_refs::errors::GitFix;
use gitviber_refs::net::NetActivity;
use gitviber_refs::toast::Action as ToastAction;
use gitviber_refs::{RepoHold, hold_repo};
use gitviber_review::model::agent::is_unknown_option;
use gitviber_ui::actions::git;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{Toast, Workspace};
use gitviber_ui::{ActiveAppTheme as _, Settings};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{
    Enter, Input, InputEvent, InputState, Paste, Textarea, TextareaState,
};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::{
    Anchor, App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

/// While amending: HEAD's message in the fields, the user's own draft aside.
struct Amend {
    aside: Draft,
    sha: String,
    original: (String, String),
}

/// A commit as it started: where, and what it was told to do after.
pub(crate) struct Started {
    /// The worktree core had open, which the commit is made in.
    pub repo: Option<PathBuf>,
    /// Its draft's key.
    pub root: String,
    pub summary: String,
    pub then: Option<Then>,
    /// Nested repositories "commit everything" left out.
    pub skipped: usize,
    pub amended: bool,
}

/// A message nobody wrote in: blank, or still the `start` it began as.
fn untouched(start: &(String, String), summary: &str, body: &str) -> bool {
    (summary.trim().is_empty() && body.trim().is_empty()) || (summary == start.0 && body == start.1)
}

/// What the panel tells the box about the repo.
#[derive(Clone, Default)]
pub struct RepoFacts {
    pub root: String,
    /// The main worktree: sign-off is kept per repository, across its worktrees.
    pub main: String,
    pub branch: Option<String>,
    pub head: Option<Arc<HeadMessage>>,
    pub lists: Lists,
    /// Set while git has left a message for the next commit (a squash merge, `cherry-pick -n`).
    pub prepared: Option<String>,
    pub pushed: bool,
    /// What the list's filter leaves, while it has text.
    pub shown: Option<Lists>,
}

pub struct CommitBox {
    workspace: WeakEntity<Workspace>,
    facts: RepoFacts,
    summary: Entity<InputState>,
    body: Entity<TextareaState>,
    co_authors: Vec<String>,
    amend: Option<Amend>,
    no_verify: bool,
    /// While a commit runs: the window's repo stays put until it ends (its push is for it).
    busy: Option<RepoHold>,
    suggesting: bool,
    /// The summary and description a new draft starts as: a prepared message, or none and
    /// commit.template. A draft still as it started isn't the user's, and follows it.
    start: (String, String),
    /// Add co-author's picker, while it's open.
    co_author: Option<Entity<CoAuthorPicker>>,
    _subscriptions: Vec<Subscription>,
    _task: Task<()>,
    _suggest: Task<()>,
    _template: Task<()>,
    _live: Live,
}

impl CommitBox {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let summary = cx.new(|cx| InputState::new(window, cx).placeholder("Summary"));
        let body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Description")
                .rows(2)
        });
        let subscriptions = vec![
            cx.subscribe(&summary, |this, _, e: &InputEvent, cx| this.edited(e, cx)),
            cx.subscribe(&body, |this, _, e: &InputEvent, cx| this.edited(e, cx)),
        ];
        CommitBox {
            workspace,
            facts: RepoFacts::default(),
            summary,
            body,
            co_authors: vec![],
            amend: None,
            no_verify: false,
            busy: None,
            suggesting: false,
            start: Default::default(),
            co_author: None,
            _subscriptions: subscriptions,
            _task: Task::ready(()),
            _suggest: Task::ready(()),
            _template: Task::ready(()),
            _live: Live::new("CommitBox"),
        }
    }

    fn edited(&mut self, e: &InputEvent, cx: &mut Context<Self>) {
        if matches!(e, InputEvent::Change) {
            self.keep_draft(cx);
            cx.notify();
        }
    }

    pub(crate) fn summary_text(&self, cx: &App) -> String {
        self.summary.read(cx).value().to_string()
    }

    pub(crate) fn body_text(&self, cx: &App) -> String {
        self.body.read(cx).value().to_string()
    }

    fn draft(&self, cx: &App) -> Draft {
        Draft {
            summary: self.summary_text(cx),
            body: self.body_text(cx),
            co_authors: self.co_authors.clone(),
        }
    }

    /// The draft outlives the box and the app, per worktree; an amend message isn't one, nor
    /// a message as it started (empty, the prepared one or commit.template).
    fn keep_draft(&self, cx: &mut Context<Self>) {
        if self.amend.is_some() || self.facts.root.is_empty() {
            return;
        }
        let draft = self.draft(cx);
        let untouched =
            untouched(&self.start, &draft.summary, &draft.body) && draft.co_authors.is_empty();
        store::set_draft(&self.facts.root, (!untouched).then_some(draft), cx);
    }

    fn set_fields(
        &mut self,
        summary: &str,
        body: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (summary, body) = (summary.to_string(), body.to_string());
        self.summary
            .update(cx, |s, cx| s.set_value(summary, window, cx));
        self.body.update(cx, |s, cx| s.set_value(body, window, cx));
        // set_value says nothing of the change: a suggestion or an amend's draft coming back
        // is kept as typing is.
        self.keep_draft(cx);
    }

    /// The panel's news: what's staged and changed, HEAD, the branch.
    pub fn set_facts(&mut self, facts: RepoFacts, window: &mut Window, cx: &mut Context<Self>) {
        let new_root = facts.root != self.facts.root;
        let prepared_changed = facts.prepared != self.facts.prepared;
        self.facts = facts;
        if new_root {
            let draft = store::draft(&self.facts.root, cx).unwrap_or_default();
            self.co_authors = draft.co_authors.clone();
            self.amend = None;
            self.set_fields(&draft.summary, &draft.body, window, cx);
        }
        if new_root || prepared_changed {
            self.load_template(window, cx);
        }
        // An agent committing meanwhile moves HEAD; an untouched amend message follows it.
        if let (Some(amend), Some(head)) = (&self.amend, self.facts.head.clone())
            && head.sha != amend.sha
            && (self.summary_text(cx), self.body_text(cx)) == amend.original
        {
            self.amend = Some(Amend {
                aside: amend.aside.clone(),
                sha: head.sha.clone(),
                original: (head.subject.clone(), head.body.clone()),
            });
            self.set_fields(&head.subject, &head.body, window, cx);
        }
        cx.notify();
    }

    /// An empty message starts as git's editor would: with a prepared message, else
    /// commit.template in the description.
    fn load_template(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let core = crate::core(cx);
        let prepared = self.facts.prepared.is_some();
        self._template = cx.spawn_in(window, async move |this, cx| {
            let template = cx
                .background_spawn(async move { api::changes::commit_template(&core) })
                .await
                .ok()
                .flatten();
            this.update_in(cx, |this, window, cx| {
                let start = match (&template, prepared) {
                    (Some(t), true) => {
                        let (s, b) = t.split_once('\n').unwrap_or((t, ""));
                        (s.to_string(), b.trim().to_string())
                    }
                    (t, false) => (String::new(), t.clone().unwrap_or_default()),
                    (None, true) => (String::new(), String::new()),
                };
                // A message still as it started follows: an aborted squash's goes with it.
                let old = std::mem::replace(&mut this.start, start.clone());
                if let Some(a) = this.amend.as_mut() {
                    if a.aside.co_authors.is_empty()
                        && untouched(&old, &a.aside.summary, &a.aside.body)
                    {
                        (a.aside.summary, a.aside.body) = start;
                    }
                } else if untouched(&old, &this.summary_text(cx), &this.body_text(cx)) {
                    this.set_fields(&start.0, &start.1, window, cx);
                }
            })
            .ok();
        });
    }

    /// A multi-line paste into Summary: its first line there, the rest into the description,
    /// as git reads a message. One line pastes as usual.
    fn paste_summary(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else {
            return;
        };
        let range = self.summary.read(cx).selected_range();
        let Some((summary, body, caret)) = paste_message(
            &self.summary_text(cx),
            &self.body_text(cx),
            &self.start.1,
            &text,
            range.start,
            range.end,
        ) else {
            return;
        };
        cx.stop_propagation();
        self.set_fields(&summary, &body, window, cx);
        self.summary
            .update(cx, |s, cx| s.set_selected_range(caret..caret, cx));
        cx.notify();
    }

    fn open_co_author(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        let on_add: crate::co_authors::OnAdd = std::rc::Rc::new(move |author, _, cx| {
            weak.update(cx, |this, cx| {
                this.co_authors.push(author);
                this.co_author = None;
                this.keep_draft(cx);
                cx.notify();
            })
            .ok();
        });
        let taken = self.co_authors.clone();
        self.co_author = Some(cx.new(|cx| CoAuthorPicker::new(taken, on_add, window, cx)));
        cx.notify();
    }

    /// The files the commit takes that the list's filter hides.
    pub(crate) fn hidden(&self) -> usize {
        let Some(shown) = &self.facts.shown else {
            return 0;
        };
        let lists = &self.facts.lists;
        let committed: Vec<String> = if !lists.staged.is_empty() || self.amend.is_some() {
            lists.staged.iter().map(|f| f.path.clone()).collect()
        } else {
            stageable(&lists.unstaged).0
        };
        let visible: HashSet<&str> = shown
            .staged
            .iter()
            .chain(&shown.unstaged)
            .map(|f| f.path.as_str())
            .collect();
        committed
            .iter()
            .filter(|p| !visible.contains(p.as_str()))
            .count()
    }

    fn toggle_amend(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.drop_suggestion(cx);
        if on {
            let Some(head) = self.facts.head.clone() else {
                return;
            };
            let aside = self.draft(cx);
            self.amend = Some(Amend {
                aside,
                sha: head.sha.clone(),
                original: (head.subject.clone(), head.body.clone()),
            });
            self.set_fields(&head.subject, &head.body, window, cx);
        } else if let Some(amend) = self.amend.take() {
            self.co_authors = amend.aside.co_authors.clone();
            self.set_fields(&amend.aside.summary, &amend.aside.body, window, cx);
        }
        cx.notify();
    }

    fn edited_amend(&self, cx: &App) -> bool {
        self.amend
            .as_ref()
            .is_some_and(|a| (self.summary_text(cx), self.body_text(cx)) != a.original)
    }

    fn sign_off(&self, cx: &App) -> bool {
        Settings::get(cx).sign_off_repos.contains(&self.facts.main)
    }

    fn set_sign_off(&self, on: bool, cx: &mut App) {
        let main = self.facts.main.clone();
        Settings::update(cx, |s| {
            s.sign_off_repos.retain(|r| *r != main);
            if on {
                s.sign_off_repos.push(main);
            }
        });
    }

    fn facts_for_plan(&self, cx: &App) -> Facts<'_> {
        let (paths, nested) = stageable(self.facts.lists.list(Kind::Unstaged));
        Facts {
            staged: self.facts.lists.staged.len(),
            stageable: paths.len(),
            nested,
            conflicts: self.facts.lists.conflicted.len(),
            amend: self.amend.is_some(),
            edited: self.edited_amend(cx),
            summary_empty: self.summary_text(cx).trim().is_empty(),
            busy: self.busy.is_some(),
            branch: self.facts.branch.as_deref(),
        }
    }

    fn toast(&self, toast: Toast, cx: &mut App) {
        if let Some(ws) = self.workspace.upgrade() {
            ws.update(cx, |ws, cx| ws.toast(toast, cx));
        }
    }

    fn refresh_project(&self, cx: &mut App) {
        if let Some(project) = self
            .workspace
            .upgrade()
            .and_then(|ws| ws.read(cx).project().cloned())
        {
            project.update(cx, |p, cx| p.repo_changed(cx));
        }
    }

    /// `then`: push or sync right after, as the top bar's commands do.
    pub fn commit(&mut self, then: Option<Then>, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_with(then, false, window, cx);
    }

    /// `skip_hooks`: this once, from a failed commit's toast.
    fn commit_with(
        &mut self,
        then: Option<Then>,
        skip_hooks: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let plan = plan(self.facts_for_plan(cx));
        if plan.blocked.is_some() {
            return;
        }
        self.drop_suggestion(cx);
        // Hooks can run for minutes: the worktree stays open until the commit (and its push) is done.
        self.busy = Some(hold_repo("Commit", cx));
        cx.notify();
        let summary = self.summary_text(cx).trim().to_string();
        let body = self.body_text(cx).trim().to_string();
        // An untouched amend message goes as none, so git keeps the original exactly.
        let message = if self.amend.is_some() && !self.edited_amend(cx) {
            String::new()
        } else if body.is_empty() {
            summary.clone()
        } else {
            format!("{summary}\n\n{body}")
        };
        let options = CommitOptions {
            amend: self.amend.is_some(),
            sign_off: self.sign_off(cx),
            no_verify: self.no_verify || skip_hooks,
            co_authors: self.co_authors.clone(),
        };
        let (paths, skipped) = stageable(self.facts.lists.list(Kind::Unstaged));
        let auto = plan.stages_all;
        let core = crate::core(cx);
        let started = Started {
            repo: core.repo().ok(),
            root: self.facts.root.clone(),
            summary,
            then,
            skipped,
            amended: options.amend,
        };
        self._task = cx.spawn_in(window, async move |this, cx| {
            // Nothing staged means "commit everything", the common case after an agent run.
            let checked = {
                let core = core.clone();
                let paths = paths.clone();
                cx.background_spawn(async move {
                    if auto {
                        api::changes::stage(&core, paths, false)?;
                    }
                    api::changes::large_staged(&core)
                })
                .await
            };
            let large = match checked {
                Ok(large) => large,
                Err(e) => {
                    this.update(cx, |this, cx| this.failed(&e, then, cx)).ok();
                    return;
                }
            };
            if !large.is_empty() {
                let text = large_files_text(&large, auto);
                let weak = this.clone();
                let (paths, message, options) = (paths.clone(), message.clone(), options);
                this.update_in(cx, move |_, window, cx| {
                    let go = std::rc::Rc::new(std::cell::RefCell::new(Some((
                        message, options, started,
                    ))));
                    let (go_ok, weak_ok, weak_cancel) = (go.clone(), weak.clone(), weak.clone());
                    window.open_alert_dialog(cx, move |alert, _, _| {
                        let (go_ok, weak_ok, weak_cancel, paths) = (
                            go_ok.clone(),
                            weak_ok.clone(),
                            weak_cancel.clone(),
                            paths.clone(),
                        );
                        alert
                            .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                            .title("Large files")
                            .description(text.clone())
                            .ok_text("Commit anyway")
                            .show_cancel(true)
                            .on_ok(move |_, window, cx| {
                                if let Some((message, options, started)) = go_ok.borrow_mut().take()
                                {
                                    weak_ok
                                        .update(cx, |this, cx| {
                                            this.run_commit(message, options, started, window, cx)
                                        })
                                        .ok();
                                }
                                true
                            })
                            .on_cancel(move |_, _, cx| {
                                let paths = paths.clone();
                                weak_cancel
                                    .update(cx, |this, cx| {
                                        this.busy = None;
                                        // Nothing was staged before, so unstaging what this staged puts the index back.
                                        if auto {
                                            let core = crate::core(cx);
                                            cx.background_spawn(async move {
                                                api::changes::unstage(&core, paths)
                                            })
                                            .detach();
                                        }
                                        this.toast(Toast::info("Commit cancelled"), cx);
                                        this.refresh_project(cx);
                                        cx.notify();
                                    })
                                    .ok();
                                true
                            })
                    });
                })
                .ok();
                return;
            }
            this.update_in(cx, |this, window, cx| {
                this.run_commit(message, options, started, window, cx)
            })
            .ok();
        });
    }

    fn run_commit(
        &mut self,
        message: String,
        options: CommitOptions,
        started: Started,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let core = crate::core(cx);
        // Core opened another worktree while the large files were asked about.
        if core.repo().ok() != started.repo {
            self.busy = None;
            self.toast(
                Toast::info("Another worktree was opened while it waited.").title("Commit not run"),
                cx,
            );
            cx.notify();
            return;
        }
        // Hooks can lint or test for minutes: the top bar shows the commit, with Cancel.
        let net = NetActivity::global(cx);
        let op = net.update(cx, |a, cx| a.start("Commit".into(), cx));
        let (id, progress) = (op.id.clone(), op.progress());
        let work = cx.background_spawn(async move {
            tracked(&core, |core| {
                api::changes::commit(core, message, options, id, progress)
            })
            .map(|((), entry)| entry)
        });
        // Detached, holding the box weakly: dropping it wouldn't stop git, only leave the top bar
        // showing a commit that ended.
        cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            net.update(cx, |a, cx| a.finish(&op.id, cx));
            this.update_in(cx, |this, window, cx| {
                this.committed(result, started, window, cx)
            })
            .ok();
        })
        .detach();
    }

    /// The commit ended. It stays with the worktree it was made in: one opened meanwhile (a path
    /// past the switch guard) keeps its own message and draft, and isn't pushed.
    pub(crate) fn committed(
        &mut self,
        result: Result<Option<u64>, String>,
        started: Started,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.busy = None;
        let entry = match result {
            Ok(entry) => entry,
            Err(e) => return self.failed(&e, started.then, cx),
        };
        let here = crate::core(cx).repo().ok() == started.repo;
        if self.facts.root == started.root {
            self.clear(window, cx);
        } else if !started.amended {
            store::set_draft(&started.root, None, cx);
        }
        self.no_verify = false;
        let title = if started.amended {
            "Commit amended"
        } else {
            "Committed"
        };
        let done = Toast::success(started.summary.clone()).title(title);
        let repo = started.repo.clone().filter(|_| here);
        self.toast(with_undo(done, entry, repo, self.workspace.clone()), cx);
        if started.skipped > 0 && !started.amended {
            self.toast(
                Toast::info(crate::model::list::NESTED_EXPLAINED)
                    .title(format!("{} of the commit", left_out(started.skipped))),
                cx,
            );
        }
        match started.then {
            Some(_) if !here => self.toast(
                Toast::info("Another worktree was opened while the commit ran.")
                    .title("Not pushed"),
                cx,
            ),
            Some(Then::Push) => window.dispatch_action(Box::new(git::Push), cx),
            Some(Then::Sync) => window.dispatch_action(Box::new(git::Sync), cx),
            None => {}
        }
        self.refresh_project(cx);
        cx.notify();
    }

    /// A cancel is told as one; a hook that stopped it offers committing without hooks.
    fn failed(&mut self, e: &str, then: Option<Then>, cx: &mut Context<Self>) {
        self.busy = None;
        match e {
            CANCELLED => self.toast(Toast::info("Commit cancelled"), cx),
            CANCELLED_STASHED => self.toast(
                Toast::info(
                    "Hooks may have left changes in a stash: check Stashes before you commit again.",
                )
                .title("Commit cancelled"),
                cx,
            ),
            _ => {
                // The toast outlives this commit: its button commits what the box holds then.
                let (weak, root) = (cx.weak_entity(), self.facts.root.clone());
                let without = ToastAction::new("Commit without hooks", move |window, cx| {
                    weak.update(cx, |this, cx| {
                        if this.facts.root == root {
                            this.commit_with(then, true, window, cx);
                        } else {
                            this.toast(
                                Toast::info("It was for the repository open before this one.")
                                    .title("Commit without hooks not run"),
                                cx,
                            );
                        }
                    })
                    .ok();
                });
                let toast = git_error("Commit failed", e, &vec![(GitFix::Hooks, without)]);
                self.toast(toast, cx);
                // git has no name or email for the commit: the app asks for them, even after a "Not now".
                if no_identity(e) {
                    cx.dispatch_action(&gitviber_ui::keymap::requests::AskGitIdentity);
                }
            }
        }
        self.refresh_project(cx);
        cx.notify();
    }

    /// After a commit the fields start over; an amend's draft set aside comes back. A prepared
    /// message went with the commit: what starts the next one comes with the next read.
    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.amend.take().map(|a| a.aside).unwrap_or_else(|| {
            let (summary, body) = if self.facts.prepared.is_some() {
                Default::default()
            } else {
                self.start.clone()
            };
            Draft {
                summary,
                body,
                ..Default::default()
            }
        });
        self.co_authors = next.co_authors.clone();
        self.set_fields(&next.summary, &next.body, window, cx);
    }

    fn can_suggest(&self, cx: &App) -> bool {
        let s = Settings::get(cx);
        s.suggest_enabled
            && !self.suggesting
            && self.busy.is_none()
            && (self.amend.is_some()
                || self.facts.lists.staged.len() + self.facts.lists.unstaged.len() > 0)
    }

    pub fn suggest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_suggest(cx) {
            return;
        }
        let s = Settings::get(cx);
        let line = command_line(&s.suggest_command, &s.suggest_models, &s.suggest_efforts);
        let plain = plain_line(&s.suggest_command, &s.suggest_models, &s.suggest_efforts);
        let program = program_of(&s.suggest_command);
        let scope = if self.amend.is_some() {
            Scope::Amend
        } else if !self.facts.lists.staged.is_empty() {
            Scope::Staged
        } else {
            Scope::All
        };
        let core = crate::core(cx);
        self.suggesting = true;
        cx.notify();
        self._suggest = cx.spawn_in(window, async move |this, cx| {
            let out = cx
                .background_spawn(async move {
                    let ask = |line| {
                        api::changes::suggest_message(&core, line, SUGGEST_PROMPT.into(), scope)
                    };
                    match (ask(line), plain) {
                        // A Claude Code older than the lean flags refuses them: once more
                        // without, as a guided review does.
                        (Err(e), Some(plain)) if is_unknown_option(&e) => ask(plain),
                        (out, _) => out,
                    }
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.suggesting = false;
                match out.map(|o| parse_suggestion(&o)) {
                    Ok(Some((summary, body))) => {
                        let blank =
                            untouched(&this.start, &this.summary_text(cx), &this.body_text(cx));
                        this.set_fields(&summary, &body, window, cx);
                        if !blank {
                            this.toast(
                                Toast::info(
                                    "What you had is replaced; ⌘Z in the fields brings it back.",
                                )
                                .title("Message replaced with the suggestion"),
                                cx,
                            );
                        }
                    }
                    Ok(None) => this.toast(
                        Toast::error(format!("{program} printed nothing."))
                            .title("No message suggested"),
                        cx,
                    ),
                    // A cancel says so in its error; nothing to tell.
                    Err(e) if e.contains("cancel") => {}
                    Err(e) => this.toast(Toast::error(e).title("Couldn't suggest a message"), cx),
                }
                cx.notify();
            })
            .ok();
        });
    }

    fn cancel_suggestion(&mut self, cx: &mut Context<Self>) {
        let core = crate::core(cx);
        cx.background_spawn(
            async move { api::changes::suggest_cancel(&core, SuggestKind::Message) },
        )
        .detach();
    }

    /// A commit or an Amend toggle drops a suggestion still on its way.
    fn drop_suggestion(&mut self, cx: &mut Context<Self>) {
        if self.suggesting {
            self._suggest = Task::ready(());
            self.suggesting = false;
            self.cancel_suggestion(cx);
        }
    }

    #[cfg(test)]
    pub(crate) fn committing(&self) -> bool {
        self.busy.is_some()
    }

    #[cfg(test)]
    pub(crate) fn facts(&self) -> &RepoFacts {
        &self.facts
    }

    /// Writes the message, as if typed (tests).
    #[cfg(test)]
    pub(crate) fn write_message(
        &mut self,
        summary: &str,
        body: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_fields(summary, body, window, cx);
    }

    /// The cursor into the description at `line`, `column` (tests).
    #[cfg(test)]
    pub(crate) fn put_cursor_in_body(
        &self,
        line: u32,
        column: u32,
        window: &mut Window,
        cx: &mut App,
    ) {
        let at = gpui_kit::component::input::Position::new(line, column);
        self.body
            .update(cx, |s, cx| s.set_cursor_position(at, window, cx));
    }

    #[cfg(test)]
    pub(crate) fn message_focused(&self, window: &Window, cx: &App) -> bool {
        use gpui_kit::Focusable as _;
        self.summary.read(cx).focus_handle(cx).is_focused(window)
    }

    pub fn focus_message(&self, window: &mut Window, cx: &mut App) {
        self.summary.update(cx, |s, cx| s.focus(window, cx));
    }
}

type OnRemove = Box<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Then {
    Push,
    Sync,
}

impl Render for CommitBox {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let plan = plan(self.facts_for_plan(cx));
        let length = self.summary_text(cx).chars().count();
        let sign_off = self.sign_off(cx);
        let suggest_on = Settings::get(cx).suggest_enabled;
        let program = program_of(&Settings::get(cx).suggest_command);
        let pushed = self.amend.is_some() && self.facts.pushed;
        let can = plan.blocked.is_none();
        let hidden = self.hidden();
        let skipped = plan.stages_all && stageable(&self.facts.lists.unstaged).1 > 0;
        let tip: SharedString = match plan.blocked {
            Some(b) => b.to_string(),
            None => {
                let mut parts = vec![plan.target.clone()];
                if skipped {
                    let n = stageable(&self.facts.lists.unstaged).1;
                    parts.push(left_out(n).to_lowercase());
                }
                if hidden > 0 {
                    parts.push(format!("including {} the filter hides", files(hidden)));
                }
                parts.join(", ")
            }
        }
        .into();
        let label: SharedString = if self.busy.is_some() {
            "Committing…".into()
        } else if hidden > 0 {
            format!("{} · {hidden} hidden", plan.label).into()
        } else {
            plan.label.into()
        };
        let weak = cx.entity().downgrade();
        // A commit option that's on, shown so it isn't forgotten; × turns it off.
        let chip = |id: SharedString,
                    label: String,
                    tip: SharedString,
                    warn: bool,
                    on_remove: OnRemove| {
            h_flex()
                .id(id.clone())
                .gap_1()
                .px_1p5()
                .rounded_sm()
                .bg(if warn {
                    t.modified.opacity(0.15)
                } else {
                    t.elevated
                })
                .text_size(px(11.))
                .text_color(if warn { t.modified } else { t.muted_foreground })
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
                .child(div().child(label))
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-remove")))
                        .cursor_pointer()
                        .child("×")
                        .on_click(move |_, window, cx| on_remove(window, cx)),
                )
        };
        let options_weak = weak.clone();
        let picker = self.co_author.clone();
        let co_weak = weak.clone();
        v_flex()
            .key_context("CommitMessage")
            // ⌘↵ in either field commits. Taken before the field sees it: the description's own
            // Enter would put a newline at the cursor first, into the message.
            .capture_action(cx.listener(|this, e: &Enter, window, cx| {
                if e.secondary {
                    cx.stop_propagation();
                    this.commit(None, window, cx);
                }
            }))
            .flex_none()
            .gap_1p5()
            .p_2()
            .border_t_1()
            .border_color(t.border)
            .bg(t.chrome(t.panel))
            .child(
                div()
                    .relative()
                    .capture_action(
                        cx.listener(|this, _: &Paste, window, cx| this.paste_summary(window, cx)),
                    )
                    .child(Input::new(&self.summary).small())
                    .when(length > 50, |el| {
                        el.child(
                            div()
                                .absolute()
                                .top(px(6.))
                                .right_2()
                                .text_size(px(10.5))
                                .text_color(if length > SUMMARY_LIMIT {
                                    t.modified
                                } else {
                                    t.subtle
                                })
                                .child(length.to_string()),
                        )
                    }),
            )
            .child(Textarea::new(&self.body).small())
            .when(
                self.no_verify || sign_off || !self.co_authors.is_empty(),
                |el| {
                    let w1 = weak.clone();
                    let w2 = weak.clone();
                    el.child(
                        h_flex()
                            .flex_wrap()
                            .gap_1()
                            .when(self.no_verify, |el| {
                                el.child(chip(
                                    "chip-hooks".into(),
                                    "Skip hooks".into(),
                                    "The pre-commit and commit-msg hooks won't run for this commit"
                                        .into(),
                                    true,
                                    Box::new(move |_, cx| {
                                        w1.update(cx, |this, cx| {
                                            this.no_verify = false;
                                            cx.notify();
                                        })
                                        .ok();
                                    }),
                                ))
                            })
                            .when(sign_off, |el| {
                                el.child(chip(
                                    "chip-signoff".into(),
                                    "Sign off".into(),
                                    "Adds Signed-off-by with your name, on every commit in this repository"
                                        .into(),
                                    false,
                                    Box::new(move |_, cx| {
                                        w2.update(cx, |this, cx| this.set_sign_off(false, cx)).ok();
                                    }),
                                ))
                            })
                            .children(self.co_authors.iter().enumerate().map(|(i, a)| {
                                let (w, a2) = (weak.clone(), a.clone());
                                chip(
                                    format!("chip-co-author-{i}").into(),
                                    author_name(a).to_string(),
                                    format!("Co-authored-by: {a}").into(),
                                    false,
                                    Box::new(move |_, cx| {
                                        let a = a2.clone();
                                        w.update(cx, |this, cx| {
                                            this.co_authors.retain(|x| *x != a);
                                            this.keep_draft(cx);
                                            cx.notify();
                                        })
                                        .ok();
                                    }),
                                )
                            })),
                    )
                },
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        gpui_kit::component::checkbox::Checkbox::new("amend")
                            .label("Amend")
                            .checked(self.amend.is_some())
                            .on_click(cx.listener(|this, on: &bool, window, cx| {
                                this.toggle_amend(*on, window, cx)
                            })),
                    )
                    .when(suggest_on, |el| {
                        el.child(
                            Button::new("suggest")
                                .ghost()
                                .xsmall()
                                .icon(if self.suggesting {
                                    Lucide::LoaderCircle
                                } else {
                                    Lucide::Sparkles
                                })
                                .tooltip(SharedString::from(if self.suggesting {
                                    format!("Stop {program}")
                                } else {
                                    format!("Suggest a message with {program}")
                                }))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    if this.suggesting {
                                        this.drop_suggestion(cx);
                                        cx.notify();
                                    } else {
                                        this.suggest(window, cx);
                                    }
                                })),
                        )
                    })
                    .child(
                        // Above the button and over the panel's width: Changes sits at the left.
                        Popover::new("co-author-picker")
                            .anchor(Anchor::BottomLeft)
                            .open(picker.is_some())
                            .when_some(picker.as_ref(), |p, picker| {
                                p.track_focus(&picker.focus_handle(cx))
                            })
                            .on_open_change(move |open: &bool, window, cx| {
                                co_weak
                                    .update(cx, |this, cx| {
                                        if !*open {
                                            this.co_author = None;
                                            cx.notify();
                                        } else if this.co_author.is_none() {
                                            this.open_co_author(window, cx);
                                        }
                                    })
                                    .ok();
                            })
                            .trigger(
                                Button::new("co-author")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::UserPlus)
                                    .tooltip("Add co-author"),
                            )
                            .content(move |_, _, _| match &picker {
                                Some(p) => p.clone().into_any_element(),
                                None => div().into_any_element(),
                            }),
                    )
                    .child(
                        Button::new("commit-options")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Ellipsis)
                            .tooltip("Commit options")
                            .dropdown_menu(move |menu, _, cx| {
                                let Some(this) = options_weak.upgrade() else {
                                    return menu;
                                };
                                let (label, can, sign_off, no_verify) = {
                                    let b = this.read(cx);
                                    let p = crate::model::commit::plan(b.facts_for_plan(cx));
                                    (p.label, p.blocked.is_none(), b.sign_off(cx), b.no_verify)
                                };
                                let (w0, w1, w2, w3, w4) = (
                                    options_weak.clone(),
                                    options_weak.clone(),
                                    options_weak.clone(),
                                    options_weak.clone(),
                                    options_weak.clone(),
                                );
                                menu.item(PopupMenuItem::new("Add co-author…").on_click(
                                    move |_, window, cx| {
                                        w0.update(cx, |this, cx| this.open_co_author(window, cx))
                                            .ok();
                                    },
                                ))
                                .separator()
                                .item(
                                    PopupMenuItem::new(format!("{label} & Push"))
                                        .disabled(!can)
                                        .on_click(move |_, window, cx| {
                                            w1.update(cx, |this, cx| {
                                                this.commit(Some(Then::Push), window, cx)
                                            })
                                            .ok();
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new(format!("{label} & Sync"))
                                        .disabled(!can)
                                        .on_click(move |_, window, cx| {
                                            w2.update(cx, |this, cx| {
                                                this.commit(Some(Then::Sync), window, cx)
                                            })
                                            .ok();
                                        }),
                                )
                                .separator()
                                .item(
                                    PopupMenuItem::new("Sign off (this repo)")
                                        .checked(sign_off)
                                        .on_click(move |_, _, cx| {
                                            w3.update(cx, |this, cx| {
                                                this.set_sign_off(!sign_off, cx)
                                            })
                                            .ok();
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new("Skip hooks (this commit)")
                                        .checked(no_verify)
                                        .on_click(move |_, _, cx| {
                                            w4.update(cx, |this, cx| {
                                                this.no_verify = !no_verify;
                                                cx.notify();
                                            })
                                            .ok();
                                        }),
                                )
                            }),
                    )
                    .child(
                        Button::new("commit")
                            .primary()
                            .small()
                            .flex_1()
                            .label(label)
                            .disabled(!can)
                            .tooltip(tip)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.commit(None, window, cx)),
                            ),
                    ),
            )
            .when(self.suggesting, |el| {
                el.child(
                    div()
                        .text_size(px(11.))
                        .text_color(t.muted_foreground)
                        .child(format!("Asking {program} for a message…")),
                )
            })
            .when(pushed, |el| {
                el.child(
                    div()
                        .text_size(px(11.))
                        .text_color(t.modified)
                        .child("Already pushed: amending needs a force push"),
                )
            })
            .on_action(
                cx.listener(|this, _: &git::Commit, window, cx| this.commit(None, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &git::SuggestMessage, window, cx| this.suggest(window, cx)),
            )
    }
}

/// The ask before committing files GitHub won't take: the push fails, and an agent's dump or
/// build output committed by mistake then has to be cut out of history. `auto`: Commit all
/// staged them, and Cancel unstages them again.
fn large_files_text(large: &[gitviber_core::git::LargeFile], auto: bool) -> String {
    let mut shown: Vec<String> = large
        .iter()
        .take(10)
        .map(|f| format!("{} ({})", f.path, f.size))
        .collect();
    if large.len() > 10 {
        shown.push(format!("…and {} more", large.len() - 10));
    }
    let (it, these) = if large.len() == 1 {
        ("it", "This file is")
    } else {
        ("them", "These files are")
    };
    let out = if auto {
        format!("Cancel unstages what Commit all staged, so you can add {it} to .gitignore")
    } else {
        format!("Unstage {it} and add {it} to .gitignore")
    };
    format!(
        "{these} over GitHub's 100 MB limit:\n\n{}\n\nA push with {it} is refused, and taking {it} out later means rewriting history. {out}, or track {it} with Git LFS (git lfs track), before you commit.",
        shown.join("\n")
    )
}

/// git refused the commit for want of an author: "Author identity unknown … Please tell me who
/// you are", or an empty name.
fn no_identity(message: &str) -> bool {
    message.lines().any(|l| {
        l.starts_with("*** Please tell me who you are") || l.starts_with("fatal: empty ident name")
    })
}

#[cfg(test)]
mod identity_tests {
    use super::no_identity;

    #[test]
    fn a_commit_without_an_author_is_told_apart() {
        let git = "Author identity unknown\n\n*** Please tell me who you are.\n\nRun\n";
        assert!(no_identity(git));
        assert!(no_identity(
            "fatal: empty ident name (for <a@b>) not allowed"
        ));
        assert!(!no_identity("error: gpg failed to sign the data"));
    }
}
