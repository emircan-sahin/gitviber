//! An issue's tab: read it, comment, edit, label, close (completed or not planned) and reopen,
//! delete (admins), or start it in its own worktree. Its body and thread live here only.

use std::sync::Arc;
use std::time::Instant;

use gitviber_core::api;
use gitviber_core::github::IssueDetail;
use gitviber_ui::actions::github::PostComment;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{TabIcon, TabItem, Toast, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, RetainAllImageCache,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    WeakEntity, Window, div, prelude::FluentBuilder as _, px, rems,
};

use crate::cache::{Cache, MIN_AGE, Wakes};
use crate::logic::actions::issue_rights;
use crate::logic::issue_work::issue_branch_name;
use crate::model::{Account, IssueItem, LabelItem, Target, repo_of};
use crate::requests::{GithubRequest, request};
use crate::ui::common::{
    Markdown, ago, copy, issue_icon, issue_pill, label_chip, open_url, section, theme,
};
use crate::ui::dialogs::confirm;
use crate::ui::labels::LabelPicker;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CloseReason {
    Completed,
    NotPlanned,
}

impl CloseReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::NotPlanned => "not_planned",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Completed => "Close as completed",
            Self::NotPlanned => "Close as not planned",
        }
    }
}

pub struct IssueView {
    url: String,
    number: u64,
    target: Target,
    web: String,
    item: Option<IssueItem>,
    ws: WeakEntity<Workspace>,
    detail: Option<Arc<IssueDetail>>,
    detail_error: Option<String>,
    detail_at: Option<Instant>,
    loading: bool,
    body: Option<Markdown>,
    thread: Vec<Markdown>,
    editing: Option<(Entity<InputState>, Entity<TextareaState>)>,
    comment: Entity<TextareaState>,
    comment_clear: bool,
    /// The labels being picked, shown in place of the issue's until they're saved.
    picking: Option<Vec<LabelItem>>,
    busy: Option<SharedString>,
    wakes: Wakes,
    images: Entity<RetainAllImageCache>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    _detail: Task<()>,
    _action: Task<()>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl IssueView {
    pub fn new(
        ws: WeakEntity<Workspace>,
        url: String,
        item: Option<IssueItem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let number = url
            .rsplit('/')
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or_default();
        let repo = repo_of(&url);
        let comment = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(4)
                .placeholder("Leave a comment (markdown)")
        });
        let subs = vec![
            cx.subscribe(&comment, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.observe_global::<Cache>(|this: &mut Self, cx| {
                if Cache::wakes(cx) != this.wakes {
                    cx.notify();
                }
            }),
            cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    Cache::wake(cx);
                }
            }),
        ];
        let mut view = IssueView {
            web: format!("https://github.com/{repo}"),
            target: Some(repo),
            url,
            number,
            item,
            ws,
            detail: None,
            detail_error: None,
            detail_at: None,
            loading: false,
            body: None,
            thread: vec![],
            editing: None,
            comment,
            comment_clear: false,
            picking: None,
            busy: None,
            wakes: Cache::wakes(cx),
            images: RetainAllImageCache::new(cx),
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            _detail: Task::ready(()),
            _action: Task::ready(()),
            _subs: subs,
            _live: Live::new("IssueView"),
        };
        crate::account::load(cx, false);
        view.load(cx);
        view
    }

    fn item(&self) -> Option<IssueItem> {
        self.detail
            .as_ref()
            .map(|d| IssueItem::from(&d.issue))
            .or_else(|| self.item.clone())
    }

    fn toast(&self, toast: Toast, cx: &mut App) {
        self.ws.update(cx, |ws, cx| ws.toast(toast, cx)).ok();
    }

    pub fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.detail_at = Some(Instant::now());
        let (core, target, number) = (crate::core(cx), self.target.clone(), self.number);
        self._detail = cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move { api::github::issue_detail(&core, target, number) })
                .await;
            this.update(cx, |this, cx| {
                this.loading = false;
                match read {
                    Ok(d) => {
                        // Closed by a push or edited on GitHub: the lists still show it as it was.
                        if Cache::list_is_behind::<IssueItem>(
                            cx,
                            "issues:",
                            &d.issue.url,
                            &d.issue.updated_at,
                            |r| (r.url.as_str(), r.updated_at.as_str()),
                        ) {
                            Cache::issues_changed(cx);
                        }
                        let none = |_: &str| None;
                        this.body = (!d.body.trim().is_empty())
                            .then(|| Markdown::new(&d.body, &this.web, &this.url, &none, cx));
                        this.thread = d
                            .thread
                            .iter()
                            .map(|c| Markdown::new(&c.body, &this.web, &this.url, &none, cx))
                            .collect();
                        this.detail = Some(Arc::new(d));
                        this.detail_error = None;
                    }
                    Err(e) => this.detail_error = Some(e),
                }
                cx.notify();
            })
            .ok();
        });
        cx.notify();
    }

    fn account(&self, cx: &App) -> Option<Arc<Account>> {
        crate::account::account(cx)
    }

    /// Runs a write under `busy`; on success says `done`, reads again, and runs `after`.
    fn act(
        &mut self,
        busy: &'static str,
        done: String,
        run: impl FnOnce(&gitviber_core::Core) -> Result<(), String> + Send + 'static,
        after: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(busy.into());
        let core = crate::core(cx);
        self._action = cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { run(&core) }).await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match result {
                    Ok(()) => {
                        this.toast(Toast::success(done), cx);
                        Cache::issues_changed(cx);
                        after(this, cx);
                        this.load(cx);
                    }
                    Err(e) => this.toast(Toast::error(e).title(format!("{busy} failed")), cx),
                }
                cx.notify();
            })
            .ok();
        });
        cx.notify();
    }

    fn text(&self, cx: &App) -> String {
        self.comment.read(cx).value().trim().to_string()
    }

    /// The comment box empties at the next frame, which has the window it takes.
    fn clear_comment(&mut self, cx: &mut Context<Self>) {
        self.comment_clear = true;
        cx.notify();
    }

    fn post(&mut self, cx: &mut Context<Self>) {
        let text = self.text(cx);
        if text.is_empty() {
            return;
        }
        let (target, number) = (self.target.clone(), self.number);
        self.act(
            "Comment",
            "Comment added".into(),
            move |core| api::github::issue_comment(core, target, number, text),
            |this, cx| this.clear_comment(cx),
            cx,
        );
    }

    /// Like GitHub's "Close with comment": the comment goes first so it reads before the close.
    fn set_open(&mut self, open: bool, reason: CloseReason, cx: &mut Context<Self>) {
        let (target, number, text) = (self.target.clone(), self.number, self.text(cx));
        let done = format!("{} #{number}", if open { "Reopened" } else { "Closed" });
        let run = move |core: &gitviber_core::Core| {
            if !text.is_empty() {
                api::github::issue_comment(core, target.clone(), number, text)?;
            }
            api::github::issue_set_open(core, target, number, open, reason.as_str().into())
                .map(|_| ())
        };
        self.act(
            if open { "Reopen" } else { "Close" },
            done,
            run,
            |this, cx| this.clear_comment(cx),
            cx,
        );
    }

    fn edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = &self.detail else { return };
        let (title, body) = (d.issue.title.clone(), d.body.clone());
        let title = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Title")
                .default_value(title)
        });
        let body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(12)
                .placeholder("Description (markdown)")
                .default_value(body)
        });
        title.update(cx, |t, cx| t.focus(window, cx));
        self.editing = Some((title, body));
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let Some((title, body)) = &self.editing else {
            return;
        };
        let (title, body) = (
            title.read(cx).value().trim().to_string(),
            body.read(cx).value().to_string(),
        );
        if title.is_empty() {
            return;
        }
        let (target, number) = (self.target.clone(), self.number);
        self.act(
            "Edit",
            format!("Updated #{number}"),
            move |core| api::github::issue_edit(core, target, number, title, body).map(|_| ()),
            |this, _| this.editing = None,
            cx,
        );
    }

    /// Like GitHub, the picked labels are saved together when the picker closes.
    fn pick_labels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = self.item() else { return };
        self.picking = Some(i.labels.clone());
        let weak = cx.weak_entity();
        let closing = cx.weak_entity();
        LabelPicker::open(
            vec![self.target.clone()],
            i.labels.clone(),
            Some("Saved when closed".into()),
            std::rc::Rc::new(move |labels, _, cx| {
                weak.update(cx, |this, cx| {
                    this.picking = Some(labels);
                    cx.notify();
                })
                .ok();
            }),
            Some(std::rc::Rc::new(
                move |labels: Vec<LabelItem>, _: &mut Window, cx: &mut App| {
                    closing
                        .update(cx, |this, cx| {
                            this.save_labels(labels, i.labels.clone(), cx)
                        })
                        .ok();
                },
            )),
            window,
            cx,
        );
    }

    fn save_labels(&mut self, picked: Vec<LabelItem>, had: Vec<LabelItem>, cx: &mut Context<Self>) {
        let same = picked.len() == had.len()
            && picked.iter().all(|l| had.iter().any(|m| m.name == l.name));
        if same {
            self.picking = None;
            cx.notify();
            return;
        }
        let (target, number) = (self.target.clone(), self.number);
        let names: Vec<String> = picked.iter().map(|l| l.name.clone()).collect();
        self.act(
            "Labels",
            "Labels updated".into(),
            move |core| api::github::issue_set_labels(core, target, number, names).map(|_| ()),
            |this, _| this.picking = None,
            cx,
        );
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = self.item() else { return };
        let weak = cx.weak_entity();
        let message = format!(
            "Delete #{} \"{}\" and all its comments? This can't be undone.",
            i.number, i.title
        );
        confirm(
            "Delete issue",
            message,
            "Delete",
            window,
            cx,
            move |_, cx| {
                weak.update(cx, |this, cx| {
                    let (target, number, key) = (
                        this.target.clone(),
                        this.number,
                        format!("issue:{}", this.url),
                    );
                    let ws = this.ws.clone();
                    // A deleted issue's tab closes: nothing to read again.
                    this.act(
                        "Delete",
                        format!("Deleted #{number}"),
                        move |core| api::github::issue_delete(core, target, number),
                        move |_, cx| {
                            ws.update(cx, |ws, cx| ws.close_tab(&key, cx)).ok();
                        },
                        cx,
                    );
                })
                .ok();
            },
        );
    }

    fn start_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(i), Some(origin)) = (
            self.item(),
            self.account(cx)
                .and_then(|a| a.origin.as_ref().map(|o| o.full_name())),
        ) else {
            return;
        };
        let branch = issue_branch_name(i.number, &i.title);
        request(
            GithubRequest::StartIssue {
                number: i.number,
                title: i.title,
                url: i.url,
                origin,
                branch,
            },
            window,
            cx,
        );
    }
}

impl TabItem for IssueView {
    fn tab_title(&self, _: &App) -> SharedString {
        match self.item() {
            Some(i) => format!("#{} {}", i.number, i.title).into(),
            None => format!("#{}", self.number).into(),
        }
    }

    fn tab_icon(&self, _: &App) -> TabIcon {
        TabIcon::Icon(IconName::CircleDot)
    }

    fn tab_tooltip(&self, _: &App) -> Option<SharedString> {
        Some(self.url.clone().into())
    }

    fn tab_focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.focus.clone())
    }
}

impl Render for IssueView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let wakes = Cache::wakes(cx);
        if wakes != self.wakes {
            self.wakes = wakes;
            if self.detail_at.is_none_or(|at| at.elapsed() > MIN_AGE) && !self.loading {
                self.load(cx);
            }
        }
        if std::mem::take(&mut self.comment_clear) {
            self.comment.update(cx, |c, cx| c.set_value("", window, cx));
        }
        let Some(i) = self.item() else {
            let text = self
                .detail_error
                .clone()
                .unwrap_or_else(|| "Loading the issue…".into());
            return div()
                .id("issue")
                .track_focus(&self.focus)
                .size_full()
                .p(px(32.))
                .text_center()
                .text_size(rems(0.78))
                .text_color(t.muted_foreground)
                .child(text)
                .into_any_element();
        };
        let account = self.account(cx);
        let d = self.detail.clone();
        let rights = issue_rights(
            &i,
            account.as_deref(),
            d.as_ref().and_then(|d| d.closed_by.as_deref()),
        );
        let busy = self.busy.is_some();
        let text = self.text(cx);
        let labels = self.picking.clone().unwrap_or_else(|| i.labels.clone());

        let header: AnyElement = if let Some((title, body)) = &self.editing {
            let empty = title.read(cx).value().trim().is_empty();
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(Input::new(title))
                .child(Textarea::new(body))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(6.))
                        .child(
                            Button::new("cancel-edit")
                                .ghost()
                                .small()
                                .label("Cancel")
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.editing = None;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("save-edit")
                                .primary()
                                .small()
                                .label("Save")
                                .disabled(busy || empty)
                                .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                        ),
                )
                .into_any_element()
        } else {
            let mut meta = div()
                .mt(px(6.))
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .text_size(rems(0.75))
                .text_color(t.muted_foreground)
                .child(issue_pill(&i, &t))
                .child(
                    div()
                        .text_color(t.foreground.opacity(0.85))
                        .child(i.author.clone()),
                )
                .child(format!("opened {}", ago(&i.created_at)));
            if !i.assignees.is_empty() {
                meta = meta
                    .child(div().text_color(t.subtle).child("·"))
                    .child(format!("assigned to {}", i.assignees.join(", ")));
            }
            for l in &labels {
                meta = meta.child(label_chip(l, &t));
            }
            if rights.label {
                meta = meta.child(
                    Button::new("edit-labels")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Tag)
                        .label(if labels.is_empty() {
                            "Add labels"
                        } else {
                            "Edit labels"
                        })
                        .disabled(busy || d.is_none())
                        .on_click(cx.listener(|this, _, window, cx| this.pick_labels(window, cx))),
                );
            }
            div()
                .flex()
                .items_start()
                .gap(px(12.))
                .child(div().mt(px(6.)).child(issue_icon(&i, &t)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_x(px(6.))
                                .text_size(rems(1.06))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(i.title.clone())
                                .child(
                                    div()
                                        .text_color(t.subtle)
                                        .font_weight(FontWeight::NORMAL)
                                        .child(format!("#{}", i.number)),
                                ),
                        )
                        .child(meta),
                )
                .into_any_element()
        };

        let (url, copy_url) = (i.url.clone(), i.url.clone());
        let weak = cx.weak_entity();
        let close_label = if text.is_empty() {
            "Close issue"
        } else {
            "Close with comment"
        };
        let actions = div()
            .mt(px(16.))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .when(self.editing.is_none() && rights.edit, |a| {
                a.child(
                    Button::new("edit")
                        .small()
                        .icon(IconName::Pencil)
                        .label("Edit")
                        .disabled(busy || d.is_none())
                        .on_click(cx.listener(|this, _, window, cx| this.edit(window, cx))),
                )
            })
            .when(i.open && rights.close, |a| {
                a.child(
                    Button::new("close")
                        .small()
                        .icon(IconName::CircleCheck)
                        .label(close_label)
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_open(false, CloseReason::Completed, cx)
                        })),
                )
                .child(
                    Button::new("close-how")
                        .small()
                        .icon(IconName::ChevronDown)
                        .disabled(busy)
                        .dropdown_menu(move |menu, _, _| {
                            let mut menu = menu;
                            for r in [CloseReason::Completed, CloseReason::NotPlanned] {
                                let weak = weak.clone();
                                let icon = if r == CloseReason::Completed {
                                    IconName::CircleCheck
                                } else {
                                    IconName::CircleSlash
                                };
                                menu =
                                    menu.item(PopupMenuItem::new(r.label()).icon(icon).on_click(
                                        move |_, _, cx| {
                                            weak.update(cx, |this, cx| this.set_open(false, r, cx))
                                                .ok();
                                        },
                                    ));
                            }
                            menu
                        }),
                )
            })
            .when(!i.open && rights.reopen, |a| {
                a.child(
                    Button::new("reopen")
                        .small()
                        .icon(IconName::CircleDot)
                        .label(if text.is_empty() {
                            "Reopen"
                        } else {
                            "Reopen with comment"
                        })
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_open(true, CloseReason::Completed, cx)
                        })),
                )
            })
            .when(
                i.open && account.as_ref().is_some_and(|a| a.origin.is_some()),
                |a| {
                    a.child(
                        Button::new("start")
                            .small()
                            .icon(IconName::FolderGit2)
                            .label("Start in a worktree…")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.start_worktree(window, cx)),
                            ),
                    )
                },
            )
            .child(
                Button::new("on-github")
                    .small()
                    .icon(IconName::ExternalLink)
                    .label("Open on GitHub")
                    .on_click(move |_, _, cx| open_url(&url, cx)),
            )
            .child(
                Button::new("copy-link")
                    .ghost()
                    .small()
                    .icon(IconName::Link)
                    .tooltip("Copy link")
                    .on_click(move |_, window, cx| copy(&copy_url, "Link copied", window, cx)),
            )
            .child(
                Button::new("reload")
                    .ghost()
                    .small()
                    .icon(IconName::RefreshCw)
                    .tooltip("Refresh")
                    .loading(self.loading)
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
            )
            .when_some(self.busy.clone(), |a, b| {
                a.child(
                    div()
                        .text_size(rems(0.75))
                        .text_color(t.muted_foreground)
                        .child(format!("{b}…")),
                )
            })
            .when_some(
                self.detail_error.clone().filter(|_| !busy && d.is_some()),
                |a, e| {
                    a.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(rems(0.75))
                            .text_color(t.removed)
                            .child(format!("Couldn't refresh: {e}")),
                    )
                },
            )
            // GitHub lets repository admins alone delete issues.
            .when(rights.delete, |a| {
                a.child(div().flex_1()).child(
                    Button::new("delete")
                        .ghost()
                        .small()
                        .danger()
                        .icon(IconName::Trash)
                        .label("Delete")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))),
                )
            });

        let mut page = div()
            .mx_auto()
            .max_w(px(900.))
            .w_full()
            .px(px(24.))
            .py(px(20.))
            .child(header)
            .child(actions);
        if self.editing.is_none() {
            let desc = match (&self.body, &d) {
                (Some(md), _) => div()
                    .px(px(12.))
                    .py(px(10.))
                    .child(md.element())
                    .into_any_element(),
                (None, Some(_)) => div()
                    .px(px(12.))
                    .py(px(10.))
                    .text_size(rems(0.75))
                    .text_color(t.subtle)
                    .italic()
                    .child("No description provided.")
                    .into_any_element(),
                (None, None) => div().h(px(8.)).into_any_element(),
            };
            page = page.child(section("Description", None, &t).child(desc));
        }
        if let Some(d) = d.as_ref().filter(|d| !d.thread.is_empty()) {
            let mut s = section("Conversation", Some(d.thread.len().to_string().into()), &t);
            for (c, md) in d.thread.iter().zip(&self.thread) {
                s = s.child(
                    div()
                        .px(px(12.))
                        .py(px(10.))
                        .border_b_1()
                        .border_color(t.border)
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .text_size(rems(0.72))
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(c.author.clone()),
                                )
                                .child(div().text_color(t.subtle).child(ago(&c.created_at))),
                        )
                        .when(!c.body.trim().is_empty(), |d| {
                            d.child(div().mt(px(6.)).child(md.element()))
                        }),
                );
            }
            page = page.child(s);
        }
        let can_post = !text.is_empty() && !busy;
        page = page.child(
            section("Add a comment", None, &t).child(
                div()
                    .p(px(8.))
                    .key_context("IssueComment")
                    .on_action(cx.listener(move |this, _: &PostComment, _, cx| {
                        if can_post {
                            this.post(cx);
                        }
                    }))
                    .child(Textarea::new(&self.comment).small())
                    .child(
                        div().mt(px(8.)).flex().justify_end().child(
                            Button::new("post")
                                .primary()
                                .small()
                                .label("Comment")
                                .disabled(!can_post)
                                .on_click(cx.listener(|this, _, _, cx| this.post(cx))),
                        ),
                    ),
            ),
        );
        div()
            .image_cache(self.images.clone())
            .id("issue")
            .track_focus(&self.focus)
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(page)
            .into_any_element()
    }
}
