//! The worktree dialogs: New worktree (a branch, or one from a commit, in its own folder, with
//! a terminal running a command there), Rename (its folder too), Lock, and Clean up.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gitviber_core::{api, git};
use gitviber_ui::memory::Live;
use gitviber_ui::{ActiveAppTheme, Workspace};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement as _, PathPromptOptions, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, Window, div, prelude::FluentBuilder as _, px, rems,
};

use super::{
    Cleanable, Worktree, WorktreeEvent, WorktreeEvents, default_branch, folder_for_branch,
    parent_folder, short_path,
};
use crate::names::{
    FOLD_CASE, WorktreeBase, folder_name, local_names, plural, ref_name_check, short_ref,
    worktree_base, worktree_branch,
};
use crate::ops::{main_of, open_worktree};
use crate::repo::{GitAction, RepoState, undo_action};
use crate::toast::{Kind, Note};
use crate::ui::{hint, note, open_form, ref_menu};

/// The window's repo state, when a project is open.
fn state_of(window: &Window, cx: &mut App) -> Option<Entity<RepoState>> {
    let ws = Workspace::for_window(window, cx)?;
    let project = ws.read(cx).project()?.clone();
    Some(RepoState::get(&project, cx))
}

fn is_commit(base: &str) -> bool {
    (base.len() == 40 || base.len() == 64) && base.chars().all(|c| c.is_ascii_hexdigit())
}

// MARK: New worktree

/// A pull request to check out in a new worktree, as GitHub's views name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullAsk {
    /// owner/name of the repository the PR is on.
    pub target: String,
    pub number: u64,
    pub head_ref: String,
    /// The PR's branch lives on origin: checked out under its own name.
    pub same_repo: bool,
    /// The local branch it lands on.
    pub branch: String,
}

/// Remembers a branch as an issue's once its worktree is made: a PR from it then closes it.
pub type Remember = Rc<dyn Fn(&str, &mut App)>;

/// An issue to start work on: the branch is named after it, and a PR from it closes it.
#[derive(Clone)]
pub struct IssueAsk {
    pub number: u64,
    pub title: String,
    /// The branch it proposes (`12-short-title`), which can be changed.
    pub branch: String,
    pub remember: Option<Remember>,
}

#[derive(Clone)]
enum Source {
    Plain,
    Pull(PullAsk),
    Issue(IssueAsk),
}

/// The Run command with `{issue}` as the issue's number: `claude "Fix #{issue}"`.
pub fn with_issue(command: &str, number: u64) -> String {
    command.replace("{issue}", &number.to_string())
}

pub struct NewWorktree {
    state: Entity<RepoState>,
    source: Source,
    main: String,
    beside: String,
    fallback: String,
    name: Entity<InputState>,
    command: Entity<InputState>,
    from: String,
    /// No branch to default to (unborn or detached): HEAD is listed, not silently used.
    head_option: bool,
    dir: String,
    terminal: bool,
    switch_to: bool,
    /// Asked once, when Create is pressed on the default branch.
    warned: bool,
    pending: bool,
    includes: usize,
    tags: Rc<Vec<String>>,
    /// A pull request's form has nothing to type: Enter is the form's own.
    focus: gpui_kit::FocusHandle,
    _read: Task<()>,
    _pick: Option<Task<()>>,
    _subs: Vec<Subscription>,
    _live: Live,
}

fn open_with(source: Source, base: Option<String>, window: &mut Window, cx: &mut App) {
    let Some(state) = state_of(window, cx) else {
        return;
    };
    let title = match &source {
        Source::Pull(p) => format!("Check out #{} in a new worktree", p.number),
        Source::Issue(i) => format!("Start #{} in a new worktree", i.number),
        Source::Plain => "New worktree".into(),
    };
    let pull = matches!(source, Source::Pull(_));
    let form = cx.new(|cx| NewWorktree::new(state, base, source, window, cx));
    let focus = if pull { form.read(cx).focus.clone() } else { form.read(cx).name.focus_handle(cx) };
    open_form(title, 480., form, Some(focus), window, cx);
}

/// The New worktree dialog, from `base` (a full ref or a commit's id; None: the default).
pub fn open_new_worktree(base: Option<String>, window: &mut Window, cx: &mut App) {
    open_with(Source::Plain, base, window, cx);
}

/// A pull request's branch in a worktree of its own (GitHub's "Check out in new worktree…"):
/// the folder, a terminal and its command, and whether to switch, asked first.
pub fn open_worktree_for_pull(pull: PullAsk, window: &mut Window, cx: &mut App) {
    open_with(Source::Pull(pull), None, window, cx);
}

/// A new branch for an issue, in a worktree of its own (GitHub's "Start in a worktree…"): its
/// name proposed and editable, `{issue}` in the command its number.
pub fn open_worktree_for_issue(issue: IssueAsk, window: &mut Window, cx: &mut App) {
    open_with(Source::Issue(issue), None, window, cx);
}

impl NewWorktree {
    fn new(
        state: Entity<RepoState>,
        base: Option<String>,
        source: Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let main = main_of(&state, cx);
        let beside = format!("{}{}.worktrees", parent_folder(&main), folder_name(&main));
        let shared = gitviber_ui::Settings::get(cx)
            .worktree_root
            .clone()
            .map(|r| format!("{}/{}", r.trim_end_matches('/'), folder_name(&main)));
        let fallback = shared.unwrap_or_else(|| beside.clone());
        let dir = crate::store::worktree_dir(&main, cx).unwrap_or_else(|| fallback.clone());
        let from = base.unwrap_or_else(|| worktree_base(&state.read(cx).branches));
        let issue = matches!(source, Source::Issue(_));
        let run = if issue { crate::store::issue_run(&main, cx) } else { crate::store::worktree_run(&main, cx) };
        let proposed = match &source {
            Source::Issue(i) => i.branch.clone(),
            _ => String::new(),
        };
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Branch name").default_value(proposed));
        let command = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(if issue { "and run… (e.g. claude \"Fix #{issue}\")" } else { "and run… (e.g. claude)" })
                .default_value(run)
        });
        let subs = vec![
            cx.subscribe_in(&name, window, |this: &mut Self, _, e: &InputEvent, window, cx| match e {
                InputEvent::Change => {
                    this.warned = false;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                _ => {}
            }),
            // The hint under the command follows what's typed in it.
            cx.subscribe(&command, |_, _, e: &InputEvent, cx| {
                if let InputEvent::Change = e {
                    cx.notify();
                }
            }),
        ];
        let core = gitviber_ui::core(cx);
        let read = cx.background_spawn(async move {
            (
                api::worktrees::worktree_includes(&core).unwrap_or(0),
                api::branches::tags(&core).unwrap_or_default(),
            )
        });
        let read = cx.spawn(async move |this, cx| {
            let (includes, tags) = read.await;
            let _ = this.update(cx, |this, cx| {
                this.includes = includes;
                this.tags = Rc::new(tags);
                cx.notify();
            });
        });
        NewWorktree {
            head_option: from == "HEAD",
            state,
            source,
            main,
            beside,
            fallback,
            name,
            command,
            from,
            dir,
            terminal: true,
            switch_to: false,
            warned: false,
            pending: false,
            includes: 0,
            tags: Rc::default(),
            focus: cx.focus_handle(),
            _read: read,
            _pick: None,
            _subs: subs,
            _live: Live::new("NewWorktree"),
        }
    }

    fn typed(&self, cx: &App) -> String {
        self.name.read(cx).value().to_string()
    }

    fn pull(&self) -> Option<&PullAsk> {
        match &self.source {
            Source::Pull(p) => Some(p),
            _ => None,
        }
    }

    fn issue(&self) -> Option<&IssueAsk> {
        match &self.source {
            Source::Issue(i) => Some(i),
            _ => None,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        if let Some(pull) = self.pull().cloned() {
            return self.check_out(pull, window, cx);
        }
        let branches = self.state.read(cx).branches.clone();
        let picked = worktree_branch(&self.typed(cx), &branches, FOLD_CASE);
        let existing = picked.base != WorktreeBase::Picked;
        let n = picked.name.clone();
        // The default branch checked out here would be held by this folder, and git then refuses
        // to switch to it anywhere else, the main folder included.
        let guarded =
            existing && !picked.taken && Some(n.as_str()) == default_branch(&branches).as_deref();
        if n.is_empty() || picked.taken {
            return;
        }
        if guarded && !self.warned {
            self.warned = true;
            cx.notify();
            return;
        }
        self.create(picked, window, cx);
    }

    /// What runs after the worktree is made, in it: the command, `{issue}` filled in.
    fn command_to_run(&self, cx: &App) -> Option<String> {
        let typed = self.command.read(cx).value().trim().to_string();
        self.terminal.then(|| match self.issue() {
            Some(i) => with_issue(&typed, i.number),
            None => typed,
        })
    }

    /// What the dialog remembers and asks for once the worktree is there, without the form,
    /// which may be gone by then (a pull request's closes as its fetch starts).
    fn after(&self, branch: String, cx: &App) -> After {
        After {
            main: self.main.clone(),
            dir: self.dir.clone(),
            fallback: self.fallback.clone(),
            typed: self.terminal.then(|| self.command.read(cx).value().trim().to_string()),
            command: self.command_to_run(cx),
            issue: self.issue().is_some(),
            remember: self.issue().and_then(|i| i.remember.clone()).map(|r| (r, branch)),
            switch_to: self.switch_to,
        }
    }

    fn create(
        &mut self,
        picked: crate::names::WorktreeBranch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // "Create anyway" clicked twice, or after Create: one add_worktree at a time.
        if self.pending {
            return;
        }
        let n = picked.name.clone();
        let base = match picked.base {
            WorktreeBase::Picked => Some(self.from.clone()),
            WorktreeBase::Existing => None,
            WorktreeBase::Ref(r) => Some(r),
        };
        let existing = base.as_deref() != Some(self.from.as_str()) || picked.track;
        let where_ = (self.dir != self.beside).then(|| self.dir.clone());
        let made = Arc::new(Mutex::new(None::<String>));
        let slot = made.clone();
        let track = picked.track;
        let done = if existing {
            format!("Checked out {n} in a new worktree")
        } else {
            format!("Created worktree {n}")
        };
        let after = self.after(n.clone(), cx);
        self.pending = true;
        cx.notify();
        let rx = self.state.update(cx, |s, cx| {
            s.run(
                GitAction::new("Create worktree").untracked().done(done),
                move |core, _| {
                    let path = api::worktrees::add_worktree(core, n, base, track, where_)?;
                    *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
                    Ok(false)
                },
                window,
                cx,
            )
        });
        let handle = window.window_handle();
        self._pick = Some(cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            let path = made.lock().unwrap_or_else(|e| e.into_inner()).take();
            let _ = handle.update(cx, |_, window, cx| {
                let Some(path) = path.filter(|_| ok) else {
                    let _ = this.update(cx, |this, cx| {
                        this.pending = false;
                        cx.notify();
                    });
                    return;
                };
                crate::askpass::close_form(window, cx);
                after.made(path, window, cx);
            });
        }));
    }

    /// A pull request's branch: nothing typed to keep, and the fetch's Cancel is in the top bar
    /// behind this dialog, so the dialog goes as it starts.
    fn check_out(&mut self, pull: PullAsk, window: &mut Window, cx: &mut Context<Self>) {
        let where_ = (self.dir != self.beside).then(|| self.dir.clone());
        let made = Arc::new(Mutex::new(None::<String>));
        let slot = made.clone();
        let after = self.after(pull.branch.clone(), cx);
        let done = format!("Checked out #{} in worktree {}", pull.number, folder_for_branch(&pull.branch));
        self.pending = true;
        let rx = self.state.update(cx, |s, cx| {
            s.run(
                GitAction::new("Check out PR").net().untracked().done(done),
                move |core, op| {
                    let path = api::github::pr_checkout_worktree(
                        core,
                        Some(pull.target),
                        pull.number,
                        pull.head_ref,
                        pull.same_repo,
                        where_,
                        op.id.clone(),
                        op.progress(),
                    )?;
                    *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
                    Ok(false)
                },
                window,
                cx,
            )
        });
        crate::askpass::close_form(window, cx);
        // Holds no view: the form is gone with its dialog.
        let handle = window.window_handle();
        cx.spawn(async move |_, cx| {
            let ok = rx.await.unwrap_or(false);
            let path = made.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(path) = path.filter(|_| ok) {
                let _ = handle.update(cx, |_, window, cx| after.made(path, window, cx));
            }
        })
        .detach();
    }

    fn choose_folder(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        self._pick = Some(cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(p) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    this.dir = p.to_string_lossy().into_owned();
                    cx.notify();
                });
            }
        }));
    }

    /// A new branch from the default one instead: the name is the only thing left to type.
    fn branch_off(
        &mut self,
        picked_base: Option<String>,
        n: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.from = picked_base.unwrap_or_else(|| format!("refs/heads/{n}"));
        self.name.update(cx, |s, cx| s.set_value("", window, cx));
        self.warned = false;
        self.name.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }
}

/// What follows a new worktree: its folder (and its command) remembered for the project, an
/// issue's branch remembered as the issue's, a terminal running the command, and the switch.
struct After {
    main: String,
    dir: String,
    fallback: String,
    /// The command as typed, to offer again; None: no terminal asked for.
    typed: Option<String>,
    /// What the terminal runs: `{issue}` filled in.
    command: Option<String>,
    issue: bool,
    remember: Option<(Remember, String)>,
    switch_to: bool,
}

impl After {
    fn made(self, path: String, window: &mut Window, cx: &mut App) {
        // Remembered for the project once it worked, so its next worktree goes there too.
        crate::store::set_worktree_dir(&self.main, (self.dir != self.fallback).then_some(self.dir.clone()), cx);
        if let Some(c) = &self.typed {
            if self.issue {
                crate::store::set_issue_run(&self.main, c, cx);
            } else {
                crate::store::set_worktree_run(&self.main, c, cx);
            }
        }
        if let Some((remember, branch)) = &self.remember {
            remember(branch, cx);
        }
        WorktreeEvents::emit(WorktreeEvent::Created { path: path.clone(), command: self.command }, cx);
        if self.switch_to {
            open_worktree(path, window, cx);
        }
    }
}

impl Render for NewWorktree {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let branches = self.state.read(cx).branches.clone();
        let typed = self.typed(cx);
        let pull = self.pull().cloned();
        let issue = self.issue().cloned();
        let picked = worktree_branch(&typed, &branches, FOLD_CASE);
        // A pull request's branch is the PR's: nothing to check or pick a base for.
        let existing = pull.is_some() || picked.base != WorktreeBase::Picked;
        let n = pull.as_ref().map_or(picked.name.clone(), |p| p.branch.clone());
        let guarded = pull.is_none()
            && existing
            && !picked.taken
            && Some(n.as_str()) == default_branch(&branches).as_deref();
        let warning = self.warned && guarded;
        let ready = !n.is_empty() && (pull.is_some() || !picked.taken) && !self.pending && !warning;
        let weak = cx.weak_entity();
        let about = match (&pull, &issue) {
            (Some(p), _) => format!("The pull request's branch, {}, in its own folder; this one stays as it is.", p.branch),
            (None, Some(i)) => format!("A new branch for “{}”, in its own folder; its pull request will close #{}.", i.title, i.number),
            _ => "A new branch, checked out in its own folder, side by side with this one.".into(),
        };
        let command_typed = self.command.read(cx).value().to_string();
        let issue_hint = (self.terminal && (issue.is_some() || command_typed.contains("{issue}"))).then(|| match &issue {
            Some(i) => format!("{{issue}} in the command becomes {}, the issue's number.", i.number),
            None => "{issue} is filled in only when starting from an issue.".to_string(),
        });
        let base: Option<gpui_kit::AnyElement> = if existing {
            None
        } else if is_commit(&self.from) {
            Some(
                note(format!("From commit {}", &self.from[..7]), cx)
                    .mt_3()
                    .into_any_element(),
            )
        } else {
            let tags = self.tags.clone();
            let (current, head) = (self.from.clone(), self.head_option);
            let label = if self.from == "HEAD" {
                "HEAD".to_string()
            } else {
                short_ref(&self.from).to_string()
            };
            let pick_to = weak.clone();
            Some(
                div()
                    .mt_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(note("From", cx))
                    .child(
                        Button::new("wt-base")
                            .small()
                            .outline()
                            .label(label)
                            .dropdown_caret(true)
                            .dropdown_menu(move |menu, _, _| {
                                let to = pick_to.clone();
                                ref_menu(
                                    menu,
                                    &branches,
                                    &tags,
                                    head,
                                    &current,
                                    Rc::new(move |r, _, cx| {
                                        let _ = to.update(cx, |this, cx| {
                                            this.from = r;
                                            cx.notify();
                                        });
                                    }),
                                )
                            }),
                    )
                    .into_any_element(),
            )
        };
        let shown_dir = format!(
            "{}/{}",
            short_path(&self.dir, &self.main),
            if n.is_empty() {
                "…".to_string()
            } else {
                folder_for_branch(&n)
            }
        );
        let picked_base = match &picked.base {
            WorktreeBase::Ref(r) => Some(r.clone()),
            _ => None,
        };
        let n_off = n.clone();
        div()
            .flex()
            .flex_col()
            .text_size(rems(0.75))
            .track_focus(&self.focus)
            .when(pull.is_some(), |d| {
                d.on_key_down(cx.listener(|this, e: &gpui_kit::KeyDownEvent, window, cx| {
                    if e.keystroke.key == "enter" {
                        cx.stop_propagation();
                        this.submit(window, cx);
                    }
                }))
            })
            .child(note(about, cx))
            .when(pull.is_none(), |d| {
                d.child(div().mt_3().child(Input::new(&self.name))).children(hint(picked.hint.as_deref(), picked.taken, cx))
            })
            .children(base)
            .child(
                div()
                    .mt_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(note("Folder", cx))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().flex_1().min_w_0().truncate().font_family(t.fonts.code.clone()).child(shown_dir))
                            .when(self.dir != self.fallback, |d| {
                                d.child(Button::new("wt-dir-default").small().ghost().label("Default").on_click(cx.listener(|this, _, _, cx| {
                                    this.dir = this.fallback.clone();
                                    cx.notify();
                                })))
                            })
                            .child(Button::new("wt-dir").small().outline().label("Change…").on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx)))),
                    )
                    .when(self.includes > 0, |d| d.child(note(format!("Copies {} listed in .worktreeinclude", plural(self.includes, "ignored file")), cx))),
            )
            .child(
                div()
                    .mt_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Checkbox::new("wt-terminal").label("Open a terminal in it").checked(self.terminal).on_click(cx.listener(|this, c: &bool, _, cx| {
                        this.terminal = *c;
                        cx.notify();
                    })))
                    .child(div().flex_1().child(Input::new(&self.command).disabled(!self.terminal))),
            )
            .children(issue_hint.map(|h| note(h, cx).mt_1p5()))
            .when(warning, |d| {
                d.child(
                    div()
                        .mt_3()
                        .p_2()
                        .rounded(px(6.))
                        .bg(t.removed.opacity(0.1))
                        .child(format!("{n} is best kept in the main folder, {}: a worktree that holds it blocks switching to it there.", folder_name(&self.main)))
                        .child(
                            div()
                                .mt_2()
                                .flex()
                                .justify_end()
                                .gap_2()
                                .child(Button::new("wt-branch-off").small().outline().label("Create from a new branch instead").on_click(cx.listener(
                                    move |this, _, window, cx| this.branch_off(picked_base.clone(), &n_off, window, cx),
                                )))
                                .child(Button::new("wt-anyway").small().ghost().label("Create anyway").on_click(cx.listener(|this, _, window, cx| {
                                    let branches = this.state.read(cx).branches.clone();
                                    let picked = worktree_branch(&this.typed(cx), &branches, FOLD_CASE);
                                    this.create(picked, window, cx);
                                }))),
                        ),
                )
            })
            .child(
                div()
                    .mt_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(Checkbox::new("wt-switch").label("Switch to it").checked(self.switch_to).on_click(cx.listener(|this, c: &bool, _, cx| {
                        this.switch_to = *c;
                        cx.notify();
                    })))
                    .child(div().flex_1())
                    .child(Button::new("wt-create").primary().label(if pull.is_some() { "Check out" } else { "Create" }).disabled(!ready).on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))),
            )
    }
}

// MARK: Rename

pub struct RenameWorktree {
    state: Entity<RepoState>,
    worktree: Worktree,
    main: String,
    name: Entity<InputState>,
    moving: bool,
    pending: bool,
    _task: Option<Task<()>>,
    _subs: Vec<Subscription>,
    _live: Live,
}

pub fn open_rename(w: Worktree, window: &mut Window, cx: &mut App) {
    let Some(state) = state_of(window, cx) else {
        return;
    };
    let form = cx.new(|cx| {
        let old = w.branch.clone().unwrap_or_default();
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("New name")
                .default_value(old)
        });
        let subs = vec![cx.subscribe_in(
            &name,
            window,
            |this: &mut RenameWorktree, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                _ => cx.notify(),
            },
        )];
        RenameWorktree {
            main: main_of(&state, cx),
            moving: stays(&w).is_none(),
            state,
            worktree: w,
            name,
            pending: false,
            _task: None,
            _subs: subs,
            _live: Live::new("RenameWorktree"),
        }
    });
    let focus = form.read(cx).name.focus_handle(cx);
    open_form("Rename worktree", 460., form, Some(focus), window, cx);
}

/// Why its folder can't move: rename_worktree's own checks.
fn stays(w: &Worktree) -> Option<&'static str> {
    if w.main {
        Some("The main worktree's folder stays where it is.")
    } else if w.current {
        Some("This window has it open; switch to another worktree to move its folder.")
    } else if w.locked {
        Some("It's locked; unlock it (the lock on its row) to move its folder.")
    } else {
        None
    }
}

impl RenameWorktree {
    fn plan(&self, cx: &App) -> (crate::names::NameCheck, bool, String) {
        let old = self.worktree.branch.clone().unwrap_or_default();
        let check = ref_name_check(
            &self.name.read(cx).value(),
            &local_names(&self.state.read(cx).branches, Some(&old)),
            true,
            FOLD_CASE,
        );
        let target = format!(
            "{}{}",
            parent_folder(&self.worktree.path),
            folder_for_branch(&check.name)
        );
        let moving = self.moving
            && stays(&self.worktree).is_none()
            && !check.name.is_empty()
            && target != self.worktree.path;
        (check, moving, old)
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (check, moving, old) = self.plan(cx);
        let n = check.name.clone();
        if n.is_empty() || (n == old && !moving) || check.taken || self.pending {
            return;
        }
        let from = self.worktree.path.clone();
        let done = if n == old {
            format!("Moved {} to {}", folder_name(&from), folder_for_branch(&n))
        } else {
            format!(
                "Renamed {old} to {n}{}",
                if moving { ", folder too" } else { "" }
            )
        };
        // The color it shows now, which its new name could change.
        let picked = crate::store::colors(cx);
        let list: Vec<(String, bool)> = self
            .state
            .read(cx)
            .worktrees
            .iter()
            .map(|w| (w.path.clone(), w.main))
            .collect();
        let hue = super::worktree_hues(&list, &picked)
            .get(&from)
            .copied()
            .flatten();
        let had_pick = picked.contains_key(&from);
        let made = Arc::new(Mutex::new(None::<String>));
        let slot = made.clone();
        let path = from.clone();
        self.pending = true;
        let rx = self.state.update(cx, |s, cx| {
            s.run(
                GitAction::new("Rename worktree").untracked().done(done),
                move |core, _| {
                    let to = api::worktrees::rename_worktree(core, path, n, moving)?;
                    *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(to);
                    Ok(false)
                },
                window,
                cx,
            )
        });
        let handle = window.window_handle();
        self._task = Some(cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            let to = made.lock().unwrap_or_else(|e| e.into_inner()).take();
            let _ = handle.update(cx, |_, window, cx| {
                if !ok {
                    let _ = this.update(cx, |this, cx| {
                        this.pending = false;
                        cx.notify();
                    });
                    return;
                }
                if let Some(to) = to.filter(|t| *t != from) {
                    crate::store::move_root(&from, &to, cx);
                    // Kept as it showed, rather than the new name's.
                    if !had_pick && let Some(h) = hue {
                        crate::store::set_color(&to, Some(super::hue_name(h)), cx);
                    }
                }
                crate::askpass::close_form(window, cx);
            });
        }));
    }
}

impl Render for RenameWorktree {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let (check, moving, old) = self.plan(cx);
        let terminals = if moving {
            crate::hooks::terminals_in(&self.worktree.path, cx)
        } else {
            0
        };
        let upstream = self
            .state
            .read(cx)
            .branches
            .iter()
            .find(|b| !b.remote && b.name == old)
            .and_then(|b| b.upstream.clone());
        let ready = !check.name.is_empty()
            && (check.name != old || moving)
            && !check.taken
            && !self.pending;
        let mut text = format!(
            "Renames the branch {old} checked out in {}.",
            short_path(&self.worktree.path, &self.main)
        );
        if let Some(u) = upstream {
            text.push_str(&format!(
                " It keeps tracking {u}; the branch menu can rename that too."
            ));
        }
        let stay = stays(&self.worktree);
        let detail = stay.map(str::to_string).unwrap_or_else(|| {
            format!(
                "{} → {}",
                short_path(&self.worktree.path, &self.main),
                if check.name.is_empty() {
                    "…".into()
                } else {
                    folder_for_branch(&check.name)
                }
            )
        });
        div()
            .flex()
            .flex_col()
            .text_size(rems(0.75))
            .child(note(text, cx))
            .child(div().mt_3().child(Input::new(&self.name)))
            .children(hint(check.hint.as_deref(), check.taken, cx))
            .child(
                div()
                    .mt_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        Checkbox::new("wt-move").label("Rename its folder to match").checked(self.moving && stay.is_none()).disabled(stay.is_some()).on_click(
                            cx.listener(|this, c: &bool, _, cx| {
                                this.moving = *c;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(note(detail, cx).pl_6()),
            )
            .when(terminals > 0, |d| {
                let who = if terminals == 1 { "A terminal runs".to_string() } else { format!("{terminals} terminals run") };
                d.child(div().mt_3().p_2().rounded(px(6.)).bg(t.removed.opacity(0.1)).text_color(t.removed).child(format!(
                    "{who} in this folder. The shell moves with it, but whatever holds on to the old path, like an agent or a dev server, may lose track of its files."
                )))
            })
            .child(div().mt_4().flex().justify_end().child(
                Button::new("wt-rename").primary().label(if terminals > 0 { "Rename and move" } else { "Rename" }).disabled(!ready).on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            ))
    }
}

// MARK: Lock

pub struct LockWorktree {
    state: Entity<RepoState>,
    worktree: Worktree,
    main: String,
    reason: Entity<InputState>,
    pending: bool,
    _task: Option<Task<()>>,
    _subs: Vec<Subscription>,
    _live: Live,
}

pub fn open_lock(w: Worktree, window: &mut Window, cx: &mut App) {
    let Some(state) = state_of(window, cx) else {
        return;
    };
    let form = cx.new(|cx| {
        let reason = cx.new(|cx| InputState::new(window, cx).placeholder("Reason (optional)"));
        let subs = vec![cx.subscribe_in(
            &reason,
            window,
            |this: &mut LockWorktree, _, e: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = e {
                    this.submit(window, cx);
                }
            },
        )];
        LockWorktree {
            main: main_of(&state, cx),
            state,
            worktree: w,
            reason,
            pending: false,
            _task: None,
            _subs: subs,
            _live: Live::new("LockWorktree"),
        }
    });
    let focus = form.read(cx).reason.focus_handle(cx);
    open_form("Lock worktree", 440., form, Some(focus), window, cx);
}

impl LockWorktree {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        self.pending = true;
        let r = self.reason.read(cx).value().trim().to_string();
        let w = self.worktree.clone();
        let rx =
            crate::ops::lock_worktree(&self.state, &w, (!r.is_empty()).then_some(r), window, cx);
        let handle = window.window_handle();
        self._task = Some(cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            let _ = handle.update(cx, |_, window, cx| {
                if ok {
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

impl Render for LockWorktree {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .text_size(rems(0.75))
            .child(note(
                format!(
                    "Keeps {} from being pruned, moved or removed until it's unlocked, as for a folder on a drive that isn't always plugged in.",
                    short_path(&self.worktree.path, &self.main)
                ),
                cx,
            ))
            .child(div().mt_3().child(Input::new(&self.reason)))
            .child(div().mt_4().flex().justify_end().child(
                Button::new("wt-lock").primary().label("Lock").disabled(self.pending).on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            ))
    }
}

// MARK: Clean up

/// Ignored entries named before the rest are counted.
const NAMED: usize = 4;

enum Read<T> {
    Ok(T),
    Failed(String),
}

pub struct CleanUp {
    state: Entity<RepoState>,
    main: String,
    list: Vec<Cleanable>,
    states: HashMap<String, Read<u32>>,
    ignored: HashMap<String, Read<git::IgnoredFiles>>,
    unpicked: std::collections::HashSet<String>,
    pending: bool,
    _reads: Vec<Task<()>>,
    _task: Option<Task<()>>,
    _live: Live,
}

/// Clean up: merged worktrees removed together, each with its branch where git (or the merged
/// pull request's head) shows nothing on it is lost. Their ignored files aren't changes to git,
/// so `git worktree remove` deletes them without asking: they're listed here.
pub fn open_clean_up(list: Vec<Cleanable>, window: &mut Window, cx: &mut App) {
    let Some(state) = state_of(window, cx) else {
        return;
    };
    let form = cx.new(|cx| CleanUp::new(state, list, cx));
    open_form("Clean up merged worktrees", 520., form, None, window, cx);
}

impl CleanUp {
    fn new(state: Entity<RepoState>, list: Vec<Cleanable>, cx: &mut Context<Self>) -> Self {
        let core = gitviber_ui::core(cx);
        let mut reads = vec![];
        // What's uncommitted now, all at once; then the ignored files one at a time, as each walk sizes folders.
        for c in &list {
            let (core, path) = (core.clone(), c.worktree.path.clone());
            let key = path.clone();
            let read =
                cx.background_spawn(
                    async move { api::worktrees::worktree_state(&core, path, false) },
                );
            reads.push(cx.spawn(async move |this, cx| {
                let s = read.await;
                let _ = this.update(cx, |this, cx| {
                    this.states.insert(
                        key,
                        match s {
                            Ok(s) => Read::Ok(s.uncommitted),
                            Err(e) => Read::Failed(e),
                        },
                    );
                    cx.notify();
                });
            }));
        }
        let paths: Vec<String> = list.iter().map(|c| c.worktree.path.clone()).collect();
        reads.push(cx.spawn(async move |this, cx| {
            for path in paths {
                let (core, p) = (core.clone(), path.clone());
                let read = cx
                    .background_spawn(async move { api::worktrees::worktree_ignored(&core, p) })
                    .await;
                if this
                    .update(cx, |this, cx| {
                        this.ignored.insert(
                            path,
                            match read {
                                Ok(i) => Read::Ok(i),
                                Err(e) => Read::Failed(e),
                            },
                        );
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        CleanUp {
            main: main_of(&state, cx),
            state,
            list,
            states: HashMap::new(),
            ignored: HashMap::new(),
            unpicked: Default::default(),
            pending: false,
            _reads: reads,
            _task: None,
            _live: Live::new("CleanUp"),
        }
    }

    fn blocked(&self, c: &Cleanable, cx: &App) -> Option<String> {
        let w = &c.worktree;
        if w.locked {
            return Some(format!(
                "Locked{}",
                w.lock_reason
                    .as_deref()
                    .map(|r| format!(": {r}"))
                    .unwrap_or_default()
            ));
        }
        match crate::hooks::terminals_in(&w.path, cx) {
            0 => {}
            1 => return Some("A terminal runs in it".into()),
            n => return Some(format!("{n} terminals run in it")),
        }
        match self.states.get(&w.path) {
            Some(Read::Failed(e)) => return Some(e.clone()),
            Some(Read::Ok(n)) if *n > 0 => {
                return Some(format!(
                    "{}: remove it from its row to drop them",
                    plural(*n as usize, "uncommitted change")
                ));
            }
            _ => {}
        }
        if let Some(Read::Ok(i)) = self.ignored.get(&w.path)
            && let Some(d) = i.denied.first()
        {
            return Some(if d == "./" {
                "Its folder can't be deleted (permission denied)".into()
            } else {
                format!("{d} in it can't be deleted (permission denied)")
            });
        }
        None
    }

    fn chosen(&self, cx: &App) -> Vec<Cleanable> {
        self.list
            .iter()
            .filter(|c| {
                !self.unpicked.contains(&c.worktree.path)
                    && self.blocked(c, cx).is_none()
                    && self.states.contains_key(&c.worktree.path)
            })
            .cloned()
            .collect()
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Checked again here and in core: the list may be minutes old.
        let items: Vec<git::CleanUp> = self
            .chosen(cx)
            .into_iter()
            .filter(|c| crate::hooks::terminals_in(&c.worktree.path, cx) == 0)
            .map(|c| git::CleanUp {
                path: c.worktree.path,
                merged_head: c.merged_head,
            })
            .collect();
        if items.is_empty() || self.pending {
            return;
        }
        self.pending = true;
        let out = Arc::new(Mutex::new(None::<(git::CleanedUp, Option<u64>)>));
        let slot = out.clone();
        let rx = self.state.update(cx, |s, cx| {
            s.run(
                GitAction::new("Clean up").untracked(),
                move |core, _| {
                    let before = api::journal::journal_last(core).ok().flatten();
                    let done = api::worktrees::clean_up_worktrees(core, items)?;
                    let after = api::journal::journal_last(core)
                        .ok()
                        .flatten()
                        .filter(|a| Some(*a) != before);
                    *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some((done, after));
                    Ok(false)
                },
                window,
                cx,
            )
        });
        let (handle, state) = (window.window_handle(), self.state.downgrade());
        self._task = Some(cx.spawn(async move |this, cx| {
            let ok = rx.await.unwrap_or(false);
            let result = out.lock().unwrap_or_else(|e| e.into_inner()).take();
            let _ = handle.update(cx, |_, window, cx| {
                if let Some((done, entry)) = result {
                    report(done, entry.map(|id| undo_action(state.clone(), id))).show(window, cx);
                }
                if ok {
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

fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let (mut v, mut i) = (n as f64, 0);
    // 1023.6 KB would round to "1024 KB": past 1023.5 it's the next unit's.
    while v >= if i > 0 { 1023.5 } else { 1024. } && i < UNITS.len() - 1 {
        v /= 1024.;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else if v < 9.95 {
        format!("{v:.1} {}", UNITS[i])
    } else {
        format!("{v:.0} {}", UNITS[i])
    }
}

/// "Deletes ignored node_modules/ (312 MB), .env (9 B) and 3 more · 315 MB in all".
fn ignored_line(i: &git::IgnoredFiles) -> String {
    if i.entries.is_empty() {
        return "No ignored files in it".into();
    }
    let total: u64 = i.entries.iter().map(|e| e.bytes).sum();
    let named = i
        .entries
        .iter()
        .take(NAMED)
        .map(|e| format!("{} ({})", e.path, format_bytes(e.bytes)))
        .collect::<Vec<_>>()
        .join(", ");
    let more = if i.entries.len() > NAMED {
        format!(" and {} more", i.entries.len() - NAMED)
    } else {
        String::new()
    };
    format!(
        "Deletes ignored {named}{more} · {}{} in all",
        if i.complete { "" } else { "at least " },
        format_bytes(total)
    )
}

fn report(done: git::CleanedUp, undo: Option<crate::toast::Action>) -> Note {
    let git::CleanedUp {
        removed,
        deleted,
        kept,
        failed,
    } = done;
    let mut lines = vec![];
    if !deleted.is_empty() {
        lines.push(format!(
            "Deleted {} {}.",
            if deleted.len() == 1 {
                "branch"
            } else {
                "branches"
            },
            deleted.join(", ")
        ));
    }
    if !kept.is_empty() {
        lines.push(format!(
            "Kept {} {}: moved since, or out in another worktree.",
            if kept.len() == 1 {
                "branch"
            } else {
                "branches"
            },
            kept.join(", ")
        ));
    }
    lines.extend(
        failed
            .iter()
            .map(|(p, why)| format!("Left {}: {why}.", folder_name(p))),
    );
    // The folders went outside the journal, so Undo has only the branches to bring back.
    if undo.is_some() && !removed.is_empty() {
        lines.push("Undo brings back the branches, not the worktree folders.".into());
    }
    let title = if removed.is_empty() {
        "No worktree removed".to_string()
    } else {
        format!("Removed {}", plural(removed.len(), "worktree"))
    };
    let kind = if failed.is_empty() {
        Kind::Success
    } else {
        Kind::Info
    };
    Note::new(kind, title)
        .maybe_detail((!lines.is_empty()).then(|| lines.join("\n")))
        .action(undo)
}

impl Render for CleanUp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let chosen = self.chosen(cx).len();
        let rows = self.list.clone().into_iter().enumerate().map(|(i, c)| {
            let w = c.worktree.clone();
            let why = self.blocked(&c, cx);
            let read = self.states.contains_key(&w.path);
            let line = why
                .clone()
                .unwrap_or_else(|| match self.ignored.get(&w.path) {
                    Some(Read::Ok(i)) => ignored_line(i),
                    Some(Read::Failed(e)) => format!("Couldn't list its ignored files: {e}"),
                    None => "Looking for ignored files…".into(),
                });
            let path = w.path.clone();
            div()
                .flex()
                .items_start()
                .gap_2()
                .py_1()
                .when(why.is_some(), |d| d.opacity(0.6))
                .child(
                    Checkbox::new(("cleanup", i))
                        .checked(why.is_none() && read && !self.unpicked.contains(&w.path))
                        .disabled(why.is_some() || !read)
                        .on_click(cx.listener(move |this, c: &bool, _, cx| {
                            if *c {
                                this.unpicked.remove(&path);
                            } else {
                                this.unpicked.insert(path.clone());
                            }
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .child(
                                    div().truncate().font_family(t.fonts.code.clone()).child(
                                        w.branch
                                            .clone()
                                            .unwrap_or_else(|| folder_name(&w.path).to_string()),
                                    ),
                                )
                                .child(crate::ui::chip(c.why.clone(), false, cx)),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(rems(0.6875))
                                .text_color(t.subtle)
                                .child(short_path(&w.path, &self.main)),
                        )
                        .child(
                            div()
                                .text_size(rems(0.6875))
                                .text_color(if why.is_some() {
                                    t.removed
                                } else {
                                    t.muted_foreground
                                })
                                .child(line),
                        ),
                )
        });
        let label = if chosen > 1 {
            format!("Remove {}", plural(chosen, "worktree"))
        } else {
            "Remove worktree".into()
        };
        div()
            .flex()
            .flex_col()
            .text_size(rems(0.75))
            .child(note("Removes each folder, and its branch where everything on it is merged. Ignored files in them go too, for good.", cx))
            .child(div().id("cleanup-list").mt_3().max_h(px(320.)).overflow_y_scroll().children(rows))
            .child(div().mt_4().flex().justify_end().child(
                Button::new("wt-cleanup").danger().label(label).disabled(chosen == 0 || self.pending).on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            ))
    }
}
