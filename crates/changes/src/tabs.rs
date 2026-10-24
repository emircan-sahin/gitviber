//! The center tabs of Changes' kinds: a file's unstaged, staged, conflicted or branch-review
//! diff, and a whole list as one scroll. Each follows the repo on disk, walks the Changes list
//! with J/K, marks its file viewed with V, and hands the app's diff commands to its view.

use crate::conflict::ConflictView;
use crate::panel::{self, ChangesPanel};
use crate::store;
use gitviber_core::api;
use gitviber_core::git::FileChange;
use gitviber_diff::{
    Compare, DiffEvent, DiffSource, DiffView, FileFacts, LineAction, NoticeLevel, StackedDiff,
    StackedEvent, StackedFile,
};
use gitviber_ui::actions::{diff, file, review};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{
    ChangesOf, ProjectEvent, Selection, TabHandle, TabIcon, TabItem, Toast, Workspace,
    register_opener, selection_path,
};
use gpui_kit::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Subscription, Task, WeakEntity, Window, div,
};
use std::sync::Arc;

/// The kinds Changes opens.
pub fn register(cx: &mut App) {
    for kind in ["unstaged", "staged", "conflict", "branch"] {
        register_opener(cx, kind, open);
    }
    // The unstaged, staged and branch lists; a commit's or a range's list opens as
    // "changes-commit" / "changes-range", History's (Selection::opener_kind).
    register_opener(cx, "changes", open);
}

fn open(sel: &Selection, window: &mut Window, cx: &mut App) -> Option<TabHandle> {
    let workspace = Workspace::for_window(window, cx)?.downgrade();
    if !matches!(sel, Selection::Changes { .. }) && source_of(sel).is_none() {
        return None;
    }
    let sel = sel.clone();
    Some(TabHandle::new(
        cx.new(|cx| ChangeTab::new(sel, workspace, window, cx)),
    ))
}

/// What a selection compares, for the diff view.
pub fn source_of(sel: &Selection) -> Option<DiffSource> {
    let (compare, file) = match sel {
        Selection::Unstaged { file } | Selection::Conflict { file } => (Compare::Unstaged, file),
        Selection::Staged { file } => (Compare::Staged, file),
        Selection::Branch { base, file, .. } => (Compare::Base { base: base.clone() }, file),
        _ => return None,
    };
    Some(DiffSource::from_change(compare, file))
}

fn facts(f: &FileChange) -> FileFacts {
    FileFacts {
        status: f.status.clone(),
        mode: f.mode.clone(),
        submodule: f.submodule.clone(),
    }
}

enum Body {
    Diff(Entity<DiffView>),
    Stacked(Entity<StackedDiff>),
    Conflict(Entity<ConflictView>),
}

pub struct ChangeTab {
    sel: Selection,
    workspace: WeakEntity<Workspace>,
    body: Body,
    focus: FocusHandle,
    /// The worktree the tab's typing is kept under, once attached.
    root: Option<String>,
    /// Typing not written to the store yet.
    edit_pending: bool,
    /// The review notes on the diff's lines (a local diff's).
    _notes: Option<Entity<gitviber_review::DiffNotes>>,
    _subscriptions: Vec<Subscription>,
    _load: Task<()>,
    _keep: Task<()>,
    _live: Live,
}

/// How long typing must pause before what's typed is kept for the next launch.
const KEEP_AFTER: std::time::Duration = std::time::Duration::from_millis(300);

impl ChangeTab {
    fn new(
        sel: Selection,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = vec![];
        let body = match &sel {
            Selection::Conflict { file }
                if file
                    .conflict
                    .as_deref()
                    .is_some_and(|c| c == "UU" || c == "AA") =>
            {
                Body::Conflict(cx.new(|cx| ConflictView::new(file.path.clone(), window, cx)))
            }
            Selection::Changes { .. } => {
                let view = cx.new(|cx| {
                    let mut s = StackedDiff::new(vec![], cx);
                    s.set_title(Some(sel.title()), cx);
                    s
                });
                subscriptions.push(cx.subscribe_in(
                    &view,
                    window,
                    |this, _, e: &StackedEvent, window, cx| this.stacked_event(e, window, cx),
                ));
                Body::Stacked(view)
            }
            _ => {
                let source = source_of(&sel).expect("checked by open");
                let view = cx.new(|cx| DiffView::new(source, cx));
                subscriptions.push(cx.subscribe_in(
                    &view,
                    window,
                    |this, _, e: &DiffEvent, window, cx| this.diff_event(e, window, cx),
                ));
                Body::Diff(view)
            }
        };
        // Typing still waiting for its pause goes to the store as the tab closes or the app quits.
        subscriptions.push(cx.on_release(|this, cx| this.keep_edit(cx)));
        subscriptions.push(cx.on_app_quit(|this, cx| {
            if this.edit_pending {
                this.keep_edit(cx);
                gitviber_ui::store::flush(cx);
            }
            async {}
        }));
        let tab = ChangeTab {
            sel,
            workspace,
            body,
            focus: cx.focus_handle(),
            root: None,
            edit_pending: false,
            _notes: None,
            _subscriptions: subscriptions,
            _load: Task::ready(()),
            _keep: Task::ready(()),
            _live: Live::new("ChangeTab"),
        };
        // Opened from inside the workspace's update: it can be read once that's done.
        let this = cx.weak_entity();
        cx.defer(move |cx| {
            this.update(cx, |this, cx| this.attach(cx)).ok();
        });
        tab
    }

    /// Follows the project's changes on disk, shows the viewed mark and the list's files, and
    /// brings back typing kept from before a restart.
    fn attach(&mut self, cx: &mut Context<Self>) {
        if let Some(project) = self
            .workspace
            .upgrade()
            .and_then(|ws| ws.read(cx).project().cloned())
        {
            self.root = Some(project.read(cx).root().to_string_lossy().into_owned());
            self._subscriptions
                .push(cx.subscribe(&project, |this, _, e: &ProjectEvent, cx| {
                    if let ProjectEvent::RepoChanged(_) = e {
                        this.repo_changed(cx);
                    }
                }));
        }
        self.sync_viewed(cx);
        self.repo_changed_stacked(cx);
        if let Body::Diff(view) = &self.body
            && matches!(
                self.sel,
                Selection::Unstaged { .. } | Selection::Staged { .. } | Selection::Branch { .. }
            )
        {
            self._notes = Some(gitviber_review::attach_notes(view, None, cx));
        }
        if let (Some(root), Some(path), Body::Diff(view)) =
            (&self.root, self.sel.edit_path(), &self.body)
            && let Some(kept) = store::edit(root, &path, cx)
        {
            view.update(cx, |v, cx| v.restore_edit(kept.text, kept.base, cx));
        }
    }

    fn keep_edit_soon(&mut self, cx: &mut Context<Self>) {
        self.edit_pending = true;
        self._keep = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(KEEP_AFTER).await;
            this.update(cx, |this, cx| this.keep_edit(cx)).ok();
        });
    }

    /// What's typed and not saved, kept under the file for the next launch; nothing once
    /// it's saved or dropped.
    fn keep_edit(&mut self, cx: &mut App) {
        if !std::mem::take(&mut self.edit_pending) {
            return;
        }
        let (Some(root), Some(path), Body::Diff(view)) =
            (&self.root, self.sel.edit_path(), &self.body)
        else {
            return;
        };
        let edit = view
            .read(cx)
            .unsaved_edit(cx)
            .map(|(text, base)| store::Edit { text, base });
        store::set_edit(root, &path, edit, cx);
    }

    fn repo_changed(&mut self, cx: &mut Context<Self>) {
        match &self.body {
            Body::Diff(v) => v.update(cx, |v, cx| v.repo_changed(cx)),
            Body::Conflict(v) => v.update(cx, |v, cx| v.refresh(cx)),
            Body::Stacked(_) => self.repo_changed_stacked(cx),
        }
        self.sync_viewed(cx);
    }

    /// A list's files, read again: they come and go with the work.
    fn repo_changed_stacked(&mut self, cx: &mut Context<Self>) {
        let (Body::Stacked(view), Selection::Changes { of }) = (&self.body, &self.sel) else {
            return;
        };
        let view = view.downgrade();
        let core = crate::core(cx);
        let of = match of {
            ChangesOf::Unstaged => Some(false),
            ChangesOf::Staged => Some(true),
            _ => None,
        };
        let review = self
            .workspace
            .upgrade()
            .and_then(|ws| ws.read(cx).review_base().map(str::to_string));
        let workspace = self.workspace.clone();
        self._load = cx.spawn(async move |_, cx| {
            let files = cx
                .background_spawn(async move {
                    match of {
                        Some(staged) => api::changes::status(&core).map(|s| {
                            let (list, compare) = if staged {
                                (s.staged, Compare::Staged)
                            } else {
                                (s.unstaged, Compare::Unstaged)
                            };
                            list.into_iter()
                                .filter(|f| f.nested.is_none())
                                .map(|f| (f, compare.clone()))
                                .collect::<Vec<_>>()
                        }),
                        None => match review {
                            Some(base) => api::changes::branch_review(&core, base).map(|r| {
                                let compare = Compare::Base {
                                    base: r.base.clone(),
                                };
                                r.files.into_iter().map(|f| (f, compare.clone())).collect()
                            }),
                            None => Ok(vec![]),
                        },
                    }
                })
                .await;
            let Ok(files) = files else { return };
            cx.update(|cx| {
                let viewed = |sel: &Selection| {
                    workspace
                        .upgrade()
                        .is_some_and(|ws| panel::is_viewed(ws.read(cx), sel))
                };
                let list: Vec<StackedFile> = files
                    .into_iter()
                    .map(|(f, compare)| {
                        let file = Arc::new(f.clone());
                        let sel = match &compare {
                            Compare::Staged => Selection::Staged { file },
                            Compare::Base { base } => Selection::Branch {
                                base: base.clone(),
                                label: String::new(),
                                file,
                                fixed: false,
                            },
                            _ => Selection::Unstaged { file },
                        };
                        StackedFile {
                            viewed: viewed(&sel),
                            sig: crate::model::list::file_sig(&f),
                            additions: f.additions,
                            deletions: f.deletions,
                            source: DiffSource::new(compare, f.path.clone())
                                .old_path(f.old_path.clone())
                                .file(facts(&f)),
                        }
                    })
                    .collect();
                view.update(cx, |v, cx| v.set_files(list, cx)).ok();
            });
        });
    }

    fn file(&self) -> Option<&Arc<FileChange>> {
        match &self.sel {
            Selection::Unstaged { file }
            | Selection::Staged { file }
            | Selection::Conflict { file }
            | Selection::Branch { file, .. } => Some(file),
            _ => None,
        }
    }

    /// The Viewed button shows the mark as it is now; staged files count as viewed.
    fn sync_viewed(&mut self, cx: &mut Context<Self>) {
        let Body::Diff(view) = &self.body else { return };
        let shown = match (&self.sel, self.workspace.upgrade()) {
            (Selection::Staged { .. }, _) => Some(true),
            (Selection::Unstaged { .. } | Selection::Branch { .. }, Some(ws)) => {
                Some(panel::is_viewed(ws.read(cx), &self.sel))
            }
            _ => None,
        };
        view.update(cx, |v, cx| v.set_viewed(shown, cx));
    }

    fn toggle_viewed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(file), Some(ws)) = (self.file().cloned(), self.workspace.upgrade()) else {
            return;
        };
        let kind = self.sel.kind();
        if kind == "staged" {
            // Unchecking a staged file takes it back out of the commit.
            panel::unstage(vec![file.as_ref().clone()], ws.downgrade(), cx);
            return;
        }
        let on = !panel::is_viewed(ws.read(cx), &self.sel);
        let sel = self.sel.clone();
        ws.update(cx, |ws, cx| panel::set_viewed(ws, &sel, on, cx));
        self.sync_viewed(cx);
        let _ = window;
    }

    /// J / K: the next or previous file of the Changes list, in a preview tab.
    fn step_file(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ws) = self.workspace.upgrade() else {
            return;
        };
        let Some(panel) = ws
            .read(cx)
            .built_panel("changes")
            .and_then(|p| p.view().downcast::<ChangesPanel>().ok())
        else {
            return;
        };
        let list = panel.read(cx).walk_list(&self.sel);
        let key = gitviber_ui::workspace::selection_key(&self.sel);
        let at = list
            .iter()
            .position(|s| gitviber_ui::workspace::selection_key(s) == key);
        let next = match (at, forward) {
            (Some(i), true) => list.get(i + 1),
            (Some(i), false) => i.checked_sub(1).and_then(|i| list.get(i)),
            (None, _) => list.first(),
        };
        if let Some(next) = next.cloned() {
            ws.update(cx, |ws, cx| ws.open(next, true, window, cx));
        }
    }

    fn toast(&self, toast: Toast, cx: &mut Context<Self>) {
        if let Some(ws) = self.workspace.upgrade() {
            ws.update(cx, |ws, cx| ws.toast(toast, cx));
        }
    }

    fn diff_event(&mut self, e: &DiffEvent, window: &mut Window, cx: &mut Context<Self>) {
        match e {
            DiffEvent::ToggleViewed => self.toggle_viewed(window, cx),
            DiffEvent::OpenFile(path) => {
                if let Some(ws) = self.workspace.upgrade() {
                    let sel = Selection::File { path: path.clone() };
                    ws.update(cx, |ws, cx| ws.open(sel, false, window, cx));
                }
            }
            DiffEvent::Notice(n) => {
                let toast = match n.level {
                    NoticeLevel::Error => Toast::error(n.detail.clone().unwrap_or_default()),
                    NoticeLevel::Success => Toast::success(n.detail.clone().unwrap_or_default()),
                    NoticeLevel::Info => Toast::info(n.detail.clone().unwrap_or_default()),
                };
                self.toast(toast.title(n.title.clone()), cx);
            }
            DiffEvent::LinesChanged | DiffEvent::Saved(_) => self.refresh_project(cx),
            DiffEvent::Edited => self.keep_edit_soon(cx),
            DiffEvent::Loaded { .. } | DiffEvent::SelectionChanged(_) | DiffEvent::AddNote(_) => {}
        }
    }

    fn stacked_event(&mut self, e: &StackedEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ws) = self.workspace.upgrade() else {
            return;
        };
        match e {
            StackedEvent::OpenFile(source) => {
                if let Some(sel) = panel::selection_for(&ws, source, cx) {
                    ws.update(cx, |ws, cx| ws.open(sel, false, window, cx));
                }
            }
            StackedEvent::ToggleViewed(source) => {
                let kind = match source.compare {
                    Compare::Staged => "staged",
                    Compare::Base { .. } => "branch",
                    _ => "unstaged",
                };
                if let Some(sel) = panel::selection_for(&ws, source, cx) {
                    if kind == "staged" {
                        if let Some(file) = sel.file().cloned() {
                            panel::unstage(vec![file], ws.downgrade(), cx);
                        }
                    } else {
                        let on = !panel::is_viewed(ws.read(cx), &sel);
                        ws.update(cx, |ws, cx| panel::set_viewed(ws, &sel, on, cx));
                        self.repo_changed_stacked(cx);
                    }
                }
            }
            StackedEvent::AddNote { .. } | StackedEvent::Notice(_) => {}
        }
    }

    fn refresh_project(&mut self, cx: &mut Context<Self>) {
        if let Some(project) = self
            .workspace
            .upgrade()
            .and_then(|ws| ws.read(cx).project().cloned())
        {
            project.update(cx, |p, cx| p.repo_changed(cx));
        }
    }

    fn view(&self) -> AnyView {
        match &self.body {
            Body::Diff(v) => v.clone().into(),
            Body::Stacked(v) => v.clone().into(),
            Body::Conflict(v) => v.clone().into(),
        }
    }

    pub(crate) fn diff(&self) -> Option<&Entity<DiffView>> {
        if let Body::Diff(v) = &self.body {
            Some(v)
        } else {
            None
        }
    }
}

impl Focusable for ChangeTab {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.body {
            Body::Diff(v) => v.focus_handle(cx),
            Body::Stacked(v) => v.focus_handle(cx),
            Body::Conflict(v) => v.focus_handle(cx),
        }
    }
}

impl TabItem for ChangeTab {
    fn tab_title(&self, _: &App) -> SharedString {
        match &self.sel {
            Selection::Changes { .. } => self.sel.title(),
            sel => selection_path(sel)
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_string()
                .into(),
        }
    }

    fn tab_detail(&self, _: &App) -> Option<SharedString> {
        Some(
            match self.sel.kind() {
                "staged" => "staged",
                "conflict" => "conflict",
                "branch" => "branch",
                "changes" => return None,
                _ => "diff",
            }
            .into(),
        )
    }

    fn tab_icon(&self, _: &App) -> TabIcon {
        match &self.sel {
            Selection::Changes { .. } => self.sel.icon(),
            sel => TabIcon::File(selection_path(sel).into()),
        }
    }

    fn tab_tooltip(&self, _: &App) -> Option<SharedString> {
        Some(selection_path(&self.sel).into())
    }

    fn is_dirty(&self, cx: &App) -> bool {
        self.diff()
            .is_some_and(|v| v.read(cx).has_unsaved_edits(cx))
    }

    fn tab_focus(&self, cx: &App) -> Option<FocusHandle> {
        Some(Focusable::focus_handle(self, cx))
    }
}

impl Render for ChangeTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("change-tab")
            .track_focus(&self.focus)
            .size_full()
            .child(self.view())
            .on_action(cx.listener(move |this, _: &review::NextFile, window, cx| {
                if let Body::Stacked(v) = &this.body {
                    v.update(cx, |v, cx| v.step(true, cx));
                } else {
                    this.step_file(true, window, cx);
                }
            }))
            .on_action(cx.listener(move |this, _: &review::PrevFile, window, cx| {
                if let Body::Stacked(v) = &this.body {
                    v.update(cx, |v, cx| v.step(false, cx));
                } else {
                    this.step_file(false, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &review::ToggleViewed, window, cx| {
                if let Body::Stacked(v) = &this.body {
                    v.update(cx, |v, cx| v.toggle_top_viewed(cx));
                } else {
                    this.toggle_viewed(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &diff::NextChange, _, cx| {
                if let Some(v) = this.diff() {
                    v.update(cx, |v, cx| v.next_change(cx));
                }
            }))
            .on_action(cx.listener(|this, _: &diff::PrevChange, _, cx| {
                if let Some(v) = this.diff() {
                    v.update(cx, |v, cx| v.prev_change(cx));
                }
            }))
            .on_action(cx.listener(|this, _: &diff::StageChange, _, cx| {
                this.line_action(LineAction::Stage, cx)
            }))
            .on_action(cx.listener(|this, _: &diff::UnstageChange, _, cx| {
                this.line_action(LineAction::Unstage, cx)
            }))
            .on_action(cx.listener(|this, _: &diff::DiscardChange, _, cx| {
                this.line_action(LineAction::Discard, cx)
            }))
            .on_action(cx.listener(|this, _: &file::Save, window, cx| {
                if let Some(v) = this.diff() {
                    v.update(cx, |v, cx| v.save(window, cx));
                }
            }))
    }
}

impl ChangeTab {
    fn line_action(&mut self, action: LineAction, cx: &mut Context<Self>) {
        if let Some(v) = self.diff() {
            v.update(cx, |v, cx| v.line_action(action, cx));
        }
    }
}
