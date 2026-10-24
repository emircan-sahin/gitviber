//! A bisect under way: mark the commit checked out good or bad (or skip it) until git names the
//! first bad one, then stop to go back where it started.

use gitviber_core::api;
use gitviber_refs::RepoState;
use gitviber_refs::ask::{Ask, ask};
use gitviber_refs::toast::Note;
use gitviber_ui::ActiveAppTheme;
use gitviber_ui::memory::Live;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FontWeight, Global, IntoElement, ParentElement as _,
    Render, SharedString, Styled as _, Task, Window, div, prelude::FluentBuilder as _, rems,
};

/// What git said last: its words, and the first bad commit once found.
#[derive(Default)]
struct Said(Option<(String, Option<String>)>);
impl Global for Said {}

pub fn said(v: Option<(String, Option<String>)>, cx: &mut App) {
    cx.set_global(Said(v));
    cx.refresh_windows();
}

fn last(cx: &App) -> Option<(String, Option<String>)> {
    cx.try_global::<Said>().and_then(|s| s.0.clone())
}

pub struct BisectBar {
    repo: Entity<RepoState>,
    busy: bool,
    task: Option<Task<()>>,
    _live: Live,
}

impl BisectBar {
    pub fn new(repo: Entity<RepoState>) -> Self {
        BisectBar {
            repo,
            busy: false,
            task: None,
            _live: Live::new("BisectBar"),
        }
    }

    fn mark(&mut self, verdict: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let core = gitviber_ui::core(cx);
        self.busy = true;
        let step =
            cx.background_spawn(async move { api::history::bisect_mark(&core, verdict.into()) });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let step = step.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match step {
                    Ok(step) => {
                        if let Some(bad) = &step.first_bad {
                            Note::success(format!(
                                "First bad commit: {}",
                                &bad[..bad.len().min(7)]
                            ))
                            .detail("Stop the bisect to go back to your branch.")
                            .show(window, cx);
                        }
                        said(Some((step.message, step.first_bad)), cx);
                    }
                    Err(e) => Note::error("Bisect failed").detail(e).show(window, cx),
                }
                this.repo.update(cx, |r, cx| r.refresh(cx));
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let found = last(cx).and_then(|l| l.1).is_some();
        let asked = (!found).then(|| {
            ask(
                Ask::new(
                    "Stop bisect",
                    "Stop bisecting? You go back to where it started.",
                    "Stop",
                ),
                window,
                cx,
            )
        });
        let core = gitviber_ui::core(cx);
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            if let Some(asked) = asked
                && !asked.await.unwrap_or(false)
            {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.busy = true;
                cx.notify();
            });
            let stopped = cx
                .background_executor()
                .spawn(async move { api::sync::op_abort(&core) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match stopped {
                    Ok(()) => said(None, cx),
                    Err(e) => Note::error("Bisect failed").detail(e).show(window, cx),
                }
                this.repo.update(cx, |r, cx| r.refresh(cx));
                cx.notify();
            });
        }));
    }
}

impl Render for BisectBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let last = last(cx);
        let bad = last.as_ref().and_then(|l| l.1.clone());
        let text: SharedString = match (&bad, &last) {
            (Some(b), _) => format!("The first bad commit is {}.", &b[..b.len().min(7)]).into(),
            (None, Some((m, _))) => m.clone().into(),
            _ => "Does the commit checked out have the problem?".into(),
        };
        let busy = self.busy;
        div()
            .flex_none()
            .border_b_1()
            .border_color(t.primary.opacity(0.3))
            .bg(t.primary.opacity(0.1))
            .px_3()
            .py_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(rems(0.75))
                    .child(
                        Icon::new(IconName::SearchCode)
                            .small()
                            .text_color(t.primary),
                    )
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Bisecting"))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(t.muted_foreground)
                            .child(text),
                    ),
            )
            .child(
                div()
                    .mt_2()
                    .flex()
                    .gap_1()
                    .when(bad.is_none(), |d| {
                        d.child(
                            Button::new("good")
                                .small()
                                .primary()
                                .label("Good")
                                .flex_1()
                                .disabled(busy)
                                .tooltip("This commit doesn't have the problem")
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.mark("good", window, cx)
                                    }),
                                ),
                        )
                        .child(
                            Button::new("bad")
                                .small()
                                .danger()
                                .label("Bad")
                                .flex_1()
                                .disabled(busy)
                                .tooltip("This commit has the problem")
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.mark("bad", window, cx)),
                                ),
                        )
                        .child(
                            Button::new("skip")
                                .small()
                                .label("Skip")
                                .disabled(busy)
                                .tooltip("Can't test this one: try a commit next to it")
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.mark("skip", window, cx)
                                    }),
                                ),
                        )
                    })
                    .child(
                        Button::new("stop")
                            .small()
                            .label("Stop")
                            .when(bad.is_some(), |b| b.flex_1())
                            .disabled(busy)
                            .tooltip("Stop and go back to your branch")
                            .on_click(cx.listener(|this, _, window, cx| this.stop(window, cx))),
                    ),
            )
    }
}
