//! The GitHub dialogs: asking before a write, a review, a new pull request (with the agent's
//! suggestion for its title and description) and a new issue.

use std::rc::Rc;

use gitviber_core::api;
use gitviber_core::github::Pull;
use gitviber_ui::Settings;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{Toast, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable as _, WindowExt as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    rems,
};

use crate::cache::Cache;
use crate::logic::actions::{ReviewEvent, branch_title};
use crate::logic::issue_work::with_closing;
use crate::logic::suggest::{PULL_PROMPT, command_line, parse_suggestion, program_of};
use crate::model::{Access, IssueItem, PullItem, Target};
use crate::store::Store;
use crate::ui::common::theme;
use crate::ui::pull_view::PullView;
use crate::ui::pulls_panel::BranchStatus;

/// Asks before a write; `on_ok` runs on OK.
pub fn confirm(
    title: &'static str,
    message: String,
    ok: &'static str,
    window: &mut Window,
    cx: &mut App,
    on_ok: impl Fn(&mut Window, &mut App) + 'static,
) {
    let on_ok = Rc::new(on_ok);
    let message: SharedString = message.into();
    window.open_alert_dialog(cx, move |alert, _, _| {
        let f = on_ok.clone();
        alert
            .title(title)
            .description(message.clone())
            .show_cancel(true)
            .ok_text(ok)
            .on_ok(move |_, window, cx| {
                f(window, cx);
                true
            })
    });
}

fn field_label(text: &'static str) -> impl IntoElement {
    div()
        .text_size(rems(0.72))
        .font_weight(FontWeight::MEDIUM)
        .child(text)
}

fn toast_in(ws: &WeakEntity<Workspace>, toast: Toast, cx: &mut App) {
    ws.update(cx, |ws, cx| ws.toast(toast, cx)).ok();
}

/// GitHub's "Review changes": a verdict and a note, which GitHub requires unless approving. The
/// text is the PR tab's: Esc or a click outside keeps it, and the dialog closes once the review
/// lands; a refused one keeps it for another try.
pub struct ReviewDialog {
    pull: WeakEntity<PullView>,
    body: Entity<TextareaState>,
    pick: ReviewEvent,
    /// Your own PR: GitHub refuses approving it or requesting changes.
    own: bool,
    /// Write access: anyone may review, but only a writer's verdict counts toward merging.
    counts: bool,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl ReviewDialog {
    pub fn open(
        pull: Entity<PullView>,
        body: Entity<TextareaState>,
        pick: Option<ReviewEvent>,
        own: bool,
        counts: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let view = cx.new(|cx| {
            let done = pull.read(cx).reviews_done();
            let subs = vec![
                cx.observe_in(&pull, window, move |_, pull, window, cx| {
                    if pull.read(cx).reviews_done() != done {
                        window.close_dialog(cx);
                    } else {
                        cx.notify();
                    }
                }),
                cx.subscribe(&body, |_, _, _: &InputEvent, cx| cx.notify()),
            ];
            let pick = pick
                .filter(|p| !own || *p == ReviewEvent::Comment)
                .unwrap_or(if own {
                    ReviewEvent::Comment
                } else {
                    ReviewEvent::Approve
                });
            ReviewDialog {
                pull: pull.downgrade(),
                body: body.clone(),
                pick,
                own,
                counts,
                _subs: subs,
                _live: Live::new("ReviewDialog"),
            }
        });
        let first = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                let body = view.read(cx).body.clone();
                body.update(cx, |b, cx| b.focus(window, cx));
            }
            dialog
                .title("Review changes")
                .w(px(400.))
                .child(view.clone())
        });
    }
}

impl Render for ReviewDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let body = self.body.read(cx).value().trim().to_string();
        let busy = self.pull.upgrade().is_some_and(|p| p.read(cx).is_busy());
        let ready = !busy && (!self.pick.needs_body() || !body.is_empty());
        let choices: Vec<AnyElement> = ReviewEvent::ALL
            .into_iter()
            .map(|e| {
                let off = self.own && e != ReviewEvent::Comment;
                let note = if off {
                    "Not available on your own pull request.".to_string()
                } else if !self.counts && e != ReviewEvent::Comment {
                    format!(
                        "{} Without write access it won't count toward merging.",
                        e.note()
                    )
                } else {
                    e.note().to_string()
                };
                let on = self.pick == e;
                div()
                    .id(e.as_str())
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .py(px(3.))
                    .when(off, |d| d.opacity(0.5))
                    .when(!off, |d| {
                        d.cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.pick = e;
                                cx.notify();
                            }))
                    })
                    .child(
                        Icon::new(if on {
                            IconName::CircleDot
                        } else {
                            IconName::Circle
                        })
                        .xsmall()
                        .text_color(if on { t.primary } else { t.subtle }),
                    )
                    .child(
                        div()
                            .child(
                                div()
                                    .text_size(rems(0.75))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(e.label()),
                            )
                            .child(
                                div()
                                    .text_size(rems(0.69))
                                    .text_color(t.muted_foreground)
                                    .child(note),
                            ),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(Textarea::new(&self.body).small())
            .children(choices)
            .child(
                div().flex().justify_end().child(
                    Button::new("submit-review")
                        .primary()
                        .small()
                        .label(if busy {
                            "Submitting…"
                        } else {
                            "Submit review"
                        })
                        .disabled(!ready)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let event = this.pick;
                            this.pull
                                .update(cx, |p, cx| p.submit_review(event, cx))
                                .ok();
                        })),
                ),
            )
    }
}

/// Where the branch stands now, read again: what Create and "Push to origin" decide on, since
/// an agent may have committed while the dialog was open.
fn read_status(core: &gitviber_core::Core) -> Result<BranchStatus, String> {
    let s = api::changes::status(core)?;
    Ok(BranchStatus::read(
        s.branch.clone(),
        s.follows.clone(),
        s.push.as_ref(),
    ))
}

/// "New pull request": into origin or a fork's original, from the current branch, which is
/// pushed first when GitHub doesn't have it as it is.
pub struct CreatePullDialog {
    ws: WeakEntity<Workspace>,
    target: Access,
    origin: Access,
    head: String,
    status: BranchStatus,
    title: Entity<InputState>,
    body: Entity<TextareaState>,
    /// Once typed in, the fields are the user's; the draft and a suggestion stop filling them.
    typed: (bool, bool),
    bases: Vec<String>,
    base: String,
    /// The base as a remote-tracking ref, once its remote is known: what the draft and a
    /// suggestion compare with.
    base_ref: Option<String>,
    draft: bool,
    maintainer_edits: bool,
    issue_url: Option<String>,
    busy: bool,
    suggesting: bool,
    _draft: Task<()>,
    _branches: Task<()>,
    _status: Task<()>,
    _suggest: Task<()>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl CreatePullDialog {
    pub fn open(
        ws: WeakEntity<Workspace>,
        target: Access,
        origin: Access,
        head: String,
        status: BranchStatus,
        window: &mut Window,
        cx: &mut App,
    ) {
        let view = cx.new(|cx| CreatePullDialog::new(ws, target, origin, head, status, window, cx));
        let first = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                let title = view.read(cx).title.clone();
                title.update(cx, |t, cx| t.focus(window, cx));
            }
            dialog
                .title("New pull request")
                .w(px(560.))
                .child(view.clone())
                .on_close({
                    let view = view.downgrade();
                    move |_, _, cx| {
                        // A suggestion still running describes nothing anyone will use.
                        if view.upgrade().is_some_and(|v| v.read(cx).suggesting) {
                            api::changes::suggest_cancel(
                                &crate::core(cx),
                                gitviber_core::suggest::Kind::Pull,
                            );
                        }
                    }
                })
        });
    }

    fn upstream(&self) -> bool {
        self.target.full_name() != self.origin.full_name()
    }

    fn new(
        ws: WeakEntity<Workspace>,
        target: Access,
        origin: Access,
        head: String,
        status: BranchStatus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Title")
                .default_value(head.clone())
        });
        let body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(8)
                .placeholder("Description (markdown)")
        });
        let subs = vec![
            cx.subscribe_in(
                &title,
                window,
                |this: &mut Self, _, ev, window, cx| match ev {
                    InputEvent::Change => {
                        this.typed.0 = true;
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.submit(window, cx),
                    _ => {}
                },
            ),
            cx.subscribe(&body, |this: &mut Self, _, ev: &InputEvent, _| {
                if matches!(ev, InputEvent::Change) {
                    this.typed.1 = true;
                }
            }),
        ];
        // A branch started from an issue closes it, as the body says where it can be seen.
        let issue_url = Store::get(cx).branch_issue(&origin.full_name(), &head);
        let base = target
            .default_branch
            .clone()
            .unwrap_or_else(|| "main".into());
        let mut view = CreatePullDialog {
            ws,
            bases: vec![base.clone()],
            base,
            base_ref: None,
            target,
            origin,
            head,
            status,
            title,
            body,
            typed: (false, false),
            draft: false,
            maintainer_edits: true,
            issue_url,
            busy: false,
            suggesting: false,
            _draft: Task::ready(()),
            _branches: Task::ready(()),
            _status: Task::ready(()),
            _suggest: Task::ready(()),
            _subs: subs,
            _live: Live::new("CreatePullDialog"),
        };
        let closing = view.closing("");
        view.body
            .update(cx, |b, cx| b.set_value(closing, window, cx));
        view.read_bases(cx);
        view.read_draft(window, cx);
        view
    }

    fn closing(&self, text: &str) -> String {
        with_closing(text, self.issue_url.as_deref(), &self.target.full_name())
    }

    /// The repo's default branch is always offered, even if not fetched. origin/* says nothing
    /// about the parent's branches: into the original, only its default is offered.
    fn read_bases(&mut self, cx: &mut Context<Self>) {
        if self.upstream() {
            return;
        }
        let core = crate::core(cx);
        self._branches = cx.spawn(async move |this, cx| {
            let Ok(branches) = cx
                .background_spawn(async move { api::branches::branches(&core) })
                .await
            else {
                return;
            };
            this.update(cx, |this, cx| {
                let mut bases = vec![this.base.clone()];
                for b in branches.iter().filter(|b| b.remote) {
                    if let Some(name) = b.name.strip_prefix("origin/")
                        && name != "HEAD"
                        && name != this.head
                        && !bases.iter().any(|x| x == name)
                    {
                        bases.push(name.to_string());
                    }
                }
                this.bases = bases;
                cx.notify();
            })
            .ok();
        });
    }

    /// GitHub's defaults: one commit titles the PR with its subject and fills the body; more take
    /// the branch name. Counted against the chosen base in the repository the PR goes to.
    fn read_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.base_ref = None;
        let (core, upstream, original, base, head) = (
            crate::core(cx),
            self.upstream(),
            self.target.full_name(),
            self.base.clone(),
            self.head.clone(),
        );
        self._draft = cx.spawn_in(window, async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    let remote = if upstream {
                        api::github::gh_original_remote(
                            &core,
                            original,
                            false,
                            "pr-draft".into(),
                            |_| {},
                        )?
                    } else {
                        Some("origin".into())
                    };
                    let Some(remote) = remote else {
                        return Ok(None);
                    };
                    let base_ref = format!("refs/remotes/{remote}/{base}");
                    let draft = api::github::pull_draft(&core, base_ref.clone())?;
                    Ok::<_, String>(Some((base_ref, draft)))
                })
                .await;
            let Ok(Some((base_ref, draft))) = found else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.base_ref = Some(base_ref);
                let one = (draft.commits == 1).then_some(draft.subject).flatten();
                if !this.typed.0 {
                    let title = one.clone().unwrap_or_else(|| branch_title(&head));
                    this.title
                        .update(cx, |t, cx| t.set_value(title, window, cx));
                }
                if !this.typed.1 {
                    let body = this.closing(&if one.is_some() {
                        draft.body.unwrap_or_default()
                    } else {
                        String::new()
                    });
                    this.body.update(cx, |b, cx| b.set_value(body, window, cx));
                }
                cx.notify();
            })
            .ok();
        });
    }

    fn pick_base(&mut self, base: String, window: &mut Window, cx: &mut Context<Self>) {
        if base == self.base {
            return;
        }
        self.base = base;
        // One on its way was asked against the old base.
        if self.suggesting {
            api::changes::suggest_cancel(&crate::core(cx), gitviber_core::suggest::Kind::Pull);
        }
        self.read_draft(window, cx);
        cx.notify();
    }

    fn suggest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(base_ref) = self.base_ref.clone() else {
            return;
        };
        let s = Settings::get(cx);
        let command = command_line(&s.suggest_command, &s.suggest_models, &s.suggest_efforts);
        let program = program_of(&s.suggest_command).to_string();
        let core = crate::core(cx);
        self.suggesting = true;
        self._suggest = cx.spawn_in(window, async move |this, cx| {
            let out = cx
                .background_spawn(async move {
                    api::changes::suggest_pull(&core, command, PULL_PROMPT.into(), base_ref)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.suggesting = false;
                match out.map(|o| parse_suggestion(&o)) {
                    Ok(Some((title, body))) => {
                        // A field typed into keeps its text; cleared, it's free again.
                        let mine = (
                            this.typed.0 && !this.title.read(cx).value().trim().is_empty(),
                            this.typed.1 && !this.body.read(cx).value().trim().is_empty(),
                        );
                        if !mine.0 {
                            this.title
                                .update(cx, |t, cx| t.set_value(title, window, cx));
                        }
                        if !mine.1 {
                            let body = this.closing(&body);
                            this.body.update(cx, |b, cx| b.set_value(body, window, cx));
                        }
                    }
                    Ok(None) => toast_in(
                        &this.ws,
                        Toast::error(format!("{program} printed nothing."))
                            .title("No description suggested"),
                        cx,
                    ),
                    Err(e) if e == gitviber_core::suggest::CANCELLED => {}
                    Err(e) => toast_in(
                        &this.ws,
                        Toast::error(e).title("Couldn't suggest a description"),
                        cx,
                    ),
                }
                cx.notify();
            })
            .ok();
        });
        cx.notify();
    }

    /// Pushes go to origin from now on; what the branch stands at is read again after.
    fn push_to_origin(&mut self, cx: &mut Context<Self>) {
        let core = crate::core(cx);
        self._status = cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move {
                    api::branches::set_push_default(&core, "origin".into())?;
                    read_status(&core)
                })
                .await;
            this.update(cx, |this, cx| {
                match read {
                    Ok(status) => this.status = status,
                    Err(e) => toast_in(
                        &this.ws,
                        Toast::error(e).title("Could not change where branches push"),
                        cx,
                    ),
                }
                cx.notify();
            })
            .ok();
        });
    }

    /// Creates it. The push is decided on the branch as it is now, not as the dialog opened; the
    /// request is the window's, so a dialog closed meanwhile still gets its PR opened.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.title.read(cx).value().trim().to_string();
        if title.is_empty() || self.busy || self.status.push_elsewhere.is_some() {
            return;
        }
        self.busy = true;
        let body = self.body.read(cx).value().to_string();
        let target: Target = self.upstream().then(|| self.target.full_name());
        let (core, head, base, draft, edits) = (
            crate::core(cx),
            self.head.clone(),
            self.base.clone(),
            self.draft,
            self.maintainer_edits,
        );
        let (ws, me) = (self.ws.clone(), cx.weak_entity());
        window
            .spawn(cx, async move |cx| {
                let made = cx
                    .background_spawn(async move {
                        let now = read_status(&core)?;
                        if now.branch.as_deref() != Some(head.as_str()) {
                            return Err(format!("{head} isn't checked out any more."));
                        }
                        if let Some(remote) = now.push_elsewhere {
                            return Err(format!(
                                "{head} pushes to {remote}, but a pull request needs it on origin."
                            ));
                        }
                        // GitHub can only open a PR from a branch it has: publish or push the latest first.
                        if now.needs_push() {
                            api::sync::push(
                                &core,
                                None,
                                None,
                                None,
                                "pr-create-push".into(),
                                |_| {},
                            )?;
                        }
                        api::github::pr_create(&core, target, title, body, head, base, draft, edits)
                    })
                    .await;
                cx.update(|window, cx| {
                    let open = me.update(cx, |this, cx| {
                        this.busy = false;
                        cx.notify();
                    });
                    match made {
                        Ok(p) => {
                            toast_in(&ws, Toast::success("Pull request created"), cx);
                            Cache::pulls_changed(cx);
                            if open.is_ok() {
                                window.close_dialog(cx);
                            }
                            crate::ui::open_pull(
                                &ws,
                                PullItem::from(&p as &Pull),
                                false,
                                window,
                                cx,
                            );
                        }
                        Err(e) => toast_in(
                            &ws,
                            Toast::error(e).title("Could not create pull request"),
                            cx,
                        ),
                    }
                })
                .ok();
            })
            .detach();
        cx.notify();
    }
}

impl Render for CreatePullDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let upstream = self.upstream();
        let head = if upstream {
            format!("{}:{}", self.origin.owner, self.head)
        } else {
            self.head.clone()
        };
        let base_name = if upstream {
            format!("{}:{}", self.target.full_name(), self.base)
        } else {
            self.base.clone()
        };
        let pushes = if self.status.push_elsewhere.is_none() && self.status.needs_push() {
            " · the branch will be pushed to origin first"
        } else {
            ""
        };
        let title_empty = self.title.read(cx).value().trim().is_empty();
        let can_suggest = Settings::get(cx).suggest_enabled;
        let mine_both = self.typed.0 && self.typed.1;
        let bases = self.bases.clone();
        let weak = cx.weak_entity();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().text_size(rems(0.75)).text_color(t.muted_foreground).font_family(t.fonts.code.clone()).child(format!("{head} → {base_name}{pushes}")))
            .when_some(self.status.push_elsewhere.clone(), |d, remote| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(3.))
                        .bg(t.removed.opacity(0.1))
                        .px(px(8.))
                        .py(px(6.))
                        .text_size(rems(0.72))
                        .text_color(t.removed)
                        .child(div().flex_1().child(format!(
                            "{} pushes to {remote}, but a pull request needs it on origin. Pulling can stay with {remote}; only pushes move.",
                            self.head
                        )))
                        .child(Button::new("push-origin").small().label("Push to origin from now on").on_click(cx.listener(|this, _, _, cx| this.push_to_origin(cx)))),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(div().flex_1().child(Input::new(&self.title)))
                    .when(can_suggest, |d| {
                        d.child(
                            Button::new("suggest")
                                .ghost()
                                .small()
                                .icon(IconName::Sparkles)
                                .loading(self.suggesting)
                                .tooltip(if self.suggesting { "Cancel the suggestion" } else { "Suggest a title and description" })
                                .disabled(!self.suggesting && (self.base_ref.is_none() || mine_both))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    if this.suggesting {
                                        api::changes::suggest_cancel(&crate::core(cx), gitviber_core::suggest::Kind::Pull);
                                    } else {
                                        this.suggest(window, cx);
                                    }
                                })),
                        )
                    }),
            )
            .child(Textarea::new(&self.body))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(rems(0.75))
                    .child(field_label("Base"))
                    .child(Button::new("base").small().label(self.base.clone()).dropdown_caret(true).dropdown_menu(move |menu, _, _| {
                        let mut menu = menu;
                        for b in bases.clone() {
                            let weak = weak.clone();
                            menu = menu.item(PopupMenuItem::new(b.clone()).on_click(move |_, window, cx| {
                                weak.update(cx, |this, cx| this.pick_base(b.clone(), window, cx)).ok();
                            }));
                        }
                        menu
                    }))
                    .child(div().flex_1())
                    .when(upstream, |d| {
                        d.child(
                            Checkbox::new("maintainer-edits")
                                .label("Maintainer edits")
                                .tooltip("Lets the original's maintainers push to your branch, as GitHub offers")
                                .checked(self.maintainer_edits)
                                .on_click(cx.listener(|this, on: &bool, _, cx| {
                                    this.maintainer_edits = *on;
                                    cx.notify();
                                })),
                        )
                    })
                    .child(Checkbox::new("draft").label("Draft").checked(self.draft).on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.draft = *on;
                        cx.notify();
                    })))
                    .child(
                        Button::new("create-pull")
                            .primary()
                            .small()
                            .label(if self.busy { "Creating…" } else { "Create" })
                            .disabled(self.busy || title_empty || self.status.push_elsewhere.is_some())
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
    }
}

/// "New issue", on origin (`target` None) or a fork's original.
pub struct CreateIssueDialog {
    ws: WeakEntity<Workspace>,
    target: Target,
    repo: String,
    title: Entity<InputState>,
    body: Entity<TextareaState>,
    busy: bool,
    _sub: Subscription,
    _live: Live,
}

impl CreateIssueDialog {
    pub fn open(
        ws: WeakEntity<Workspace>,
        target: Target,
        repo: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        let view = cx.new(|cx| {
            let title = cx.new(|cx| InputState::new(window, cx).placeholder("Title"));
            let sub = cx.subscribe_in(
                &title,
                window,
                |this: &mut Self, _, ev, window, cx| match ev {
                    InputEvent::PressEnter { .. } => this.submit(window, cx),
                    _ => cx.notify(),
                },
            );
            CreateIssueDialog {
                ws,
                target,
                repo,
                title,
                body: cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .rows(8)
                        .placeholder("Description (markdown)")
                }),
                busy: false,
                _sub: sub,
                _live: Live::new("CreateIssueDialog"),
            }
        });
        let first = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                let title = view.read(cx).title.clone();
                title.update(cx, |t, cx| t.focus(window, cx));
            }
            dialog.title("New issue").w(px(520.)).child(view.clone())
        });
    }

    /// The request is the window's: a dialog closed meanwhile still gets its issue opened, so
    /// nobody makes it twice.
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.title.read(cx).value().trim().to_string();
        if title.is_empty() || self.busy {
            return;
        }
        self.busy = true;
        let (core, target, body) = (
            crate::core(cx),
            self.target.clone(),
            self.body.read(cx).value().to_string(),
        );
        let (ws, me) = (self.ws.clone(), cx.weak_entity());
        window
            .spawn(cx, async move |cx| {
                let made = cx
                    .background_spawn(async move {
                        api::github::issue_create(&core, target, title, body)
                    })
                    .await;
                cx.update(|window, cx| {
                    let open = me.update(cx, |this, cx| {
                        this.busy = false;
                        cx.notify();
                    });
                    match made {
                        Ok(i) => {
                            toast_in(&ws, Toast::success("Issue created"), cx);
                            Cache::issues_changed(cx);
                            if open.is_ok() {
                                window.close_dialog(cx);
                            }
                            crate::ui::open_issue(&ws, IssueItem::from(&i), false, window, cx);
                        }
                        Err(e) => {
                            toast_in(&ws, Toast::error(e).title("Could not create issue"), cx)
                        }
                    }
                })
                .ok();
            })
            .detach();
        cx.notify();
    }
}

impl Render for CreateIssueDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let empty = self.title.read(cx).value().trim().is_empty();
        let place = if self.target.is_some() {
            format!("Opened on {}.", self.repo)
        } else {
            "Opened on the repository's GitHub page.".into()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(rems(0.75))
                    .text_color(t.muted_foreground)
                    .child(place),
            )
            .child(Input::new(&self.title))
            .child(Textarea::new(&self.body))
            .child(
                div().flex().justify_end().child(
                    Button::new("create-issue")
                        .primary()
                        .small()
                        .label(if self.busy { "Creating…" } else { "Create" })
                        .disabled(self.busy || empty)
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                ),
            )
    }
}
