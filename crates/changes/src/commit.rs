//! The message being written and the commit: summary and description, Amend (which swaps in
//! HEAD's message and keeps the draft aside), co-authors, sign-off, skipping hooks once, and ✦
//! asking the user's own agent CLI for a message. Nothing staged commits everything.

use crate::model::commit::{Facts, SUMMARY_LIMIT, plan};
use crate::model::list::{left_out, stageable};
use crate::model::text::{SUGGEST_PROMPT, command_line, parse_suggestion, program_of};
use crate::model::{Kind, Lists};
use crate::store::{self, Draft};
use gitviber_core::api;
use gitviber_core::git::{Commit, CommitOptions};
use gitviber_core::suggest::{Kind as SuggestKind, Scope};
use gitviber_ui::actions::git;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{Toast, Workspace};
use gitviber_ui::{ActiveAppTheme as _, Settings};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
};
use std::sync::Arc;

/// While amending: HEAD's message in the fields, the user's own draft aside.
struct Amend {
    aside: Draft,
    sha: String,
    original: (String, String),
}

/// What the panel tells the box about the repo.
#[derive(Clone, Default)]
pub struct RepoFacts {
    pub root: String,
    /// The main worktree: sign-off is kept per repository, across its worktrees.
    pub main: String,
    pub branch: Option<String>,
    pub head: Option<Arc<Commit>>,
    pub lists: Lists,
    /// Set while git has left a message for the next commit (a squash merge, `cherry-pick -n`).
    pub prepared: Option<String>,
    pub pushed: bool,
}

pub struct CommitBox {
    workspace: WeakEntity<Workspace>,
    facts: RepoFacts,
    summary: Entity<InputState>,
    body: Entity<TextareaState>,
    co_authors: Vec<String>,
    amend: Option<Amend>,
    no_verify: bool,
    busy: bool,
    suggesting: bool,
    /// The description a new draft starts with (commit.template, a prepared message).
    start_body: String,
    recent_authors: Vec<String>,
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
        let mut view = CommitBox {
            workspace,
            facts: RepoFacts::default(),
            summary,
            body,
            co_authors: vec![],
            amend: None,
            no_verify: false,
            busy: false,
            suggesting: false,
            start_body: String::new(),
            recent_authors: vec![],
            _subscriptions: subscriptions,
            _task: Task::ready(()),
            _suggest: Task::ready(()),
            _template: Task::ready(()),
            _live: Live::new("CommitBox"),
        };
        view.load_authors(cx);
        view
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
        let untouched = draft.summary.trim().is_empty()
            && (draft.body.trim().is_empty() || draft.body == self.start_body)
            && draft.co_authors.is_empty();
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
                let (summary, body) = match (&template, prepared) {
                    (Some(t), true) => {
                        let (s, b) = t.split_once('\n').unwrap_or((t, ""));
                        (s.to_string(), b.trim().to_string())
                    }
                    (t, false) => (String::new(), t.clone().unwrap_or_default()),
                    (None, true) => (String::new(), String::new()),
                };
                let old_start = std::mem::replace(&mut this.start_body, body.clone());
                let untouched = this.summary_text(cx).trim().is_empty()
                    && (this.body_text(cx).trim().is_empty() || this.body_text(cx) == old_start);
                if this.amend.is_none() && untouched {
                    this.set_fields(&summary, &body, window, cx);
                }
            })
            .ok();
        });
    }

    fn load_authors(&mut self, cx: &mut Context<Self>) {
        let core = crate::core(cx);
        self._task = cx.spawn(async move |this, cx| {
            let authors = cx
                .background_spawn(async move { api::changes::recent_authors(&core) })
                .await
                .unwrap_or_default();
            this.update(cx, |this, _| this.recent_authors = authors)
                .ok();
        });
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
            busy: self.busy,
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
        let plan = plan(self.facts_for_plan(cx));
        if plan.blocked.is_some() {
            return;
        }
        self.drop_suggestion(cx);
        self.busy = true;
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
            no_verify: self.no_verify,
            co_authors: self.co_authors.clone(),
        };
        let (paths, skipped) = stageable(self.facts.lists.list(Kind::Unstaged));
        let auto = plan.stages_all;
        let core = crate::core(cx);
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
                    this.update(cx, |this, cx| this.failed("Commit failed", &e, cx)).ok();
                    return;
                }
            };
            if !large.is_empty() {
                let shown: Vec<String> = large.iter().take(10).map(|f| format!("{} ({})", f.path, f.size)).collect();
                let text = format!(
                    "{} over GitHub's 100 MB limit:\n\n{}\n\nA push with them is refused, and taking them out later means rewriting history.",
                    if large.len() == 1 { "This file is" } else { "These files are" },
                    shown.join("\n")
                );
                let weak = this.clone();
                let (paths, message, options) = (paths.clone(), message.clone(), options);
                this.update_in(cx, move |_, window, cx| {
                    let go = std::rc::Rc::new(std::cell::RefCell::new(Some((message, options))));
                    let (go_ok, weak_ok, weak_cancel) = (go.clone(), weak.clone(), weak.clone());
                    window.open_alert_dialog(cx, move |alert, _, _| {
                        let (go_ok, weak_ok, weak_cancel, paths) = (go_ok.clone(), weak_ok.clone(), weak_cancel.clone(), paths.clone());
                        alert
                            .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                            .title("Large files")
                            .description(text.clone())
                            .ok_text("Commit anyway")
                            .show_cancel(true)
                            .on_ok(move |_, window, cx| {
                                if let Some((message, options)) = go_ok.borrow_mut().take() {
                                    weak_ok.update(cx, |this, cx| this.run_commit(message, options, then, skipped, window, cx)).ok();
                                }
                                true
                            })
                            .on_cancel(move |_, _, cx| {
                                let paths = paths.clone();
                                weak_cancel
                                    .update(cx, |this, cx| {
                                        this.busy = false;
                                        // Nothing was staged before, so unstaging what this staged puts the index back.
                                        if auto {
                                            let core = crate::core(cx);
                                            cx.background_spawn(async move { api::changes::unstage(&core, paths) }).detach();
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
            this.update_in(cx, |this, window, cx| this.run_commit(message, options, then, skipped, window, cx)).ok();
        });
    }

    fn run_commit(
        &mut self,
        message: String,
        options: CommitOptions,
        then: Option<Then>,
        skipped: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let core = crate::core(cx);
        let amended = options.amend;
        let summary = self.summary_text(cx);
        self._task = cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    api::changes::commit(&core, message, options, "commit".into(), |_| {})
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        this.clear(window, cx);
                        this.no_verify = false;
                        this.toast(
                            Toast::success(summary.trim().to_string()).title(if amended {
                                "Commit amended"
                            } else {
                                "Committed"
                            }),
                            cx,
                        );
                        if skipped > 0 && !amended {
                            this.toast(
                                Toast::info(crate::model::list::NESTED_EXPLAINED)
                                    .title(format!("{} of the commit", left_out(skipped))),
                                cx,
                            );
                        }
                        match then {
                            Some(Then::Push) => window.dispatch_action(Box::new(git::Push), cx),
                            Some(Then::Sync) => window.dispatch_action(Box::new(git::Sync), cx),
                            None => {}
                        }
                    }
                    Err(e) => this.failed("Commit failed", &e, cx),
                }
                this.refresh_project(cx);
                cx.notify();
            })
            .ok();
        });
    }

    fn failed(&mut self, title: &str, e: &str, cx: &mut Context<Self>) {
        self.busy = false;
        self.toast(Toast::error(e.to_string()).title(title.to_string()), cx);
        // git has no name or email for the commit: the app asks for them, even after a "Not now".
        if no_identity(e) {
            cx.dispatch_action(&gitviber_ui::keymap::requests::AskGitIdentity);
        }
        self.refresh_project(cx);
        cx.notify();
    }

    /// After a commit the fields start over; an amend's draft set aside comes back.
    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.amend.take().map(|a| a.aside).unwrap_or(Draft {
            body: self.start_body.clone(),
            ..Default::default()
        });
        self.co_authors = next.co_authors.clone();
        self.set_fields(&next.summary, &next.body, window, cx);
    }

    fn can_suggest(&self, cx: &App) -> bool {
        let s = Settings::get(cx);
        s.suggest_enabled
            && !self.suggesting
            && !self.busy
            && (self.amend.is_some()
                || self.facts.lists.staged.len() + self.facts.lists.unstaged.len() > 0)
    }

    pub fn suggest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_suggest(cx) {
            return;
        }
        let s = Settings::get(cx);
        let line = command_line(&s.suggest_command, &s.suggest_models, &s.suggest_efforts);
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
                    api::changes::suggest_message(&core, line, SUGGEST_PROMPT.into(), scope)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.suggesting = false;
                match out.map(|o| parse_suggestion(&o)) {
                    Ok(Some((summary, body))) => {
                        let before = (this.summary_text(cx), this.body_text(cx));
                        let blank = before.0.trim().is_empty()
                            && (before.1.trim().is_empty() || before.1 == this.start_body);
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
        let tip: SharedString = plan
            .blocked
            .map(str::to_string)
            .unwrap_or(plan.target.clone())
            .into();
        let weak = cx.entity().downgrade();
        let authors: Vec<String> = self
            .recent_authors
            .iter()
            .filter(|a| !self.co_authors.contains(a))
            .take(12)
            .cloned()
            .collect();
        let chip = |label: String, tip: &'static str, on_remove: OnRemove| {
            h_flex()
                .gap_1()
                .px_1p5()
                .rounded_sm()
                .bg(t.elevated)
                .text_size(px(11.))
                .text_color(t.muted_foreground)
                .child(div().child(label))
                .child(
                    div()
                        .id(SharedString::from(format!("chip-{tip}")))
                        .cursor_pointer()
                        .child("×")
                        .on_click(move |_, window, cx| on_remove(window, cx)),
                )
        };
        let options_weak = weak.clone();
        v_flex()
            .flex_none()
            .gap_1p5()
            .p_2()
            .border_t_1()
            .border_color(t.border)
            .bg(t.chrome(t.panel))
            .child(
                div()
                    .relative()
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
                                    "Skip hooks".into(),
                                    "hooks",
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
                                    "Sign off".into(),
                                    "signoff",
                                    Box::new(move |_, cx| {
                                        w2.update(cx, |this, cx| this.set_sign_off(false, cx)).ok();
                                    }),
                                ))
                            })
                            .children(self.co_authors.iter().map(|a| {
                                let (w, a2) = (weak.clone(), a.clone());
                                let name = a.split(" <").next().unwrap_or(a).to_string();
                                chip(
                                    name,
                                    "coauthor",
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
                                let mut menu = menu;
                                for author in &authors {
                                    let (w, a) = (options_weak.clone(), author.clone());
                                    menu =
                                        menu.item(
                                            PopupMenuItem::new(format!(
                                                "Co-author: {}",
                                                author.split(" <").next().unwrap_or(author)
                                            ))
                                            .on_click(move |_, _, cx| {
                                                let a = a.clone();
                                                w.update(cx, |this, cx| {
                                                    this.co_authors.push(a);
                                                    this.keep_draft(cx);
                                                    cx.notify();
                                                })
                                                .ok();
                                            }),
                                        );
                                }
                                if !authors.is_empty() {
                                    menu = menu.separator();
                                }
                                let (w1, w2, w3, w4) = (
                                    options_weak.clone(),
                                    options_weak.clone(),
                                    options_weak.clone(),
                                    options_weak.clone(),
                                );
                                menu.item(
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
                            .label(if self.busy {
                                "Committing…"
                            } else {
                                plan.label
                            })
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
