//! What History, the pickers and the top bar read of the open worktree: its status, branches,
//! worktrees and undo history, read again when the watcher sees a change. One per project,
//! shared by the views that hold it; the lookup keeps only weak handles, so it goes with them.
//!
//! Git actions run through `RepoState::run`: busy while one runs, then the toast (Undo when it
//! recorded an entry, git's error explained when it failed) and a refresh.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use futures::channel::oneshot;
use gitviber_core::network::CANCELLED;
use gitviber_core::{Core, Res, api, git, journal};
use gitviber_ui::Project;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::ProjectEvent;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, EntityId, EventEmitter, Global,
    SharedString, Subscription, Task, WeakEntity, Window,
};

use crate::names::Branch;
use crate::net::{NetActivity, NetOp};
use crate::toast::{Action, Fixes, Note, git_failed};

#[derive(Default)]
struct States(HashMap<EntityId, WeakEntity<RepoState>>);
impl Global for States {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepoEvent {
    /// Status, branches, worktrees or the journal were read again.
    Changed,
    /// An action finished: views that read more (History's log) read again too.
    Acted,
}

pub struct RepoState {
    project: WeakEntity<Project>,
    root: PathBuf,
    pub status: Option<Rc<git::RepoStatus>>,
    pub branches: Rc<Vec<Branch>>,
    pub worktrees: Rc<Vec<git::Worktree>>,
    /// The tags and what each points at, hashed: History reads its log again when one is made,
    /// moved or deleted, here, in a terminal or by a fetch.
    pub tags: u64,
    pub journal: Option<Rc<journal::View>>,
    pub error: Option<SharedString>,
    /// The actions running, oldest first, by their own count: one ending leaves the others busy.
    running: Vec<(u64, SharedString)>,
    /// An undo or redo is asking before it switches: ⌘Z again waits for that answer rather
    /// than stack a second question.
    pub(crate) asking: bool,
    next_run: u64,
    reading: Option<Task<()>>,
    /// A change came in while a read ran: read again once it's done.
    again: Option<bool>,
    actions: Vec<Task<()>>,
    _project: Option<Subscription>,
    _live: Live,
}

impl EventEmitter<RepoEvent> for RepoState {}

fn read(core: &Core, all: bool) -> Read {
    let status = Some(api::changes::status(core));
    if !all {
        return (status, None, None, None, None);
    }
    let branches =
        Some(api::branches::branches(core).map(|b| b.into_iter().map(Branch::from).collect()));
    let worktrees = Some(api::worktrees::worktrees(core));
    let tags = Some(tags_hash(core));
    let journal = Some(api::journal::journal(core));
    (status, branches, worktrees, tags, journal)
}

/// Every tag with the object it names, hashed: one cheap `for-each-ref`, so a refresh that
/// changed no tag costs History nothing.
fn tags_hash(core: &Core) -> Res<u64> {
    use std::hash::{Hash as _, Hasher as _};
    let out = core.read_repo(|r| {
        git::run(
            r,
            &[
                "for-each-ref",
                "--format=%(objectname) %(refname)",
                "refs/tags",
            ],
        )
    })?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    out.hash(&mut h);
    Ok(h.finish())
}

/// Rc so views can hold what they draw without copying it.
type Read = (
    Option<Res<git::RepoStatus>>,
    Option<Res<Vec<Branch>>>,
    Option<Res<Vec<git::Worktree>>>,
    Option<Res<u64>>,
    Option<Res<journal::View>>,
);

impl RepoState {
    /// The project's state, made the first time a view asks.
    pub fn get(project: &Entity<Project>, cx: &mut App) -> Entity<RepoState> {
        let id = project.entity_id();
        let states = cx.default_global::<States>();
        states.0.retain(|_, s| s.upgrade().is_some());
        if let Some(state) = states.0.get(&id).and_then(|s| s.upgrade()) {
            return state;
        }
        let project = project.clone();
        let state = cx.new(|cx| RepoState::new(&project, cx));
        cx.default_global::<States>()
            .0
            .insert(id, state.downgrade());
        state
    }

    /// The project's state if some view made it; none is made.
    pub fn existing(project: &Entity<Project>, cx: &App) -> Option<Entity<RepoState>> {
        cx.try_global::<States>()?
            .0
            .get(&project.entity_id())?
            .upgrade()
    }

    fn new(project: &Entity<Project>, cx: &mut Context<Self>) -> Self {
        let sub = cx.subscribe(
            project,
            |this: &mut Self, project, event: &ProjectEvent, cx| match event {
                ProjectEvent::RepoChanged(change) => this.refresh_parts(change.git, cx),
                ProjectEvent::Opened | ProjectEvent::Switched { .. } => {
                    this.root = project.read(cx).root().to_path_buf();
                    // The last worktree's: its undo history and HEAD aren't this one's.
                    this.journal = None;
                    this.status = None;
                    this.refresh(cx);
                }
                ProjectEvent::Failed(e) => {
                    this.error = Some(e.clone().into());
                    cx.notify();
                }
            },
        );
        let mut state = RepoState {
            project: project.downgrade(),
            root: project.read(cx).root().to_path_buf(),
            status: None,
            branches: Rc::default(),
            worktrees: Rc::default(),
            tags: 0,
            journal: None,
            error: None,
            running: Vec::new(),
            asking: false,
            next_run: 0,
            reading: None,
            again: None,
            actions: Vec::new(),
            _project: Some(sub),
            _live: Live::new("RepoState"),
        };
        if project.read(cx).is_open() {
            state.refresh(cx);
        }
        state
    }

    pub fn project(&self) -> Option<Entity<Project>> {
        self.project.upgrade()
    }

    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    /// The branch checked out; None when detached.
    pub fn branch(&self) -> Option<&str> {
        self.status.as_ref()?.branch.as_deref()
    }

    /// HEAD's commit, once read.
    pub fn head(&self) -> Option<&str> {
        self.status.as_ref()?.head.as_deref()
    }

    /// The newest action running.
    pub fn busy(&self) -> Option<&SharedString> {
        self.running.last().map(|(_, label)| label)
    }

    /// An action is running or a merge/rebase/revert waits: nothing else may move HEAD.
    pub fn locked(&self) -> bool {
        !self.running.is_empty() || self.status.as_ref().is_some_and(|s| s.operation.is_some())
    }

    fn started(&mut self, label: SharedString, cx: &mut Context<Self>) -> u64 {
        self.next_run += 1;
        self.running.push((self.next_run, label));
        cx.notify();
        self.next_run
    }

    fn ended(&mut self, run: u64) {
        self.running.retain(|(id, _)| *id != run);
    }

    /// A toast's button, run only while it still means what it said: in the worktree it was
    /// for, with nothing else running.
    fn guard(&self, action: Action, cx: &Context<Self>) -> Action {
        let (state, root) = (cx.weak_entity(), self.root.clone());
        let label = action.label.clone();
        Action::new(label.clone(), move |window, cx| {
            let state = state.upgrade();
            let now = state.as_ref().map(|s| s.read(cx));
            match now {
                Some(s) if *s.root() == root => match s.busy() {
                    Some(b) => Note::info(format!("{label} not run"))
                        .detail(format!("Wait for {b} to finish."))
                        .show(window, cx),
                    None => (action.run)(window, cx),
                },
                _ => Note::info(format!("{label} not run"))
                    .detail("It was for the worktree open before this one.")
                    .show(window, cx),
            }
        })
    }

    pub(crate) fn guarded(&self, note: Note, cx: &Context<Self>) -> Note {
        let action = note.action.clone().map(|a| self.guard(a, cx));
        let also = note.also.clone().map(|a| self.guard(a, cx));
        note.action(action).also(also)
    }

    pub fn current_worktree(&self) -> Option<&git::Worktree> {
        self.worktrees.iter().find(|w| w.current)
    }

    /// Reads everything again.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_parts(true, cx);
    }

    /// `git`: HEAD, refs or the index moved, so branches, worktrees and the journal may have too;
    /// else only the working tree changed, and status says all of it.
    fn refresh_parts(&mut self, git: bool, cx: &mut Context<Self>) {
        if self.reading.is_some() {
            self.again = Some(self.again.unwrap_or(false) || git);
            return;
        }
        let core = gitviber_ui::core(cx);
        let all = git || self.status.is_none();
        let read = cx.background_spawn(async move { read(&core, all) });
        self.reading = Some(cx.spawn(async move |this, cx| {
            let read: Read = read.await;
            let _ = this.update(cx, |this, cx| {
                this.reading = None;
                this.apply(read, cx);
                if let Some(git) = this.again.take() {
                    this.refresh_parts(git, cx);
                }
            });
        }));
    }

    /// Reads everything now, on this thread: for a demo's first frame, which an occluded window
    /// may be the only one it draws. The UI never calls it.
    #[doc(hidden)]
    pub fn load_now(&mut self, cx: &mut Context<Self>) {
        let core = gitviber_ui::core(cx);
        self.root = self
            .project
            .upgrade()
            .map(|p| p.read(cx).root().to_path_buf())
            .unwrap_or_default();
        let r = read(&core, true);
        self.apply(r, cx);
    }

    fn apply(
        &mut self,
        (status, branches, worktrees, tags, journal): Read,
        cx: &mut Context<Self>,
    ) {
        match status {
            // Read while a switch moved core to another worktree: that one's reads come next.
            Some(Ok(s)) if std::path::Path::new(&s.root) != self.root => return,
            Some(Ok(s)) => {
                self.status = Some(Rc::new(s));
                self.error = None;
            }
            Some(Err(e)) => self.error = Some(e.into()),
            None => {}
        }
        if let Some(Ok(b)) = branches {
            self.branches = Rc::new(b);
        }
        if let Some(Ok(w)) = worktrees {
            self.worktrees = Rc::new(w);
        }
        if let Some(Ok(t)) = tags {
            self.tags = t;
        }
        if let Some(Ok(j)) = journal {
            self.journal = Some(Rc::new(j));
        }
        cx.emit(RepoEvent::Changed);
        cx.notify();
    }

    /// Runs a git action: `work` gets core and, for a network command, its op (progress in the
    /// top bar, Cancel). Resolves to whether it went through; the toast and refresh follow
    /// either way. `window`: where the toasts go.
    pub fn run(
        &mut self,
        action: GitAction,
        work: impl FnOnce(&Core, &NetOp) -> Res<bool> + Send + 'static,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<bool> {
        let (tx, rx) = oneshot::channel();
        let handle = window.window_handle();
        let core = gitviber_ui::core(cx);
        let label = action.label.clone();
        let run = self.started(label.clone(), cx);
        let activity = action.net.then(|| NetActivity::global(cx));
        let root = self.root.clone();
        let op = match &activity {
            Some(a) => a.update(cx, |a, cx| a.start(label.clone(), cx)),
            None => NetOp::quiet(),
        }
        .pinned(root);
        let tracked = action.tracked;
        let op_id = op.id.clone();
        let task = cx.background_spawn(async move {
            // Core has one repo open: one switched to meanwhile isn't this action's to touch.
            if let Err(e) = op.still_in(&core) {
                return (Err(e), None);
            }
            let before = if tracked {
                api::journal::journal_last(&core).ok().flatten()
            } else {
                None
            };
            let result = work(&core, &op);
            let after = if tracked {
                api::journal::journal_last(&core).ok().flatten()
            } else {
                None
            };
            (result, after.filter(|a| Some(*a) != before))
        });
        let weak = cx.weak_entity();
        self.actions.retain(|t| !t.is_ready());
        self.actions.push(cx.spawn(async move |this, cx| {
            let (result, entry) = task.await;
            let on_error = action.on_error.clone();
            let deferred = this.update(cx, |this, cx| {
                if let Some(a) = &activity {
                    a.update(cx, |a, cx| a.finish(&op_id, cx));
                }
                this.ended(run);
                if action.net
                    && let Some((title, body)) = net_note(&action.label, &result)
                {
                    gitviber_ui::notify_if_away(|s| s.notify_git, title, body, None, cx);
                }
                let ok = result.is_ok();
                let mut deferred = None;
                let note = match result {
                    Err(e) if e != CANCELLED && on_error.is_some() => {
                        deferred = Some(e);
                        None
                    }
                    Ok(true) => Some(
                        Note::info(format!("{} stopped on conflicts", action.label)).detail(
                            action.conflicts.clone().unwrap_or_else(|| {
                                "Resolve them in Changes, then continue.".into()
                            }),
                        ),
                    ),
                    Ok(false) if tracked && entry.is_none() && action.nothing.is_some() => {
                        action.nothing.clone().map(Note::info)
                    }
                    Ok(false) => action.done.clone().map(|done| {
                        Note::success(done)
                            .maybe_detail(action.detail.clone())
                            .action(entry.map(|id| undo_action(weak.clone(), id)))
                    }),
                    Err(e) if e == CANCELLED => {
                        Some(Note::info(format!("{} cancelled", action.label)))
                    }
                    Err(e) => Some(git_failed(
                        format!("{} failed", action.label),
                        &e,
                        &action.fixes,
                    )),
                };
                if let Some(note) = note {
                    this.guarded(note, cx).show_in(handle, cx);
                }
                this.refresh(cx);
                cx.emit(RepoEvent::Acted);
                let _ = tx.send(ok);
                deferred
            });
            // After the update: the handler may run another action on this state.
            if let (Ok(Some(e)), Some(on_error)) = (deferred, on_error) {
                let fixes = action.fixes.clone();
                let title = format!("{} failed", action.label);
                let handled = cx.update(|cx| {
                    handle
                        .update(cx, |_, window, cx| on_error(&e, window, cx))
                        .unwrap_or(false)
                });
                if !handled {
                    let _ = this.update(cx, |this, cx| {
                        this.guarded(git_failed(title, &e, &fixes), cx)
                            .show_in(handle, cx)
                    });
                }
            }
        }));
        rx
    }

    /// Undoes (or with `forward`, redoes) the entries `ids` in order, each the next one at its
    /// turn; core refuses any that isn't. Stops at the first that can't be done.
    pub fn travel(
        &mut self,
        forward: bool,
        ids: Vec<u64>,
        handle: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        // ⌘Z held, or a toast's Undo while an action runs: one at a time.
        if let Some(b) = self.busy().cloned() {
            let verb = if forward { "Redo" } else { "Undo" };
            Note::info(format!("{verb} not run"))
                .detail(format!("Wait for {b} to finish."))
                .show_in(handle, cx);
            return;
        }
        let run = self.started(
            if forward {
                "Redo".into()
            } else {
                "Undo".into()
            },
            cx,
        );
        let core = gitviber_ui::core(cx);
        let op = NetOp::quiet().pinned(self.root.clone());
        let task = cx.background_spawn(async move {
            if let Err(e) = op.still_in(&core) {
                return (vec![], Some(e), ids.len());
            }
            let mut done = vec![];
            for id in &ids {
                let step = if forward {
                    api::journal::redo(&core, Some(*id))
                } else {
                    api::journal::undo(&core, Some(*id))
                };
                match step {
                    Ok(step) => done.push((*id, step)),
                    Err(e) => return (done, Some(e), ids.len()),
                }
            }
            (done, None, ids.len())
        });
        let weak = cx.weak_entity();
        self.actions.retain(|t| !t.is_ready());
        self.actions.push(cx.spawn(async move |this, cx| {
            let (done, error, asked) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.ended(run);
                let label = done
                    .last()
                    .map(|(_, s)| s.label.clone())
                    .unwrap_or_default();
                if let Some(e) = error {
                    let what = if forward {
                        "Could not redo"
                    } else {
                        "Could not undo"
                    };
                    let title = if done.is_empty() {
                        what.to_string()
                    } else {
                        format!("{what} past {label}")
                    };
                    this.guarded(git_failed(title, &e, &Fixes::new()), cx)
                        .show_in(handle, cx);
                } else if done.len() == asked {
                    let verb = if forward { "Redone" } else { "Undone" };
                    // Where HEAD ends up, when a step switched branches: ⌘Z must not switch unsaid.
                    let switched = done.iter().rev().find_map(|(_, s)| s.switch_to.clone());
                    let conflicts: Vec<String> =
                        done.iter().flat_map(|(_, s)| s.conflicts.clone()).collect();
                    let back: Vec<u64> = done.iter().rev().map(|(id, _)| *id).collect();
                    let detail = [
                        switched.map(|b| {
                            format!("Switched {}to {b}.", if forward { "" } else { "back " })
                        }),
                        (!conflicts.is_empty())
                            .then(|| format!("Conflicts are marked in {}.", conflicts.join(", "))),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                    let title = if done.len() == 1 {
                        format!("{verb}: {label}")
                    } else {
                        format!("{verb} {} actions", done.len())
                    };
                    let weak = weak.clone();
                    let again =
                        Action::new(if forward { "Undo" } else { "Redo" }, move |window, cx| {
                            let handle = window.window_handle();
                            let back = back.clone();
                            let _ = weak
                                .update(cx, |state, cx| state.travel(!forward, back, handle, cx));
                        });
                    let note = if conflicts.is_empty() {
                        Note::success(title)
                    } else {
                        Note::info(title)
                    };
                    let note = note
                        .maybe_detail((!detail.is_empty()).then_some(detail))
                        .action(Some(again));
                    this.guarded(note, cx).show_in(handle, cx);
                }
                this.refresh(cx);
                cx.emit(RepoEvent::Acted);
            });
        }));
    }
}

/// What a network action that ended says, for a notification: done, or failed with git's words.
/// None for one the user stopped.
pub fn net_note(label: &str, result: &Res<bool>) -> Option<(String, String)> {
    match result {
        Err(e) if e == CANCELLED || e == git::CANCELLED_STASHED => None,
        Err(e) => Some((format!("{label} failed"), e.clone())),
        Ok(_) => Some((format!("{label} finished"), String::new())),
    }
}

/// The Undo button of a success toast, for the entry the action recorded.
pub fn undo_action(state: WeakEntity<RepoState>, id: u64) -> Action {
    Action::new("Undo", move |window, cx| {
        let handle = window.window_handle();
        let _ = state.update(cx, |state, cx| state.travel(false, vec![id], handle, cx));
    })
}

/// What a git action is called, what it says when done, and how its failures are met.
#[derive(Clone)]
pub struct GitAction {
    pub label: SharedString,
    /// The success toast; none without it.
    pub done: Option<String>,
    pub detail: Option<String>,
    /// What to do after it stops on conflicts.
    pub conflicts: Option<String>,
    /// Said instead of `done` when it recorded no undo entry: it moved nothing.
    pub nothing: Option<String>,
    /// It records an undo entry (the journal), so its toast offers Undo.
    pub tracked: bool,
    /// A network command: its progress shows in the top bar, and Cancel stops it.
    pub net: bool,
    pub fixes: Fixes,
    /// Sees a failure first; true when it took care of it (asked to force, stashed and retried),
    /// and then no error toast shows.
    pub on_error: Option<OnError>,
}

pub type OnError = Rc<dyn Fn(&str, &mut Window, &mut App) -> bool>;

impl GitAction {
    pub fn new(label: impl Into<SharedString>) -> Self {
        GitAction {
            label: label.into(),
            done: None,
            detail: None,
            conflicts: None,
            nothing: None,
            tracked: true,
            net: false,
            fixes: vec![],
            on_error: None,
        }
    }
    pub fn done(mut self, done: impl Into<String>) -> Self {
        self.done = Some(done.into());
        self
    }
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
    pub fn conflicts(mut self, text: impl Into<String>) -> Self {
        self.conflicts = Some(text.into());
        self
    }
    pub fn nothing(mut self, text: impl Into<String>) -> Self {
        self.nothing = Some(text.into());
        self
    }
    pub fn untracked(mut self) -> Self {
        self.tracked = false;
        self
    }
    pub fn net(mut self) -> Self {
        self.net = true;
        self
    }
    pub fn on_error(mut self, f: impl Fn(&str, &mut Window, &mut App) -> bool + 'static) -> Self {
        self.on_error = Some(Rc::new(f));
        self
    }
    pub fn fix(mut self, fix: crate::errors::GitFix, action: Action) -> Self {
        self.fixes.push((fix, action));
        self
    }
}

/// `f` mapped to "didn't stop on conflicts": for the actions that can't.
pub fn plain<T>(r: Res<T>) -> Res<bool> {
    r.map(|_| false)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use gitviber_ui::Workspace;
    use gpui_kit::{
        Context, Entity, IntoElement, Render, TestAppContext, VisualTestContext, Window, div,
    };

    use super::*;

    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// The repository the tests open first: one commit.
    fn here() -> PathBuf {
        let dir = scratch("here");
        // Asked again for its path: the commit is made once.
        if !dir.join(".git/refs/heads/main").exists() {
            git(&dir, &["commit", "-q", "--allow-empty", "-m", "one"]);
        }
        dir
    }

    /// A scratch repository to point core at instead.
    fn scratch(name: &str) -> PathBuf {
        let dir = gitviber_ui::test_support::temp_dir(&format!("refs-{name}"));
        git(&dir, &["init", "-q", "-b", "main"]);
        dir.canonicalize().unwrap()
    }

    fn setup(
        cx: &mut TestAppContext,
    ) -> (Entity<Workspace>, Entity<RepoState>, &mut VisualTestContext) {
        setup_in(here(), cx)
    }

    fn setup_in(
        repo: PathBuf,
        cx: &mut TestAppContext,
    ) -> (Entity<Workspace>, Entity<RepoState>, &mut VisualTestContext) {
        gitviber_ui::test_support::config_dir("refs-repo-test");
        cx.update(|cx| {
            gitviber_ui::init(cx);
            gitviber_ui::install_core(cx);
            crate::init(cx);
        });
        let ws = cx.new(Workspace::new);
        // Toasts with buttons go to the window's Root.
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let host = cx.new(|_| Host);
            gpui_kit::base::Root::new(gpui_kit::AnyView::from(host), window, cx)
        });
        cx.update(|window, cx| {
            Workspace::register_window(&ws, window, cx);
            ws.update(cx, |ws, cx| ws.open_project(repo, window, cx));
        });
        cx.run_until_parked();
        let state = cx.update(|_, cx| {
            let project = ws.read(cx).project().cloned().expect("open");
            RepoState::get(&project, cx)
        });
        cx.run_until_parked();
        (ws, state, cx)
    }

    #[gpui_kit::test]
    fn the_worktree_stays_while_an_action_runs(cx: &mut TestAppContext) {
        let (ws, state, cx) = setup(cx);
        let root = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                ws.read(cx)
                    .project()
                    .map(|p| p.read(cx).root().to_path_buf())
            })
        };
        let run = state.update(cx, |s, cx| s.started("Push".into(), cx));
        let other = scratch("switch");
        cx.update(|window, cx| {
            ws.update(cx, |ws, cx| {
                ws.switch_worktree(other.clone(), cx);
                ws.open_project(other.clone(), window, cx);
                let toasts = ws.take_toasts();
                assert_eq!(toasts.len(), 2);
                assert!(toasts[0].message.contains("Wait for Push to finish."));
            })
        });
        cx.run_until_parked();
        assert_eq!(root(cx), Some(here()));
        // Done, it may go.
        state.update(cx, |s, _| s.ended(run));
        cx.update(|window, cx| ws.update(cx, |ws, cx| ws.open_project(other.clone(), window, cx)));
        assert_eq!(root(cx), Some(other));
    }

    #[gpui_kit::test]
    fn an_action_never_runs_in_a_worktree_opened_since(cx: &mut TestAppContext) {
        let (_ws, state, cx) = setup(cx);
        // Core has another repo open than the state's (a second window, a switch mid-way).
        let other = scratch("stale");
        let core = cx.update(|_, cx| gitviber_ui::core(cx));
        api::repo::open_repo(&core, other.to_string_lossy().into_owned()).unwrap();
        let ran = Arc::new(AtomicBool::new(false));
        let seen = ran.clone();
        let done = cx.update(|window, cx| {
            state.update(cx, |s, cx| {
                s.run(
                    GitAction::new("Push").untracked(),
                    move |_, _| {
                        seen.store(true, Ordering::Relaxed);
                        Ok(false)
                    },
                    window,
                    cx,
                )
            })
        });
        cx.run_until_parked();
        assert!(!ran.load(Ordering::Relaxed));
        assert_eq!(futures::executor::block_on(done), Ok(false));
        // And Sync's push, after its pull, asks again.
        let op = NetOp::quiet().pinned(here());
        assert_eq!(op.still_in(&core), Err(crate::net::STOPPED.to_string()));
        assert!(NetOp::quiet().pinned(other).still_in(&core).is_ok());
    }

    #[gpui_kit::test]
    fn a_toasts_button_runs_only_where_it_was_made(cx: &mut TestAppContext) {
        let (_ws, state, cx) = setup(cx);
        let ran = Rc::new(Cell::new(0));
        let count = ran.clone();
        let button = state.update(cx, |s, cx| {
            s.guard(
                Action::new("Retry with autostash", move |_, _| {
                    count.set(count.get() + 1)
                }),
                cx,
            )
        });
        cx.update(|window, cx| (button.run)(window, cx));
        assert_eq!(ran.get(), 1);
        // Something running: it waits for that.
        let run = state.update(cx, |s, cx| s.started("Pull".into(), cx));
        cx.update(|window, cx| (button.run)(window, cx));
        assert_eq!(ran.get(), 1);
        state.update(cx, |s, _| s.ended(run));
        // Another worktree open now: it was for the last one.
        state.update(cx, |s, _| s.root = scratch("button"));
        cx.update(|window, cx| (button.run)(window, cx));
        assert_eq!(ran.get(), 1);
    }

    #[test]
    fn a_network_action_says_how_it_ended_unless_stopped() {
        assert_eq!(
            net_note("Push", &Ok(false)),
            Some(("Push finished".into(), String::new()))
        );
        assert_eq!(
            net_note("Pull", &Ok(true)),
            Some(("Pull finished".into(), String::new()))
        );
        assert_eq!(
            net_note("Fetch", &Err("fatal: no route".into())),
            Some(("Fetch failed".into(), "fatal: no route".into()))
        );
        assert_eq!(net_note("Push", &Err(CANCELLED.into())), None);
        assert_eq!(net_note("Pull", &Err(git::CANCELLED_STASHED.into())), None);
    }

    #[gpui_kit::test]
    fn one_action_ending_leaves_the_others_running(cx: &mut TestAppContext) {
        let (_ws, state, cx) = setup(cx);
        let (a, b) = state.update(cx, |s, cx| {
            (
                s.started("Fetch".into(), cx),
                s.started("Rewrite".into(), cx),
            )
        });
        state.update(cx, |s, _| s.ended(a));
        assert_eq!(
            state.read_with(cx, |s, _| s.busy().cloned()),
            Some("Rewrite".into())
        );
        state.update(cx, |s, _| s.ended(b));
        assert!(state.read_with(cx, |s, _| s.busy().is_none()));
    }

    /// git in `dir`, never the owner's config: no signing, no hooks.
    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[gpui_kit::test]
    fn a_push_refused_over_a_secret_only_the_last_commit_has_offers_undoing_it(
        cx: &mut TestAppContext,
    ) {
        use std::os::unix::fs::PermissionsExt as _;
        // The push's progress comes from its stderr reader thread and wakes the bar's task.
        cx.executor().allow_parking();
        let dir = scratch("secret");
        git(&dir, &["commit", "-q", "--allow-empty", "-m", "one"]);
        git(&dir, &["commit", "-q", "--allow-empty", "-m", "token"]);
        // A remote that refuses as GitHub's push protection does, naming the pushed commit.
        let remote = gitviber_ui::test_support::temp_dir("refs-bare");
        git(&remote, &["init", "-q", "--bare"]);
        let hook = remote.join("hooks/pre-receive");
        std::fs::write(
            &hook,
            "#!/bin/sh\nread old new ref\necho \"error: GH013: Repository rule violations found for $ref.\" >&2\necho \"    - Push cannot contain secrets\" >&2\necho \"         - commit: $new\" >&2\necho \"       https://github.com/o/r/security/secret-scanning/unblock-secret/2ab\" >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        git(&dir, &["remote", "add", "origin", remote.to_str().unwrap()]);
        let (_ws, state, cx) = setup_in(dir.clone(), cx);
        let core = cx.update(|_, cx| gitviber_ui::core(cx));
        let head = git(&dir, &["rev-parse", "HEAD"]);
        let refused = api::sync::push(
            &core,
            Some(false),
            Some("origin".into()),
            None,
            "t".into(),
            |_| {},
        )
        .expect_err("the hook refuses");
        let listed = crate::errors::secret_commits(&refused);
        assert_eq!(listed, std::slice::from_ref(&head));
        assert_eq!(
            crate::ops::secret_only_in_head(&core, &listed),
            Some(head.clone())
        );
        // An earlier commit has one too: undoing the last doesn't help.
        let parent = git(&dir, &["rev-parse", "HEAD~1"]);
        let both = [head.clone(), parent];
        assert_eq!(crate::ops::secret_only_in_head(&core, &both), None);
        // The error shows once, after the check, with the way out.
        cx.update(|window, cx| crate::ops::publish(&state, "origin".into(), window, cx));
        cx.run_until_parked();
        use gpui_kit::component::WindowExt as _;
        assert_eq!(cx.update(|w, cx| w.notifications(cx).len()), 1);
        // Already on the push target: undoing it would need a force push.
        let branch = git(&dir, &["branch", "--show-current"]);
        git(
            &dir,
            &[
                "update-ref",
                &format!("refs/remotes/origin/{branch}"),
                "HEAD",
            ],
        );
        git(&dir, &["branch", "-q", "-u", &format!("origin/{branch}")]);
        assert_eq!(crate::ops::secret_only_in_head(&core, &listed), None);
    }

    #[gpui_kit::test]
    fn a_second_undo_while_the_switch_question_is_up_asks_nothing_more(cx: &mut TestAppContext) {
        // Uncommitted changes, so an undo that switches branches asks first.
        let dir = scratch("asking");
        std::fs::write(dir.join("new.txt"), "x").unwrap();
        let (_ws, state, cx) = setup_in(dir, cx);
        assert!(state.read_with(cx, |s, _| {
            s.status.as_ref().is_some_and(|s| !s.unstaged.is_empty())
        }));
        let undo = || vec![(1, Some("main".to_string()))];
        use gpui_kit::component::WindowExt as _;
        let dialogs = |cx: &mut VisualTestContext| cx.update(|w, cx| w.has_active_dialog(cx));
        // ⌘Z, then ⌘Z again while it asks.
        cx.update(|window, cx| crate::ops::travel(&state, false, undo(), window, cx));
        cx.update(|window, cx| crate::ops::travel(&state, false, undo(), window, cx));
        cx.run_until_parked();
        assert!(dialogs(cx));
        assert!(state.read_with(cx, |s, _| s.asking));
        // One answer and the question is gone: there was only one.
        cx.update(|window, cx| window.close_dialog(cx));
        cx.run_until_parked();
        assert!(!dialogs(cx));
        assert!(!state.read_with(cx, |s, _| s.asking));
        // Answered, ⌘Z asks again.
        cx.update(|window, cx| crate::ops::travel(&state, false, undo(), window, cx));
        cx.run_until_parked();
        assert!(dialogs(cx));
    }
}
