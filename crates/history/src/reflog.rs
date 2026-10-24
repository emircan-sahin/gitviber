//! Where HEAD has been (git reflog): a way back to commits a reset, rebase or amend left behind,
//! by a branch made there, a reset of the current one to it, or a look at it (detached).

use std::rc::Rc;

use gitviber_core::{api, git};
use gitviber_refs::ask::{Ask, ask};
use gitviber_refs::repo::plain;
use gitviber_refs::{GitAction, RepoState};
use gitviber_ui::ActiveAppTheme;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{now_unix, relative_time};
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, Window, div, prelude::FluentBuilder as _, px, rems, uniform_list,
};

pub struct Reflog {
    repo: Entity<RepoState>,
    entries: Option<Result<Rc<Vec<git::ReflogEntry>>, SharedString>>,
    /// The entry a new branch is being named at.
    naming: Option<usize>,
    name: Entity<InputState>,
    hover: Option<usize>,
    _read: Task<()>,
    flow: Option<Task<()>>,
    _sub: Subscription,
    _live: Live,
}

impl Reflog {
    fn head(&self) -> String {
        match &self.entries {
            Some(Ok(e)) => e.first().map(|e| e.sha.clone()).unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn entry(&self, i: usize) -> Option<(String, String)> {
        match &self.entries {
            Some(Ok(e)) => e.get(i).map(|e| (e.sha.clone(), e.message.clone())),
            _ => None,
        }
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(i), name) = (self.naming, self.name.read(cx).value().trim().to_string()) else {
            return;
        };
        let Some((sha, _)) = self.entry(i).filter(|_| !name.is_empty()) else {
            return;
        };
        let a = GitAction::new("Create branch").done(format!("Switched to new branch {name}"));
        let done = self.repo.update(cx, |r, cx| {
            r.run(
                a,
                move |core, _| plain(api::history::create_branch_at(core, name, sha)),
                window,
                cx,
            )
        });
        self.close_after(done, window, cx);
    }

    fn checkout(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((sha, _)) = self.entry(i) else {
            return;
        };
        let a =
            GitAction::new("Checkout").done(format!("Checked out {}", &sha[..7.min(sha.len())]));
        let done = self.repo.update(cx, |r, cx| {
            r.run(
                a,
                move |core, _| plain(api::history::checkout_commit(core, sha)),
                window,
                cx,
            )
        });
        self.close_after(done, window, cx);
    }

    fn reset(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((sha, message)) = self.entry(i) else {
            return;
        };
        let head = self.head();
        let short = sha[..7.min(sha.len())].to_string();
        let q = Ask::new(
            "Reset to here",
            format!("Move the current branch to {short} ({message})? Uncommitted changes to tracked files are discarded, and commits after it leave the branch."),
            "Reset",
        )
        .danger();
        let asked = ask(q, window, cx);
        self.flow = Some(cx.spawn_in(window, async move |this, cx| {
            if !asked.await.unwrap_or(false) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                let a = GitAction::new("Reset").done(format!("Reset to {short}"));
                let done = this.repo.update(cx, |r, cx| {
                    r.run(
                        a,
                        move |core, _| {
                            plain(api::history::reset(core, sha, git::ResetMode::Hard, head))
                        },
                        window,
                        cx,
                    )
                });
                this.close_after(done, window, cx);
            });
        }));
    }

    fn close_after(
        &mut self,
        done: futures::channel::oneshot::Receiver<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flow = Some(cx.spawn_in(window, async move |_, cx| {
            if done.await.unwrap_or(false) {
                let _ = cx.update(gitviber_refs::askpass::close_form);
            }
        }));
    }
}

impl Render for Reflog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let body = match &self.entries {
            None => div()
                .px_3()
                .py_2()
                .text_color(t.subtle)
                .child("Loading…")
                .into_any_element(),
            Some(Err(e)) => div()
                .px_3()
                .py_2()
                .text_color(t.muted_foreground)
                .child(e.clone())
                .into_any_element(),
            Some(Ok(e)) if e.is_empty() => div()
                .px_3()
                .py_2()
                .text_color(t.subtle)
                .child("Nothing yet.")
                .into_any_element(),
            Some(Ok(entries)) => {
                let entries = entries.clone();
                let hover = self.hover;
                uniform_list(
                    "reflog",
                    entries.len(),
                    cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                        let t = cx.app_theme().clone();
                        let now = now_unix();
                        range
                            .map(|i| {
                                let e = &entries[i];
                                let hot = hover == Some(i);
                                div()
                                    .id(("entry", i))
                                    .flex()
                                    .h(px(32.))
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .border_b_1()
                                    .border_color(t.border)
                                    .text_size(rems(0.75))
                                    .when(hot, |d| d.bg(t.hover))
                                    .on_hover(cx.listener(move |this, h: &bool, _, cx| {
                                        let next = if *h {
                                            Some(i)
                                        } else if this.hover == Some(i) {
                                            None
                                        } else {
                                            this.hover
                                        };
                                        if next != this.hover {
                                            this.hover = next;
                                            cx.notify();
                                        }
                                    }))
                                    .child(
                                        div()
                                            .w(px(64.))
                                            .flex_none()
                                            .font_family(t.fonts.code.clone())
                                            .text_size(rems(0.6875))
                                            .text_color(t.subtle)
                                            .child(e.sha[..7.min(e.sha.len())].to_string()),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .child(e.message.clone()),
                                    )
                                    .when(!hot, |d| {
                                        d.child(
                                            div()
                                                .flex_none()
                                                .text_size(rems(0.6875))
                                                .text_color(t.subtle)
                                                .child(if i == 0 {
                                                    "now".to_string()
                                                } else {
                                                    relative_time(e.timestamp, now)
                                                }),
                                        )
                                    })
                                    .when(hot, |d| {
                                        d.child(
                                            div()
                                                .flex()
                                                .flex_none()
                                                .gap_px()
                                                .child(
                                                    Button::new(("branch", i))
                                                        .ghost()
                                                        .xsmall()
                                                        .icon(IconName::GitBranchPlus)
                                                        .tooltip("Create branch here…")
                                                        .on_click(cx.listener(
                                                            move |this, _, window, cx| {
                                                                this.naming = Some(i);
                                                                this.name.update(cx, |n, cx| {
                                                                    n.focus(window, cx)
                                                                });
                                                                cx.notify();
                                                            },
                                                        )),
                                                )
                                                .when(i > 0, |d| {
                                                    d.child(
                                                        Button::new(("checkout", i))
                                                            .ghost()
                                                            .xsmall()
                                                            .icon(IconName::GitCommitHorizontal)
                                                            .tooltip("Check out (detached)")
                                                            .on_click(cx.listener(
                                                                move |this, _, window, cx| {
                                                                    this.checkout(i, window, cx)
                                                                },
                                                            )),
                                                    )
                                                    .child(
                                                        Button::new(("reset", i))
                                                            .ghost()
                                                            .xsmall()
                                                            .icon(IconName::RotateCcwClock)
                                                            .tooltip(
                                                                "Reset the current branch to here…",
                                                            )
                                                            .on_click(cx.listener(
                                                                move |this, _, window, cx| {
                                                                    this.reset(i, window, cx)
                                                                },
                                                            )),
                                                    )
                                                }),
                                        )
                                    })
                            })
                            .collect()
                    }),
                )
                .h(px(360.))
                .into_any_element()
            }
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_color(t.muted_foreground).child(
                "Everywhere HEAD has been, newest first. Commits a reset, rebase or amend left behind are still here, until git's cleanup drops them.",
            ))
            .child(div().rounded(px(6.)).border_1().border_color(t.border).overflow_hidden().child(body))
            .when_some(self.naming, |d, i| {
                let short = self.entry(i).map(|(s, _)| s[..7.min(s.len())].to_string()).unwrap_or_default();
                d.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(div().flex_1().child(Input::new(&self.name)))
                        .child(Button::new("create").primary().label(format!("Create at {short}")).on_click(cx.listener(|this, _, window, cx| this.create(window, cx)))),
                )
            })
    }
}

pub fn open_reflog(repo: Entity<RepoState>, window: &mut Window, cx: &mut App) {
    let name = cx.new(|cx| InputState::new(window, cx).placeholder("New branch name"));
    let view = cx.new(|cx| {
        let core = gitviber_ui::core(cx);
        let read = cx.background_spawn(async move { api::history::reflog(&core, 500) });
        let task = cx.spawn(async move |this, cx| {
            let entries = read.await;
            let _ = this.update(cx, |this: &mut Reflog, cx| {
                this.entries = Some(entries.map(Rc::new).map_err(Into::into));
                cx.notify();
            });
        });
        let sub = cx.subscribe_in(
            &name,
            window,
            |this: &mut Reflog, _, e: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = e {
                    this.create(window, cx);
                }
            },
        );
        Reflog {
            repo,
            entries: None,
            naming: None,
            name,
            hover: None,
            _read: task,
            flow: None,
            _sub: sub,
            _live: Live::new("Reflog"),
        }
    });
    window.open_dialog(cx, move |dialog, _, _| {
        dialog.title("Reflog").w(px(640.)).child(view.clone())
    });
}
