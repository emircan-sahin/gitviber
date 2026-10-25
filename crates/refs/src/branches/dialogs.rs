//! The branch picker's actions that need more than a click: rename (on the remote too), a new
//! branch from any base, and the upstream.

use std::rc::Rc;

use gitviber_core::api;
use gitviber_ui::memory::Live;
use gitviber_ui::{ActiveAppTheme, Workspace};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Task, Window, div, prelude::FluentBuilder as _, px, rems,
};

use crate::names::{Branch, FOLD_CASE, local_names, ref_name_check, short_ref};
use crate::repo::{GitAction, RepoState, plain};
use crate::ui::{cancel, hint, note, open_form, ref_menu};

fn state_of(window: &Window, cx: &mut App) -> Option<Entity<RepoState>> {
    let ws = Workspace::for_window(window, cx)?;
    let project = ws.read(cx).project()?.clone();
    Some(RepoState::get(&project, cx))
}

/// Closes the dialog once `rx` says the action went through; else `pending` clears.
fn close_on_ok<V: 'static>(
    rx: futures::channel::oneshot::Receiver<bool>,
    window: &Window,
    cx: &mut Context<V>,
    pending: fn(&mut V) -> &mut bool,
) -> Task<()> {
    let handle = window.window_handle();
    cx.spawn(async move |this, cx| {
        let ok = rx.await.unwrap_or(false);
        let _ = handle.update(cx, |_, window, cx| {
            if ok {
                crate::askpass::close_form(window, cx);
            } else {
                let _ = this.update(cx, |this, cx| {
                    *pending(this) = false;
                    cx.notify();
                });
            }
        });
    })
}

// MARK: Rename

pub struct RenameBranch {
    state: Entity<RepoState>,
    branch: Branch,
    main: String,
    name: Entity<InputState>,
    remote: bool,
    pending: bool,
    _task: Option<Task<()>>,
    _subs: Vec<Subscription>,
    _live: Live,
}

pub fn open_rename_branch(branch: Branch, window: &mut Window, cx: &mut App) {
    let Some(state) = state_of(window, cx) else {
        return;
    };
    let form = cx.new(|cx| {
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("New name")
                .default_value(branch.name.clone())
        });
        let subs = vec![cx.subscribe_in(
            &name,
            window,
            |this: &mut RenameBranch, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                _ => cx.notify(),
            },
        )];
        RenameBranch {
            main: crate::ops::main_of(&state, cx),
            state,
            branch,
            name,
            remote: false,
            pending: false,
            _task: None,
            _subs: subs,
            _live: Live::new("RenameBranch"),
        }
    });
    let focus = form.read(cx).name.focus_handle(cx);
    open_form("Rename branch", 460., form, Some(focus), window, cx);
}

impl RenameBranch {
    /// Only an upstream that is still there can be renamed; a local one (remote ".") never.
    fn upstream(&self, cx: &App) -> Option<String> {
        let up = self.branch.upstream.as_ref()?;
        self.state
            .read(cx)
            .branches
            .iter()
            .find(|b| b.remote && &b.name == up)
            .map(|b| b.name.clone())
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let check = ref_name_check(
            &self.name.read(cx).value(),
            &local_names(&self.state.read(cx).branches, Some(&self.branch.name)),
            true,
            FOLD_CASE,
        );
        let n = check.name.clone();
        if n.is_empty() || n == self.branch.name || check.taken || self.pending {
            return;
        }
        let remote = self.remote && self.upstream(cx).is_some();
        let remote_name = self
            .upstream(cx)
            .and_then(|u| u.split_once('/').map(|(r, _)| r.to_string()))
            .unwrap_or_default();
        let old = self.branch.name.clone();
        let done = format!(
            "Renamed {old} to {n}{}",
            if remote {
                format!(" here and on {remote_name}")
            } else {
                String::new()
            }
        );
        let mut action = GitAction::new("Rename branch").done(done);
        if remote {
            action = action.net();
        }
        self.pending = true;
        let (from, to) = (old.clone(), n.clone());
        let rx = self.state.update(cx, |s, cx| {
            s.run(
                action,
                move |core, op| {
                    plain(api::branches::rename_branch(
                        core,
                        from,
                        to,
                        remote,
                        op.id.clone(),
                        op.progress(),
                    ))
                },
                window,
                cx,
            )
        });
        let main = self.main.clone();
        let handle = window.window_handle();
        self._task = Some(cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            let _ = handle.update(cx, |_, window, cx| {
                if ok {
                    // A branch renamed in the app keeps its pin.
                    crate::store::rename_pinned(&main, &old, &n, cx);
                    crate::askpass::close_form(window, cx);
                } else {
                    let _ = this.update(cx, |this, cx| {
                        this.pending = false;
                        cx.notify();
                    });
                }
            });
        }));
    }
}

impl Render for RenameBranch {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let check = ref_name_check(
            &self.name.read(cx).value(),
            &local_names(&self.state.read(cx).branches, Some(&self.branch.name)),
            true,
            FOLD_CASE,
        );
        let ready = !check.name.is_empty()
            && check.name != self.branch.name
            && !check.taken
            && !self.pending;
        let upstream = self.upstream(cx);
        let mut text = format!("Renames the local branch {}.", self.branch.name);
        if let Some(u) = &upstream {
            let r = u.split_once('/').map_or(u.as_str(), |(r, _)| r);
            text.push_str(&format!(
                " It tracks {u}, which keeps its name unless you rename it on {r} too."
            ));
        }
        let n = if check.name.is_empty() {
            "the new name".to_string()
        } else {
            check.name.clone()
        };
        div()
            .flex()
            .flex_col()
            .text_size(rems(0.75))
            .child(note(text, cx))
            .child(div().mt_3().child(Input::new(&self.name)))
            .children(hint(check.hint.as_deref(), check.taken, cx))
            .when_some(upstream, |d, u| {
                let r = u.split_once('/').map_or(u.clone(), |(r, _)| r.to_string());
                d.child(
                    div()
                        .mt_3()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(Checkbox::new("br-remote").label(format!("Rename it on {r} too")).checked(self.remote).on_click(cx.listener(|this, c: &bool, _, cx| {
                            this.remote = *c;
                            cx.notify();
                        })))
                        .child(note(format!("Pushes {n} and tracks it, then deletes {u} from {r}. Pull requests from the old name get closed, and other clones keep tracking it."), cx).pl_6()),
                )
            })
            .child(div().mt_4().flex().justify_end().gap_2().child(cancel()).child(
                Button::new("br-rename").primary().label("Rename").disabled(!ready).on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            ))
    }
}

// MARK: New branch

pub struct NewBranch {
    state: Entity<RepoState>,
    name: Entity<InputState>,
    from: String,
    head: bool,
    switch_to: bool,
    pending: bool,
    tags: Rc<Vec<String>>,
    _read: Task<()>,
    _task: Option<Task<()>>,
    _subs: Vec<Subscription>,
    _live: Live,
}

/// A new branch at `base`: a full ref (refs/heads/…, refs/remotes/…), or HEAD.
pub fn open_new_branch(base: String, window: &mut Window, cx: &mut App) {
    let Some(state) = state_of(window, cx) else {
        return;
    };
    let form = cx.new(|cx| {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Branch name"));
        let subs = vec![cx.subscribe_in(
            &name,
            window,
            |this: &mut NewBranch, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                _ => cx.notify(),
            },
        )];
        let core = gitviber_ui::core(cx);
        let tags =
            cx.background_spawn(async move { api::branches::tags(&core).unwrap_or_default() });
        let read = cx.spawn(async move |this, cx| {
            let tags = tags.await;
            let _ = this.update(cx, |this, cx| {
                this.tags = Rc::new(tags);
                cx.notify();
            });
        });
        NewBranch {
            head: base == "HEAD",
            state,
            name,
            from: base,
            switch_to: true,
            pending: false,
            tags: Rc::default(),
            _read: read,
            _task: None,
            _subs: subs,
            _live: Live::new("NewBranch"),
        }
    });
    let focus = form.read(cx).name.focus_handle(cx);
    open_form("New branch", 440., form, Some(focus), window, cx);
}

impl NewBranch {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let check = ref_name_check(
            &self.name.read(cx).value(),
            &local_names(&self.state.read(cx).branches, None),
            false,
            FOLD_CASE,
        );
        let n = check.name.clone();
        if n.is_empty() || check.taken || self.pending {
            return;
        }
        let label = if self.from == "HEAD" {
            "HEAD".to_string()
        } else {
            short_ref(&self.from).to_string()
        };
        let done = if self.switch_to {
            format!("Switched to new branch {n}")
        } else {
            format!("Created {n} from {label}")
        };
        let (from, switch) = (self.from.clone(), self.switch_to);
        self.pending = true;
        let rx = self.state.update(cx, |s, cx| {
            s.run(
                GitAction::new("Create branch").done(done),
                move |core, _| plain(api::branches::create_branch(core, n, from, switch)),
                window,
                cx,
            )
        });
        self._task = Some(close_on_ok(rx, window, cx, |this| &mut this.pending));
    }
}

impl Render for NewBranch {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let branches = self.state.read(cx).branches.clone();
        let check = ref_name_check(
            &self.name.read(cx).value(),
            &local_names(&branches, None),
            false,
            FOLD_CASE,
        );
        let ready = !check.name.is_empty() && !check.taken && !self.pending;
        let label = if self.from == "HEAD" {
            "HEAD".to_string()
        } else {
            short_ref(&self.from).to_string()
        };
        let (tags, current, head, weak) = (
            self.tags.clone(),
            self.from.clone(),
            self.head,
            cx.weak_entity(),
        );
        div()
            .flex()
            .flex_col()
            .text_size(rems(0.75))
            .child(note(
                "It starts where the base is and tracks nothing until you publish it.",
                cx,
            ))
            .child(div().mt_3().child(Input::new(&self.name)))
            .children(hint(check.hint.as_deref(), check.taken, cx))
            .child(
                div()
                    .mt_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(note("From", cx))
                    .child(
                        Button::new("br-base")
                            .small()
                            .outline()
                            .label(label)
                            .dropdown_caret(true)
                            .dropdown_menu(move |menu, _, _| {
                                let weak = weak.clone();
                                ref_menu(
                                    menu,
                                    &branches,
                                    &tags,
                                    head,
                                    &current,
                                    Rc::new(move |r, _, cx| {
                                        let _ = weak.update(cx, |this, cx| {
                                            this.from = r;
                                            cx.notify();
                                        });
                                    }),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .mt_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Checkbox::new("br-switch")
                            .label("Switch to it")
                            .checked(self.switch_to)
                            .on_click(cx.listener(|this, c: &bool, _, cx| {
                                this.switch_to = *c;
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(cancel())
                    .child(
                        Button::new("br-create")
                            .primary()
                            .label("Create")
                            .disabled(!ready)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
    }
}

// MARK: Upstream

pub struct SetUpstream {
    state: Entity<RepoState>,
    branch: Branch,
    upstream: String,
    pending: bool,
    _task: Option<Task<()>>,
    _live: Live,
}

pub fn open_set_upstream(branch: Branch, window: &mut Window, cx: &mut App) {
    let Some(state) = state_of(window, cx) else {
        return;
    };
    let remotes: Vec<String> = state
        .read(cx)
        .branches
        .iter()
        .filter(|b| b.remote && !b.name.ends_with("/HEAD"))
        .map(|b| b.name.clone())
        .collect();
    let guess = [
        branch.upstream.clone(),
        Some(format!("origin/{}", branch.name)),
    ]
    .into_iter()
    .flatten()
    .find(|u| remotes.contains(u))
    .or_else(|| {
        remotes
            .iter()
            .find(|r| r.ends_with(&format!("/{}", branch.name)))
            .cloned()
    })
    .or_else(|| remotes.first().cloned())
    .unwrap_or_default();
    let form = cx.new(|_| SetUpstream {
        state,
        branch,
        upstream: guess,
        pending: false,
        _task: None,
        _live: Live::new("SetUpstream"),
    });
    open_form("Set upstream", 420., form, None, window, cx);
}

impl SetUpstream {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.upstream.is_empty() || self.pending {
            return;
        }
        self.pending = true;
        let (name, up) = (self.branch.name.clone(), self.upstream.clone());
        let action = GitAction::new("Set upstream")
            .untracked()
            .done(format!("{name} now tracks {up}"));
        let rx = self.state.update(cx, |s, cx| {
            s.run(
                action,
                move |core, _| plain(api::branches::set_upstream(core, name, Some(up))),
                window,
                cx,
            )
        });
        self._task = Some(close_on_ok(rx, window, cx, |this| &mut this.pending));
    }
}

impl Render for SetUpstream {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let remotes: Vec<String> = self
            .state
            .read(cx)
            .branches
            .iter()
            .filter(|b| b.remote && !b.name.ends_with("/HEAD"))
            .map(|b| b.name.clone())
            .collect();
        let now = self
            .branch
            .upstream
            .as_ref()
            .map(|u| format!("; now {u}"))
            .unwrap_or_default();
        let weak = cx.weak_entity();
        let current = self.upstream.clone();
        div()
            .flex()
            .flex_col()
            .text_size(rems(0.75))
            .child(note(
                format!(
                    "What {} pulls from and compares against{now}.",
                    self.branch.name
                ),
                cx,
            ))
            .child(if remotes.is_empty() {
                note(
                    "No remote branches. Fetch, or publish the branch first.",
                    cx,
                )
                .mt_3()
                .into_any_element()
            } else {
                div()
                    .mt_3()
                    .child(
                        Button::new("br-upstream")
                            .small()
                            .outline()
                            .label(self.upstream.clone())
                            .dropdown_caret(true)
                            .dropdown_menu(move |mut menu, _, _| {
                                menu = menu.scrollable(true).max_h(px(320.)).min_w(px(240.));
                                for r in &remotes {
                                    let (weak, r) = (weak.clone(), r.clone());
                                    menu = menu.item(
                                        PopupMenuItem::new(r.clone())
                                            .checked(r == current)
                                            .on_click(move |_, _, cx| {
                                                let _ = weak.update(cx, |this, cx| {
                                                    this.upstream = r.clone();
                                                    cx.notify();
                                                });
                                            }),
                                    );
                                }
                                menu
                            }),
                    )
                    .into_any_element()
            })
            .child(
                div()
                    .mt_4()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .text_color(t.foreground)
                    .child(cancel())
                    .child(
                        Button::new("br-set-upstream")
                            .primary()
                            .label("Set upstream")
                            .disabled(self.upstream.is_empty() || self.pending)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
    }
}
