//! A pull request's tab: what it is and where it stands, merge, review, checkout, its checks
//! (why one failed, copied for the agent), its commits (pick one or a run of them to narrow
//! Files changed), its files, and the description and conversation. The detail, files and
//! threads live here only: closing the tab frees them.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gitviber_core::api;
use gitviber_core::git::FileChange;
use gitviber_core::github::{Annotation, CheckFailure, PullDetail, PullFiles};
use gitviber_markdown::MarkdownView;
use gitviber_ui::AppTheme;
use gitviber_ui::actions::editor;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{FileIcon, LineCounts, PathLabel, StatusLetter};
use gitviber_ui::workspace::{PullRange, TabIcon, TabItem, Toast, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, EntityId, FocusHandle, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, ListAlignment, ListState,
    ParentElement as _, Pixels, Render, ScrollHandle, ScrollStrategy, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, UniformListScrollHandle,
    WeakEntity, Window, div, list, prelude::FluentBuilder as _, px, rems, uniform_list,
};

use crate::cache::{Cache, MIN_AGE, Wakes};
use crate::ci::{self, CiWatch};
use crate::logic::actions::{
    MergeMethod, PullRights, ReviewEvent, allowed_methods, pull_rights, pull_source, review_badge,
};
use crate::logic::checks::{check_note, checks_summary, failure_report, is_failing, sort_checks};
use crate::logic::keys::move_target;
use crate::logic::pull_commits::{
    CommitRun, by_day, is_whole_pr, pick_label, picked_commits, run_to,
};
use crate::model::{Account, Check, PullItem, PullState, Target, repo_of};
use crate::requests::{GithubRequest, PullSource, request};
use crate::store::Store;
use crate::ui::common::{
    Attachments, Blocks, ago, ci_badge, copy, error_text, load_failed, open_url, pull_icon,
    pull_pill, section, section_part, text_button, theme,
};
use crate::ui::dialogs::{ReviewDialog, confirm};
use crate::ui::markdown_input::MarkdownInput;
use crate::ui::page_find::{FindHost, Key, Marks, PageFind, Parts};

/// The files only change with the two commits: rarely worth reading again.
const FILES_AGE: Duration = Duration::from_secs(600);
/// Commits a PgUp/PgDn moves in the commits list.
const COMMIT_PAGE: usize = 10;
/// What a merge or cherry-pick that stopped on conflicts says.
const CONFLICTS: &str = "Stopped on conflicts: resolve them in Changes, then push.";
/// How far past the page's view its list measures what it hasn't yet.
const PAGE_OVERDRAW: Pixels = px(600.);
/// The commits list's lines are this high, but for an open message: the ones out of its view
/// stand as one gap of their exact height.
const DAY_H: Pixels = px(24.);
const ROW_H: Pixels = px(28.);
const COMMITS_H: Pixels = px(360.);
/// How far past the commits list's view its lines are still drawn.
const COMMITS_OVERSCAN: Pixels = px(360.);

/// The files Files changed lists, and the picked commits' range (base, head, label) they're from.
type Shown = (Arc<Vec<FileChange>>, Option<(String, String, String)>);

/// A line of the commits list: a day's header (by its order) or a commit (by its index).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommitLine {
    Day(usize),
    Row(usize),
}

struct Failure {
    data: Option<Arc<CheckFailure>>,
    error: Option<String>,
    /// The log's box, which a match in it scrolls.
    log: ScrollHandle,
    _task: Task<()>,
}

/// A write's end: done, or (merges, cherry-picks) stopped on conflicts left in the working tree.
pub(crate) type Wrote = Result<bool, String>;

fn done(r: Result<impl Sized, String>) -> Wrote {
    r.map(|_| false)
}

pub struct PullView {
    url: String,
    number: u64,
    target: Target,
    /// https://github.com/owner/name of the PR's repository.
    web: String,
    item: Option<PullItem>,
    ws: WeakEntity<Workspace>,
    detail: Option<Arc<PullDetail>>,
    detail_error: Option<String>,
    detail_at: Option<Instant>,
    loading: bool,
    files: Option<Arc<PullFiles>>,
    /// The files of `files`, shared with the list that draws them.
    file_list: Arc<Vec<FileChange>>,
    files_error: Option<String>,
    files_for: Option<(String, String)>,
    files_at: Option<Instant>,
    pick: Option<CommitRun>,
    picked_files: Option<(String, String, Arc<Vec<FileChange>>)>,
    picked_error: Option<String>,
    seen: Option<HashSet<String>>,
    open_bodies: HashSet<String>,
    /// The description (block 0) and each comment, synced at the next frame after a read.
    blocks: Blocks,
    text_stale: bool,
    attachments: Attachments,
    failures: std::collections::HashMap<u64, Failure>,
    busy: Option<SharedString>,
    /// What a finished write asks of another feature, sent at the next frame.
    pending_request: Option<GithubRequest>,
    /// The review being written: kept here, so a refused review or a closed dialog keeps it.
    review: Option<Entity<MarkdownInput>>,
    /// The review box's: text in it keeps the tab. Replaced with the box.
    _review: Option<Subscription>,
    review_pick: Option<ReviewEvent>,
    /// Goes up when a review lands; the dialog closes on it.
    reviews_done: u64,
    polls: u32,
    wakes: Wakes,
    generation: u64,
    focus: FocusHandle,
    /// The page as a list: all above the conversation, then each comment, so a long thread
    /// draws only what's in view.
    page: ListState,
    find: PageFind,
    /// The commits list's keys: the row they're on, by its sha.
    commits_focus: FocusHandle,
    commits_scroll: ScrollHandle,
    commit_cursor: Option<String>,
    /// A commit the keys moved to, scrolled into the list's view at the next frame.
    commits_to: Option<usize>,
    /// The line the find's current match is in: drawn wherever it is.
    commits_keep: Option<CommitLine>,
    /// Each open message's height as last drawn; the list's children that drew them.
    body_heights: HashMap<String, Pixels>,
    bodies_drawn: Vec<(usize, String)>,
    /// A frame was asked for to measure a message just opened.
    bodies_asked: bool,
    files_scroll: UniformListScrollHandle,
    _ci: Option<CiWatch>,
    _detail: Task<()>,
    _files: Task<()>,
    _picked: Task<()>,
    _action: Task<()>,
    _poll: Task<()>,
    _signed: Task<()>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl PullView {
    /// `item`: the list's copy, shown until the detail is in; None for a tab restored by its url.
    pub fn new(
        ws: WeakEntity<Workspace>,
        url: String,
        item: Option<PullItem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let number = url
            .rsplit('/')
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or_default();
        let repo = repo_of(&url);
        crate::ui::use_workspace_project(&ws, cx);
        let subs = vec![
            cx.observe_global::<ci::Ci>(|_, cx| cx.notify()),
            cx.observe_global::<Cache>(|this: &mut Self, cx| {
                if Cache::wakes(cx) != this.wakes || Cache::generation(cx) != this.generation {
                    cx.notify();
                }
            }),
            cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    Cache::wake(cx);
                }
            }),
        ];
        let commits_focus = cx.focus_handle();
        let page = ListState::new(0, ListAlignment::Top, PAGE_OVERDRAW);
        // The commits list's keys work while it's scrolled out of the page.
        page.splice_focusable(0..0, [Some(commits_focus.clone())]);
        let mut view = PullView {
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
            files: None,
            file_list: Arc::new(vec![]),
            files_error: None,
            files_for: None,
            files_at: None,
            pick: None,
            picked_files: None,
            picked_error: None,
            seen: None,
            open_bodies: HashSet::new(),
            blocks: Blocks::default(),
            text_stale: false,
            attachments: Attachments::default(),
            failures: Default::default(),
            busy: None,
            pending_request: None,
            review: None,
            _review: None,
            review_pick: None,
            reviews_done: 0,
            polls: 0,
            wakes: Cache::wakes(cx),
            generation: Cache::generation(cx),
            focus: cx.focus_handle(),
            page,
            find: PageFind::default(),
            commits_focus,
            commits_scroll: ScrollHandle::new(),
            commit_cursor: None,
            commits_to: None,
            commits_keep: None,
            body_heights: HashMap::new(),
            bodies_drawn: vec![],
            bodies_asked: false,
            files_scroll: UniformListScrollHandle::new(),
            _ci: None,
            _detail: Task::ready(()),
            _files: Task::ready(()),
            _picked: Task::ready(()),
            _action: Task::ready(()),
            _poll: Task::ready(()),
            _signed: Task::ready(()),
            _subs: subs,
            _live: Live::new("PullView"),
        };
        crate::account::load(cx, false);
        view.load(cx);
        view
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    fn key(&self) -> String {
        crate::ui::pull_key(&self.url)
    }

    /// The active tab of its workspace: what CI is kept current for.
    fn shown(&self, cx: &App) -> bool {
        self.ws.upgrade().is_some_and(|ws| {
            ws.read(cx)
                .tabs()
                .active_key()
                .is_some_and(|k| k.as_ref() == self.key())
        })
    }

    fn item(&self) -> Option<PullItem> {
        self.detail
            .as_ref()
            .map(|d| PullItem::from(&d.pull))
            .or_else(|| self.item.clone())
    }

    fn toast(&self, toast: Toast, cx: &mut App) {
        self.ws.update(cx, |ws, cx| ws.toast(toast, cx)).ok();
    }

    /// Reads the PR again; the page shows what it has meanwhile.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.detail_at = Some(Instant::now());
        let (core, target, number) = (crate::core(cx), self.target.clone(), self.number);
        self._detail = cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move { api::github::pr_detail(&core, target, number) })
                .await;
            this.update(cx, |this, cx| this.detail_landed(read, cx))
                .ok();
        });
        cx.notify();
    }

    fn detail_landed(&mut self, read: Result<PullDetail, String>, cx: &mut Context<Self>) {
        self.loading = false;
        let d = match read {
            Ok(d) => Arc::new(d),
            Err(e) => {
                self.detail_error = Some(e);
                cx.notify();
                return;
            }
        };
        self.detail_error = None;
        // Merged or closed on GitHub: the lists still show it as it was.
        if Cache::list_is_behind::<PullItem>(cx, "pulls:", &d.pull.url, &d.pull.updated_at, |r| {
            (r.url.as_str(), r.updated_at.as_str())
        }) {
            Cache::pulls_changed(cx);
        }
        let mergeable_unknown = d.pull.state == "open" && d.mergeable.is_none();
        self.detail = Some(d.clone());
        self.text_stale = true;
        self.wants_signed(cx);
        // GitHub computes mergeability in the background and answers null until it's done; ask
        // again soon, backing off, then leave it to ⟳.
        if mergeable_unknown && self.polls < 5 {
            let wait = Duration::from_secs(2 << self.polls);
            self.polls += 1;
            self._poll = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                this.update(cx, |this, cx| this.load(cx)).ok();
            });
        } else if !mergeable_unknown {
            self.polls = 0;
        }
        self.watch_ci(cx);
        let shas = (d.pull.base_sha.clone(), d.pull.head_sha.clone());
        if self.files_for.as_ref() != Some(&shas)
            || self.files_at.is_some_and(|at| at.elapsed() > FILES_AGE)
        {
            self.load_files(cx);
        }
        cx.notify();
    }

    /// CI for the head, and for each commit, newest first (asked 100 at a time), while shown.
    fn watch_ci(&mut self, cx: &mut Context<Self>) {
        let Some(d) = &self.detail else { return };
        let mut shas = vec![d.pull.head_sha.clone()];
        for c in self.commits().iter().rev() {
            if !shas.contains(&c.sha) {
                shas.push(c.sha.clone());
            }
        }
        let live = d.pull.state == "open";
        self._ci = Some(CiWatch::new(
            self.target.clone(),
            shas,
            live,
            |this: &Self, cx| this.shown(cx),
            cx,
        ));
    }

    /// The description and comments, as markdown views; at a frame, which has the window.
    fn sync_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.text_stale = false;
        let Some(d) = self.detail.clone() else { return };
        let texts: Vec<&str> = std::iter::once(d.body.as_str())
            .chain(d.comments.iter().map(|c| c.body.as_str()))
            .collect();
        let (web, url) = (self.web.clone(), self.url.clone());
        let signed = self.attachments.signed.clone();
        // Block 0 (the description) is in the page's first item, comment n's block in item n + 1.
        for item in self.blocks.sync(&texts, &web, &url, &signed, window, cx) {
            self.page.remeasure_items(item..item + 1);
        }
    }

    /// The description's view, when it has text.
    fn body_view(&self) -> Option<Entity<MarkdownView>> {
        let d = self.detail.as_ref()?;
        self.blocks.get(0).filter(|_| !d.body.trim().is_empty())
    }

    /// Comment `i`'s view, when it has text.
    fn comment_view(&self, d: &PullDetail, i: usize) -> Option<Entity<MarkdownView>> {
        let c = d.comments.get(i)?;
        self.blocks.get(i + 1).filter(|_| !c.body.trim().is_empty())
    }

    /// Cached data stays on screen; a failed refresh (a rate limit, say) is noted beside it.
    fn refresh_error(&self) -> Option<String> {
        let e = self.detail_error.as_ref()?;
        (self.busy.is_none() && self.detail.is_some()).then(|| format!("Couldn't refresh: {e}"))
    }

    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find.open(self, None, &self.focus, window, cx);
    }

    /// Scrolls the page's list to item `ix`'s top unless some of it is in view; what's in it
    /// then asks for the line it wants. By its top: items never drawn have no height yet.
    fn reveal_item(&self, ix: usize) {
        let view = self.page.viewport_bounds();
        if !self
            .page
            .bounds_for_item(ix)
            .is_some_and(|b| b.intersects(&view))
        {
            self.page.scroll_to(gpui_kit::ListOffset {
                item_ix: ix,
                offset_in_item: px(0.),
            });
        }
    }

    /// One item per comment after the first: the list follows the comments read.
    fn sync_items(&self) {
        let want = 1 + self.detail.as_ref().map_or(0, |d| d.comments.len());
        let have = self.page.item_count();
        if want > have {
            self.page.splice(have..have, want - have);
        } else if want < have {
            self.page.splice(want..have, 0);
        }
    }

    /// Signed links for the attachments the text holds, asked once per new one.
    fn wants_signed(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.detail.clone() else { return };
        let texts: Vec<&str> = std::iter::once(d.body.as_str())
            .chain(d.comments.iter().map(|c| c.body.as_str()))
            .collect();
        if !self.attachments.wants(&texts) {
            return;
        }
        let (core, target, number) = (crate::core(cx), self.target.clone(), self.number);
        self._signed = cx.spawn(async move |this, cx| {
            let Ok(map) = cx
                .background_spawn(async move { api::github::pr_attachments(&core, target, number) })
                .await
            else {
                return;
            };
            this.update(cx, |this, cx| {
                if this.attachments.add(map) {
                    this.text_stale = true;
                    cx.notify();
                }
            })
            .ok();
        });
    }

    /// Fetches the PR's commits first when they're missing; the page shows without waiting.
    fn load_files(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.detail.clone() else { return };
        let shas = (d.pull.base_sha.clone(), d.pull.head_sha.clone());
        self.files_for = Some(shas);
        self.files_at = Some(Instant::now());
        let (core, target, number) = (crate::core(cx), self.target.clone(), self.number);
        let (base_ref, base_sha, head_sha) = (
            d.pull.base_ref.clone(),
            d.pull.base_sha.clone(),
            d.pull.head_sha.clone(),
        );
        self._files = cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move {
                    api::github::pr_files(
                        &core,
                        target,
                        number,
                        base_ref,
                        base_sha,
                        head_sha,
                        format!("pr-files-{number}"),
                        |_| {},
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                match read {
                    Ok(f) => {
                        this.file_list = Arc::new(f.files.clone());
                        this.files = Some(Arc::new(f));
                        this.files_error = None;
                        this.note_seen(cx);
                        this.watch_ci(cx);
                    }
                    Err(e) => this.files_error = Some(e),
                }
                cx.notify();
            })
            .ok();
        });
    }

    fn commits(&self) -> &[gitviber_core::git::Commit] {
        self.files.as_ref().map_or(&[], |f| &f.commits)
    }

    /// The picked commits' range, and whether it narrows the PR (not every commit from its base).
    fn picked(&self) -> Option<(std::ops::Range<usize>, bool)> {
        let range = picked_commits(self.commits(), self.pick.as_ref())?;
        let whole = is_whole_pr(
            self.commits(),
            range.clone(),
            self.files.as_ref().map(|f| f.base.as_str()),
        );
        Some((range, !whole))
    }

    /// Which commits are new: pushed since you last looked. A PR opened for the first time has
    /// none; a commit stops being new once picked.
    fn note_seen(&mut self, cx: &mut Context<Self>) {
        let shas: Vec<String> = self.commits().iter().map(|c| c.sha.clone()).collect();
        if shas.is_empty() {
            return;
        }
        let picked: Vec<String> = self
            .picked()
            .map(|(r, _)| self.commits()[r].iter().map(|c| c.sha.clone()).collect())
            .unwrap_or_default();
        let seen = self
            .seen
            .get_or_insert_with(|| match Store::get(cx).seen_commits(&self.url) {
                Some(s) => s.into_iter().collect(),
                None => shas.iter().cloned().collect(),
            });
        seen.extend(picked);
        let (url, seen) = (self.url.clone(), seen.clone());
        Store::update(cx, |s| s.set_seen_commits(&url, &shas, &seen));
    }

    fn is_new(&self, sha: &str) -> bool {
        self.seen.as_ref().is_some_and(|s| !s.contains(sha))
    }

    fn set_pick(&mut self, run: Option<CommitRun>, cx: &mut Context<Self>) {
        self.pick = run;
        self.picked_files = None;
        self.picked_error = None;
        self.note_seen(cx);
        if let Some((range, true)) = self.picked() {
            let commits = self.commits();
            let (first, last) = (&commits[range.start], &commits[range.end - 1]);
            match first.parents.first().cloned() {
                // A root commit (merged in from an unrelated history) has nothing before it.
                None => {
                    self.picked_error = Some(format!(
                        "{} has no parent to compare with.",
                        first.short_sha
                    ))
                }
                Some(base) => {
                    let (core, head) = (crate::core(cx), last.sha.clone());
                    self._picked = cx.spawn(async move |this, cx| {
                        let (b, h) = (base.clone(), head.clone());
                        let read = cx
                            .background_spawn(async move { api::history::range_files(&core, b, h) })
                            .await;
                        this.update(cx, |this, cx| {
                            match read {
                                Ok(files) => {
                                    this.picked_files = Some((base, head, Arc::new(files)))
                                }
                                Err(e) => this.picked_error = Some(e),
                            }
                            cx.notify();
                        })
                        .ok();
                    });
                }
            }
        }
        cx.notify();
    }

    fn click_commit(&mut self, sha: String, extend: bool, cx: &mut Context<Self>) {
        self.commit_cursor = Some(sha.clone());
        let anchor = if extend {
            self.pick
                .as_ref()
                .filter(|_| self.picked().is_some())
                .map(|p| p.anchor.clone())
        } else {
            None
        };
        let run = run_to(self.commits(), anchor.as_deref(), &sha);
        self.set_pick(Some(run), cx);
    }

    /// Where the commits list's keys are: the row last landed on, else the picked end, else the
    /// first.
    fn commit_at(&self) -> Option<usize> {
        let commits = self.commits();
        let find = |sha: &str| commits.iter().position(|c| c.sha == sha);
        self.commit_cursor
            .as_deref()
            .and_then(find)
            .or_else(|| self.pick.as_ref().and_then(|p| find(&p.to)))
            .or((!commits.is_empty()).then_some(0))
    }

    /// ↑/↓, Home/End, PgUp/PgDn land on a commit as a click does (its changes alone), Space
    /// picks the one the keys are on, ⇧↑/⇧↓ widen or narrow the run as ⇧-click does, Esc shows
    /// every commit's changes again.
    fn commit_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let m = ev.keystroke.modifiers;
        let Some(at) = self
            .commit_at()
            .filter(|_| !(m.alt || m.platform || m.control))
        else {
            return;
        };
        let n = self.commits().len();
        let key = ev.keystroke.key.as_str();
        let sha = |this: &Self, i: usize| this.commits()[i].sha.clone();
        if m.shift {
            let to = match key {
                "up" => at.checked_sub(1),
                "down" => (at + 1 < n).then_some(at + 1),
                _ => return,
            };
            cx.stop_propagation();
            let Some(to) = to else { return };
            let anchor = self
                .pick
                .as_ref()
                .filter(|_| self.picked().is_some())
                .map_or_else(|| sha(self, at), |p| p.anchor.clone());
            let run = run_to(self.commits(), Some(&anchor), &sha(self, to));
            self.commit_cursor = Some(sha(self, to));
            self.commits_to = Some(to);
            self.set_pick(Some(run), cx);
            return;
        }
        match key {
            "escape" if self.pick.is_some() => self.set_pick(None, cx),
            "space" => self.click_commit(sha(self, at), false, cx),
            key => {
                let Some(to) = move_target(key, at, n, COMMIT_PAGE) else {
                    return;
                };
                if to != at || self.pick.is_none() {
                    self.commits_to = Some(to);
                    self.click_commit(sha(self, to), false, cx);
                }
            }
        }
        cx.stop_propagation();
    }

    /// The one before or after what's picked.
    fn step(&mut self, dir: isize, cx: &mut Context<Self>) {
        let Some((range, _)) = self.picked() else {
            return;
        };
        let at = if dir < 0 {
            range.start as isize - 1
        } else {
            range.end as isize
        };
        if let Some(c) = (at >= 0).then(|| self.commits().get(at as usize)).flatten() {
            let sha = c.sha.clone();
            self.set_pick(
                Some(CommitRun {
                    anchor: sha.clone(),
                    to: sha,
                }),
                cx,
            );
        }
    }

    /// Runs a write in the background under `busy`; when it's through, says `done` (or that it
    /// stopped on conflicts), reads the PR again and runs `after`.
    fn act(
        &mut self,
        busy: &'static str,
        done: String,
        run: impl FnOnce(&gitviber_core::Core) -> Wrote + Send + 'static,
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
                    Ok(stopped) => {
                        let toast = if stopped {
                            Toast::warning(CONFLICTS)
                        } else {
                            Toast::success(done)
                        };
                        this.toast(toast, cx);
                        Cache::pulls_changed(cx);
                        this.load(cx);
                        after(this, cx);
                    }
                    Err(e) => this.toast(Toast::error(e).title(format!("{busy} failed")), cx),
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

    fn checkout(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.item() else { return };
        let account = self.account(cx);
        let Some(source) = pull_source(&p, account.as_deref()) else {
            return;
        };
        let (target, number, head_ref, same) = (
            self.target.clone(),
            p.number,
            p.head_ref.clone(),
            source.same_repo,
        );
        let message = format!("Switched to {}", source.branch);
        // A checkout moves HEAD; the repo watcher refreshes the rest of the window.
        self.act(
            "Checkout",
            message,
            move |core| {
                done(api::github::pr_checkout(
                    core,
                    target,
                    number,
                    head_ref,
                    same,
                    format!("pr-checkout-{number}"),
                    |_| {},
                ))
            },
            |_, _| {},
            cx,
        );
    }

    fn merge(&mut self, method: MergeMethod, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.item() else { return };
        let account = self.account(cx);
        let branch = pull_source(&p, account.as_deref()).map_or(p.head_ref.clone(), |s| s.branch);
        let weak = cx.weak_entity();
        let message = format!("{}: #{} into {}?", method.label(), p.number, p.base_ref);
        confirm(
            "Merge pull request",
            message,
            "Merge",
            window,
            cx,
            move |_, cx| {
                let (branch, head_sha) = (branch.clone(), p.head_sha.clone());
                weak.update(cx, |this, cx| {
                    let (target, number) = (this.target.clone(), this.number);
                    let then = GithubRequest::OfferWorktreeRemoval {
                        branch,
                        number,
                        head_sha: head_sha.clone(),
                    };
                    // The head shown: a push since makes GitHub refuse rather than merge what nobody saw.
                    let sha = Some(head_sha);
                    this.act(
                        "Merge",
                        format!("Merged #{number}"),
                        move |core| {
                            done(api::github::pr_merge(
                                core,
                                target,
                                number,
                                method.as_str().into(),
                                sha,
                            ))
                        },
                        move |this, _| this.pending_request = Some(then),
                        cx,
                    );
                })
                .ok();
            },
        );
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (target, number) = (self.target.clone(), self.number);
        let run = move |this: &mut PullView, cx: &mut Context<PullView>| {
            let verb = if open { "Reopened" } else { "Closed" };
            let target = target.clone();
            this.act(
                if open { "Reopen" } else { "Close" },
                format!("{verb} #{number}"),
                move |core| done(api::github::pr_set_open(core, target, number, open)),
                |_, _| {},
                cx,
            );
        };
        if open {
            return run(self, cx);
        }
        let weak = cx.weak_entity();
        confirm(
            "Close pull request",
            format!("Close #{number} without merging?"),
            "Close",
            window,
            cx,
            move |_, cx| {
                weak.update(cx, |this, cx| run(this, cx)).ok();
            },
        );
    }

    /// GitHub can't merge it: bring the conflicts home. Check out the PR branch and merge the base
    /// into it; the conflicts open in Changes like any local merge. In a fork's original, the base
    /// comes from the remote pointing at it (added if missing).
    fn resolve_locally(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.item() else { return };
        let account = self.account(cx);
        let in_origin = account
            .as_ref()
            .is_some_and(|a| a.access_for(&p.url).is_some_and(|x| a.is_origin(x)));
        let same = pull_source(&p, account.as_deref()).is_some_and(|s| s.same_repo);
        let target = self.target.clone().unwrap_or_default();
        let from = if in_origin {
            format!("origin/{}", p.base_ref)
        } else {
            format!("{target}'s {}", p.base_ref)
        };
        let message = format!(
            "Check out {} and merge {from} into it? Conflicts will open in Changes; push when done.",
            p.head_ref
        );
        let weak = cx.weak_entity();
        confirm(
            "Resolve conflicts locally",
            message,
            "Start",
            window,
            cx,
            move |_, cx| {
                let (p, target) = (p.clone(), target.clone());
                weak.update(cx, |this, cx| {
                    let message =
                        format!("Merged {} into {}; review and push", p.base_ref, p.head_ref);
                    this.act(
                        "Merge",
                        message,
                        move |core| {
                            let number = p.number;
                            api::github::pr_checkout(
                                core,
                                Some(target.clone()),
                                number,
                                p.head_ref.clone(),
                                same,
                                format!("pr-checkout-{number}"),
                                |_| {},
                            )?;
                            let base = if in_origin {
                                api::sync::fetch(core, "pr-resolve-fetch".into(), |_| {})?;
                                format!("origin/{}", p.base_ref)
                            } else {
                                let remote = match api::github::gh_original_remote(
                                    core,
                                    target.clone(),
                                    true,
                                    "pr-resolve-original".into(),
                                    |_| {},
                                )? {
                                    Some(r) => r,
                                    None => api::github::gh_add_original_remote(
                                        core,
                                        "pr-resolve-add".into(),
                                        |_| {},
                                    )?,
                                };
                                format!("{remote}/{}", p.base_ref)
                            };
                            // True: it stopped on conflicts, which is what this was for.
                            api::sync::merge(core, base, None, None)
                        },
                        |_, _| {},
                        cx,
                    );
                })
                .ok();
            },
        );
    }

    fn review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.item() else { return };
        let account = self.account(cx);
        let rights = pull_rights(&p, account.as_deref(), None);
        let counts = account
            .as_ref()
            .and_then(|a| a.access_for(&p.url))
            .is_some_and(|a| a.push);
        let body = match &self.review {
            Some(b) => b.clone(),
            None => {
                let b = cx.new(|cx| {
                    MarkdownInput::new(self.web.clone(), self.url.clone(), window, cx, |t| {
                        t.rows(4).placeholder("Leave a comment (markdown)")
                    })
                    .small()
                    .signed_by(self.target.clone(), self.number)
                });
                // Text in it keeps the tab: a preview tab would be replaced, a hidden one released.
                let input = b.read(cx).input().clone();
                let sub = cx.subscribe(&input, |this: &mut Self, b, ev: &InputEvent, cx| {
                    if matches!(ev, InputEvent::Change) && !b.read(cx).value().trim().is_empty() {
                        let key = this.key();
                        this.ws.update(cx, |ws, cx| ws.pin_tab(&key, cx)).ok();
                    }
                });
                self._review = Some(sub);
                self.review = Some(b.clone());
                b
            }
        };
        ReviewDialog::open(
            cx.entity(),
            body,
            self.review_pick,
            rights.own,
            counts,
            window,
            cx,
        );
    }

    /// From the review dialog. The dialog stays until it lands; a refused one keeps its text.
    pub(crate) fn submit_review(&mut self, event: ReviewEvent, cx: &mut Context<Self>) {
        let Some(body) = self.review.clone() else {
            return;
        };
        self.review_pick = Some(event);
        let text = body.read(cx).value(cx).trim().to_string();
        let (target, number) = (self.target.clone(), self.number);
        // The head reviewed: an approval stays on what was read.
        let commit = self.detail.as_ref().map(|d| d.pull.head_sha.clone());
        self.act(
            "Review",
            event.done().into(),
            move |core| {
                done(api::github::pr_review(
                    core,
                    target,
                    number,
                    event.as_str().into(),
                    text,
                    commit,
                ))
            },
            |this, cx| {
                this.reviews_done += 1;
                this.review = None;
                this._review = None;
                this.review_pick = None;
                cx.notify();
            },
            cx,
        );
    }

    pub(crate) fn reviews_done(&self) -> u64 {
        self.reviews_done
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.busy.is_some()
    }

    fn show_failure(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.failures.remove(&id).is_some() {
            cx.notify();
            return;
        }
        self.read_failure(id, cx);
    }

    /// A finished check run doesn't change (a re-run is a new one): read once, Retry reads again.
    fn read_failure(&mut self, id: u64, cx: &mut Context<Self>) {
        let (core, target) = (crate::core(cx), self.target.clone());
        let task = cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move { api::github::check_failure(&core, target, id) })
                .await;
            this.update(cx, |this, cx| {
                if let Some(f) = this.failures.get_mut(&id) {
                    match read {
                        Ok(d) => {
                            f.data = Some(Arc::new(d));
                            f.error = None;
                        }
                        Err(e) => f.error = Some(e),
                    }
                }
                cx.notify();
            })
            .ok();
        });
        let old = self.failures.remove(&id).and_then(|f| f.data);
        self.failures.insert(
            id,
            Failure {
                data: old,
                error: None,
                log: ScrollHandle::new(),
                _task: task,
            },
        );
        cx.notify();
    }

    fn open_file(
        &self,
        file: &FileChange,
        narrowed: Option<(String, String, String)>,
        preview: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(f) = &self.files else { return };
        // The picked commits' lines aren't the head's: no url, so no line comments.
        let range = match narrowed {
            Some((base, head, label)) => PullRange {
                number: Some(self.number),
                pull_url: None,
                label: Some(label),
                base,
                head,
            },
            None => PullRange {
                number: Some(self.number),
                pull_url: Some(self.url.clone()),
                label: None,
                base: f.base.clone(),
                head: f.head.clone(),
            },
        };
        crate::ui::open_pull_file(&self.ws, range, file.clone(), preview, window, cx);
    }
}

/// "Check out in new worktree…" without the worktree dialog (no handler set): core makes it
/// beside the main one, and the window switches to it.
pub(crate) fn checkout_worktree(source: PullSource, window: &mut Window, cx: &mut App) {
    let core = crate::core(cx);
    let ws = Workspace::for_window(window, cx).map(|w| w.downgrade());
    let PullSource {
        target,
        number,
        head_ref,
        same_repo,
        ..
    } = source;
    cx.spawn(async move |cx| {
        let made = cx
            .background_spawn(async move {
                api::github::pr_checkout_worktree(
                    &core,
                    Some(target),
                    number,
                    head_ref,
                    same_repo,
                    None,
                    format!("pr-worktree-{number}"),
                    |_| {},
                )
            })
            .await;
        cx.update(|cx| {
            let Some(ws) = ws.and_then(|w| w.upgrade()) else {
                return;
            };
            ws.update(cx, |ws, cx| match made {
                Ok(path) => {
                    ws.toast(
                        Toast::success(format!("Checked out #{number} in {path}")),
                        cx,
                    );
                    ws.switch_worktree(path.into(), cx);
                }
                Err(e) => ws.toast(Toast::error(e).title("Could not make the worktree"), cx),
            });
        });
    })
    .detach();
}

impl TabItem for PullView {
    fn tab_title(&self, _: &App) -> SharedString {
        match self.item() {
            Some(p) => format!("#{} {}", p.number, p.title).into(),
            None => format!("#{}", self.number).into(),
        }
    }

    fn tab_icon(&self, _: &App) -> TabIcon {
        TabIcon::Icon(IconName::GitPullRequest)
    }

    fn tab_tooltip(&self, _: &App) -> Option<SharedString> {
        Some(self.url.clone().into())
    }

    fn tab_focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.focus.clone())
    }

    /// A review being written: closing asks, and the tab isn't released.
    fn is_dirty(&self, cx: &App) -> bool {
        self.review
            .as_ref()
            .is_some_and(|b| !b.read(cx).value(cx).trim().is_empty())
    }
}

fn branch_chip(name: impl IntoElement, t: &AppTheme) -> AnyElement {
    div()
        .flex_none()
        .px(px(6.))
        .rounded(px(3.))
        .bg(t.primary.opacity(0.12))
        .text_color(t.primary)
        .font_family(t.fonts.code.clone())
        .text_size(rems(0.69))
        .child(name)
        .into_any_element()
}

fn check_icon(state: &str, t: &AppTheme) -> Icon {
    let (name, color) = if state == "success" {
        (IconName::Check, t.added)
    } else if state == "pending" {
        (IconName::CircleDashed, t.modified)
    } else if is_failing(state) {
        (IconName::X, t.removed)
    } else {
        (IconName::CircleMinus, t.subtle)
    };
    Icon::new(name).xsmall().text_color(color)
}

/// The line under the title: who, and what they did or want.
fn who_and_verb(p: &PullItem, d: Option<&PullDetail>) -> (String, &'static str) {
    match p.state {
        PullState::Merged => (
            d.and_then(|d| d.merged_by.clone())
                .unwrap_or(p.author.clone()),
            "merged",
        ),
        PullState::Closed => (p.author.clone(), "wanted to merge"),
        PullState::Open => (p.author.clone(), "wants to merge"),
    }
}

fn commit_count(n: u64) -> String {
    format!("{n} commit{}", if n == 1 { "" } else { "s" })
}

const NO_DESCRIPTION: &str = "No description provided.";
const READING: &str = "Reading its output and log…";
const NO_OUTPUT: &str = "The check left no output; its Details page may say more.";

enum Tone {
    Quiet,
    Conflict,
    Warn,
    Ready,
}

fn has_conflicts(d: &PullDetail) -> bool {
    d.mergeable == Some(false) || d.mergeable_state == "dirty"
}

/// Where the PR stands for merging: the box's tone, title and note.
fn merge_words(d: &PullDetail) -> (Tone, String, &'static str) {
    let st = d.mergeable_state.as_str();
    let base = &d.pull.base_ref;
    if d.mergeable.is_none() {
        (
            Tone::Quiet,
            "Checking mergeability…".into(),
            "GitHub is computing whether this can merge. Refresh in a moment.",
        )
    } else if d.pull.draft && !has_conflicts(d) {
        (
            Tone::Quiet,
            "This is a draft".into(),
            "GitHub won't merge it until it's marked ready for review.",
        )
    } else if has_conflicts(d) {
        (
            Tone::Conflict,
            format!("This branch has conflicts with {base}"),
            "Resolve them locally: GitViber merges the base into this branch and opens the conflicts.",
        )
    } else if st == "blocked" {
        (
            Tone::Warn,
            "Merging is blocked".into(),
            "Required reviews or checks haven't passed yet.",
        )
    } else if st == "behind" {
        (
            Tone::Warn,
            format!("This branch is behind {base}"),
            "It can still merge, but the base has newer commits.",
        )
    } else if st == "unstable" {
        (
            Tone::Warn,
            "Checks are failing or pending".into(),
            "You can still merge if the repository allows it.",
        )
    } else {
        (
            Tone::Ready,
            "Ready to merge".into(),
            "No conflicts with the base branch.",
        )
    }
}

/// What the merge box says in place of its button, when you can't use it.
fn merge_side(d: &PullDetail, rights: &PullRights) -> Option<&'static str> {
    if has_conflicts(d) {
        // A fork's branch lives in another repo; pushing the fix here would go to the wrong place.
        (!rights.resolve).then_some("From a fork: resolve it in the fork's repository.")
    } else {
        (!rights.merge).then_some("Only people with write access can merge.")
    }
}

fn checks_error(e: &str) -> String {
    format!("Could not load all checks: {e}")
}

fn failure_error(e: &str) -> String {
    format!("Could not read the check: {e}")
}

fn annotations_error(e: &str) -> String {
    format!("Could not read the annotations: {e}")
}

fn log_error(e: &str) -> String {
    format!("Could not read the job's log: {e}")
}

fn no_output(f: &CheckFailure) -> bool {
    f.title.is_empty()
        && f.summary.is_empty()
        && f.annotations.is_empty()
        && f.log.is_none()
        && f.log_error.is_none()
        && f.annotations_error.is_none()
}

/// A part's index for check row `row`'s annotation `i`.
fn nested(row: usize, i: usize) -> usize {
    (row << 20) | i
}

fn annotation_place(a: &Annotation) -> String {
    if a.line > 0 {
        format!("{}:{}", a.path, a.line)
    } else {
        a.path.clone()
    }
}

/// Actions puts its own errors ("exit code 1") on .github, a folder: no place to open.
fn shows_place(a: &Annotation) -> bool {
    !a.path.is_empty() && a.path != ".github"
}

/// The log's last 40 lines: the error is at its end.
fn log_tail(log: &str) -> &str {
    let log = log.trim_end_matches('\n');
    let start = log
        .match_indices('\n')
        .rev()
        .nth(39)
        .map_or(0, |(i, _)| i + 1);
    &log[start..]
}

/// Each line's bytes in `text`, without its line break.
fn lines(text: &str) -> impl Iterator<Item = std::ops::Range<usize>> + '_ {
    let mut at = 0;
    text.split_inclusive('\n').map(move |l| {
        let start = at;
        at += l.len();
        start..start + l.trim_end_matches(['\n', '\r']).len()
    })
}

fn commits_on(day: &str) -> String {
    format!("Commits on {day}")
}

fn committed(c: &gitviber_core::git::Commit) -> SharedString {
    gitviber_ui::widgets::relative_time(c.committed_at, gitviber_ui::widgets::now_unix()).into()
}

fn latest(shown: usize, total: u64) -> String {
    format!("The latest {shown} of {total} commits.")
}

fn showing_from(n: usize) -> String {
    format!(
        "Showing changes from {n} commit{}",
        if n == 1 { "" } else { "s" }
    )
}

/// A file's path, its folder dimmed; split in two pieces of one part while the find marks it.
fn path_label(marks: &Marks, ix: usize, path: &str, cx: &App) -> AnyElement {
    let key = ("file", ix);
    if !marks.any(key, cx) {
        return PathLabel(path.to_string().into()).into_any_element();
    }
    let t = theme(cx);
    let cut = path.rfind('/').map_or(0, |i| i + 1);
    div()
        .flex()
        .min_w_0()
        .overflow_hidden()
        .when(cut > 0, |d| {
            d.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(t.subtle)
                    .child(marks.slice(key, path, 0..cut, cx)),
            )
        })
        .child(
            div()
                .flex_none()
                .max_w_full()
                .truncate()
                .text_color(t.foreground)
                .child(marks.slice(key, path, cut..path.len(), cx)),
        )
        .into_any_element()
}

impl PullView {
    fn merge_box(
        &self,
        d: &PullDetail,
        account: Option<&Account>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = theme(cx);
        let marks = self.find.marks();
        let p = PullItem::from(&d.pull);
        let rights = pull_rights(&p, account, d.closed_by.as_deref());
        let (tone, title, note) = merge_words(d);
        let tone = match tone {
            Tone::Quiet => t.border,
            Tone::Conflict => t.conflict.opacity(0.6),
            Tone::Warn => t.modified.opacity(0.6),
            Tone::Ready => t.added.opacity(0.6),
        };
        let conflicts = has_conflicts(d);
        let busy = self.busy.is_some();
        let action: AnyElement = if let Some(side) = merge_side(d, &rights) {
            div()
                .max_w(px(220.))
                .text_right()
                .text_size(rems(0.72))
                .text_color(t.muted_foreground)
                .child(marks.text(("merge-side", 0), side, cx))
                .into_any_element()
        } else if conflicts {
            Button::new("resolve")
                .small()
                .primary()
                .icon(IconName::GitMerge)
                .label("Resolve locally")
                .disabled(busy)
                .on_click(cx.listener(|this, _, window, cx| this.resolve_locally(window, cx)))
                .into_any_element()
        } else {
            let methods = allowed_methods(account.and_then(|a| a.access_for(&p.url)));
            let first = methods[0];
            let off = busy || d.mergeable.is_none() || d.pull.draft;
            let weak = cx.weak_entity();
            div()
                .flex()
                .gap(px(2.))
                .child(
                    Button::new("merge")
                        .small()
                        .primary()
                        .icon(IconName::GitMerge)
                        .label(first.button())
                        .disabled(off)
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.merge(first, window, cx)),
                        ),
                )
                .child(
                    Button::new("merge-method")
                        .small()
                        .primary()
                        .icon(IconName::ChevronDown)
                        .disabled(off)
                        .dropdown_menu(move |menu, _, _| {
                            let mut menu = menu.label("Merge method");
                            for m in methods.clone() {
                                let weak = weak.clone();
                                menu = menu.item(PopupMenuItem::new(m.label()).on_click(
                                    move |_, window, cx| {
                                        weak.update(cx, |this, cx| this.merge(m, window, cx)).ok();
                                    },
                                ));
                            }
                            menu
                        }),
                )
                .into_any_element()
        };
        div()
            .mt(px(16.))
            .flex()
            .items_center()
            .gap(px(12.))
            .rounded(px(6.))
            .border_1()
            .border_color(tone)
            .bg(t.panel)
            .p(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(rems(0.78))
                            .font_weight(t.semibold())
                            .child(marks.text(("merge-title", 0), title, cx)),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .text_size(rems(0.72))
                            .text_color(t.muted_foreground)
                            .child(marks.text(("merge-note", 0), note, cx)),
                    ),
            )
            .child(action)
            .into_any_element()
    }

    fn checks_section(&self, d: &PullDetail, cx: &mut Context<Self>) -> Option<AnyElement> {
        if d.ci.checks.is_empty() && d.ci.checks_error.is_none() {
            return None;
        }
        let t = theme(cx);
        let marks = self.find.marks();
        let checks: Vec<Check> = d.ci.checks.iter().map(Check::from).collect();
        let aside = (!checks.is_empty()).then(|| checks_summary(&checks).into());
        let now = gitviber_ui::widgets::now_unix();
        let mut body = section("Checks", aside, &t);
        if let Some(e) = &d.ci.checks_error {
            body = body.child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .text_size(rems(0.75))
                    .child(error_text("checks-error", &checks_error(e), t.removed, cx)),
            );
        }
        for (i, c) in sort_checks(&checks).into_iter().enumerate() {
            let note = check_note(c, now);
            let shown = c.id.is_some_and(|id| self.failures.contains_key(&id));
            let mut row = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(28.))
                .px(px(12.))
                .text_size(rems(0.75))
                .child(check_icon(&c.state, &t))
                .child(div().min_w_0().truncate().child(marks.text(
                    ("check", i),
                    c.name.clone(),
                    cx,
                )))
                .when(!note.is_empty(), |d| {
                    d.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(rems(0.69))
                            .text_color(t.subtle)
                            .child(marks.text(("check-note", i), note, cx)),
                    )
                })
                .child(div().flex_1());
            // Statuses have no output to show; only check runs do.
            if let Some(id) = c.id.filter(|_| is_failing(&c.state)) {
                row = row.child(
                    text_button(
                        ("show-failure", i),
                        if shown {
                            "Hide failure"
                        } else {
                            "Show failure"
                        },
                        &t,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.show_failure(id, cx))),
                );
            }
            if let Some(url) = c
                .url
                .clone()
                .filter(|u| u.starts_with("https://github.com/"))
            {
                row = row.child(
                    text_button(("details", i), "Details", &t)
                        .on_click(move |_, _, cx| open_url(&url, cx)),
                );
            }
            body = body.child(row);
            if let Some(id) = c.id.filter(|_| shown) {
                body = body.child(self.failure_view(i, id, c, cx));
            }
        }
        Some(body.into_any_element())
    }

    /// Check row `row`'s failure, run `id`.
    fn failure_view(
        &self,
        row: usize,
        id: u64,
        check: &Check,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = theme(cx);
        let marks = self.find.marks();
        let f = &self.failures[&id];
        let mut col = div()
            .border_y_1()
            .border_color(t.border)
            .bg(t.panel.opacity(0.5))
            .px(px(12.))
            .py(px(8.))
            .text_size(rems(0.75));
        let Some(data) = f.data.clone() else {
            return match &f.error {
                Some(e) => col
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().flex_1().child(error_text(
                                ("failure-error", row),
                                &failure_error(e),
                                t.removed,
                                cx,
                            )))
                            .child(
                                Button::new(("retry", id as usize))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::RefreshCw)
                                    .label("Retry")
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| {
                                            this.read_failure(id, cx)
                                        }),
                                    ),
                            ),
                    )
                    .into_any_element(),
                None => col
                    .child(div().text_color(t.subtle).child(marks.text(
                        ("failure-wait", row),
                        READING,
                        cx,
                    )))
                    .into_any_element(),
            };
        };
        if !data.title.is_empty() {
            col = col.child(div().font_weight(t.semibold()).child(marks.text(
                ("failure-title", row),
                data.title.clone(),
                cx,
            )));
        }
        if !data.summary.is_empty() {
            col = col.child(
                div()
                    .py(px(4.))
                    .text_color(t.muted_foreground)
                    .child(marks.text(("failure-summary", row), data.summary.clone(), cx)),
            );
        }
        for (i, a) in data.annotations.iter().enumerate() {
            let at = nested(row, i);
            let color = match a.level.as_str() {
                "failure" => t.removed,
                "warning" => t.modified,
                _ => t.subtle,
            };
            let place = annotation_place(a);
            let (path, line) = (a.path.clone(), (a.line > 0).then_some(a.line as u32));
            col = col.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .py(px(2.))
                    .child(
                        div()
                            .flex_none()
                            .text_size(rems(0.69))
                            .text_color(color)
                            .child(marks.text(("annotation-level", at), a.level.clone(), cx)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(4.))
                            // Actions puts its own errors ("exit code 1") on .github, a folder.
                            .when(shows_place(a), |d| {
                                d.child(
                                    text_button(
                                        ("annotation", i),
                                        marks.text(("annotation-place", at), place, cx),
                                        &t,
                                    )
                                    .text_color(t.primary)
                                    .font_family(t.fonts.code.clone())
                                    .on_click(
                                        move |_, window, cx| {
                                            request(
                                                GithubRequest::OpenFile {
                                                    path: path.clone(),
                                                    line,
                                                },
                                                window,
                                                cx,
                                            )
                                        },
                                    ),
                                )
                            })
                            .when(!a.title.is_empty(), |d| {
                                d.child(div().font_weight(t.semibold()).child(marks.text(
                                    ("annotation-title", at),
                                    format!("{}:", a.title),
                                    cx,
                                )))
                            })
                            .child(marks.text(("annotation-message", at), a.message.clone(), cx)),
                    ),
            );
        }
        if let Some(log) = &data.log {
            // The error is at the log's end: only the tail shows, the whole copies.
            let tail = log_tail(log);
            col = col.child(
                div()
                    .id(("log", id as usize))
                    .mt(px(6.))
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .track_scroll(&f.log)
                    .rounded(px(3.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.background)
                    .p(px(8.))
                    .font_family(t.fonts.code.clone())
                    .text_size(rems(0.69))
                    .children(lines(tail).map(|r| {
                        div().child(marks.slice(("log", row), tail, r, cx).within(&f.log))
                    })),
            );
        }
        if let Some(e) = &data.annotations_error {
            col = col.child(div().mt(px(4.)).child(error_text(
                ("annotations-error", row),
                &annotations_error(e),
                t.removed,
                cx,
            )));
        }
        if let Some(e) = &data.log_error {
            col = col.child(div().mt(px(4.)).child(error_text(
                ("log-error", row),
                &log_error(e),
                t.removed,
                cx,
            )));
        }
        if no_output(&data) {
            col = col.child(div().text_color(t.subtle).child(marks.text(
                ("no-output", row),
                NO_OUTPUT,
                cx,
            )));
        }
        let report = failure_report(&check.name, &self.url, &data);
        let retry = data.log_error.is_some() || data.annotations_error.is_some();
        col.child(
            div()
                .mt(px(8.))
                .flex()
                .gap(px(6.))
                .child(
                    Button::new(("copy-agent", id as usize))
                        .small()
                        .icon(IconName::ClipboardCopy)
                        .label("Copy for agent")
                        .on_click(move |_, window, cx| {
                            copy(
                                &report,
                                "Copied for the agent: paste it into its terminal",
                                window,
                                cx,
                            )
                        }),
                )
                .when(retry, |d| {
                    d.child(
                        Button::new(("retry-part", id as usize))
                            .ghost()
                            .small()
                            .icon(IconName::RefreshCw)
                            .label("Retry")
                            .on_click(cx.listener(move |this, _, _, cx| this.read_failure(id, cx))),
                    )
                }),
        )
        .into_any_element()
    }

    fn commits_section(
        &mut self,
        total: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let files = self.files.clone()?;
        let commits = &files.commits;
        if commits.is_empty() {
            return None;
        }
        let t = theme(cx);
        let marks = self.find.marks();
        let fresh = commits.iter().filter(|c| self.is_new(&c.sha)).count();
        let aside = if fresh > 0 {
            format!("{fresh} new · {}", commits.len())
        } else {
            commits.len().to_string()
        };
        let picked = self.picked().map(|(r, _)| r);
        let shas: Vec<String> = commits.iter().map(|c| c.sha.clone()).collect();
        let states = ci::states(cx, &self.target, &shas);
        let cursor = self
            .commit_at()
            .filter(|_| self.commits_focus.is_focused(window));
        // The open messages as the last frame drew them.
        for (child, sha) in std::mem::take(&mut self.bodies_drawn) {
            if let Some(b) = self.commits_scroll.bounds_for_item(child) {
                self.body_heights.insert(sha, b.size.height - ROW_H);
            }
        }
        self.body_heights
            .retain(|sha, _| self.open_bodies.contains(sha));
        let asked = std::mem::take(&mut self.bodies_asked);
        let view_top = -self.commits_scroll.offset().y;
        let view_h = Some(self.commits_scroll.bounds().size.height)
            .filter(|h| *h > px(0.))
            .unwrap_or(COMMITS_H);
        let near = (view_top - COMMITS_OVERSCAN)..(view_top + view_h + COMMITS_OVERSCAN);
        let shown = |y: Pixels, h: Pixels| y + h > near.start && y < near.end;
        let to = self.commits_to.take().filter(|&ix| ix < commits.len());
        let keep = self.commits_keep;
        let mut rows: Vec<AnyElement> = Vec::new();
        // Lines out of view, one gap of their summed (exact) height.
        let mut gap = px(0.);
        let flush = |rows: &mut Vec<AnyElement>, gap: &mut Pixels| {
            if *gap > px(0.) {
                rows.push(div().flex_none().h(*gap).into_any_element());
                *gap = px(0.);
            }
        };
        let (mut y, mut unmeasured) = (px(0.), false);
        for (n, (day, range)) in by_day(commits).into_iter().enumerate() {
            if keep == Some(CommitLine::Day(n)) || shown(y, DAY_H) {
                flush(&mut rows, &mut gap);
                rows.push(
                    div()
                        .flex()
                        .items_center()
                        .h(DAY_H)
                        .px(px(12.))
                        .bg(t.panel)
                        .border_b_1()
                        .border_color(t.border)
                        .text_size(rems(0.69))
                        .text_color(t.subtle)
                        .child(
                            marks
                                .text(("day", n), commits_on(&day), cx)
                                .within(&self.commits_scroll),
                        )
                        .into_any_element(),
                );
            } else {
                gap += DAY_H;
            }
            y += DAY_H;
            for ix in range {
                let c = &commits[ix];
                let open = self.open_bodies.contains(&c.sha);
                let h = if open {
                    ROW_H
                        + self.body_heights.get(&c.sha).copied().unwrap_or_else(|| {
                            unmeasured = true;
                            px(17. + 18. * lines(&c.body).count() as f32)
                        })
                } else {
                    ROW_H
                };
                // An open message is always drawn: a gap's height is then exact.
                if open || to == Some(ix) || keep == Some(CommitLine::Row(ix)) || shown(y, h) {
                    flush(&mut rows, &mut gap);
                    if to == Some(ix) {
                        self.commits_scroll.scroll_to_item(rows.len());
                    }
                    if open {
                        self.bodies_drawn.push((rows.len(), c.sha.clone()));
                    }
                    let on = picked.as_ref().is_some_and(|r| r.contains(&ix));
                    let state = states.get(&c.sha).copied();
                    rows.push(self.commit_row(ix, c, on, cursor == Some(ix), open, state, cx));
                } else {
                    gap += h;
                }
                y += h;
            }
        }
        flush(&mut rows, &mut gap);
        // A message just opened was placed by a guess: once drawn, place the lines again.
        if unmeasured && !asked {
            self.bodies_asked = true;
            cx.on_next_frame(window, |_, _, cx| cx.notify());
        }
        let mut s = section("Commits", Some(aside.into()), &t);
        if total as usize > commits.len() {
            s = s.child(
                div()
                    .px(px(12.))
                    .py(px(6.))
                    .border_b_1()
                    .border_color(t.border)
                    .text_size(rems(0.72))
                    .text_color(t.subtle)
                    .child(marks.text(("latest", 0), latest(commits.len(), total), cx)),
            );
        }
        // A long list scrolls on its own, so Files changed stays right under it.
        Some(
            s.child(
                div()
                    .id("commits")
                    .track_focus(&self.commits_focus)
                    .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                        this.commit_key(ev, cx);
                        cx.notify();
                    }))
                    .max_h(COMMITS_H)
                    .overflow_y_scroll()
                    .track_scroll(&self.commits_scroll)
                    .children(rows),
            )
            .into_any_element(),
        )
    }

    /// Commit `ix`'s row, with its message under it when that's open.
    #[allow(clippy::too_many_arguments)]
    fn commit_row(
        &self,
        ix: usize,
        c: &gitviber_core::git::Commit,
        on: bool,
        keys_here: bool,
        open: bool,
        ci_state: Option<crate::model::CiState>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = theme(cx);
        let marks = self.find.marks();
        let within = &self.commits_scroll;
        let (sha, body_sha, copy_sha, menu_sha) =
            (c.sha.clone(), c.sha.clone(), c.sha.clone(), c.sha.clone());
        let hover = t.hover;
        let (url, not_in_head, short) = (
            format!("{}/commits/{}", self.url, c.sha),
            c.not_in_head,
            c.short_sha.clone(),
        );
        let weak = cx.weak_entity();
        let row = div()
            .id(("commit", ix))
            .relative()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(ROW_H)
            .px(px(12.))
            .border_b_1()
            .border_color(t.border)
            .text_size(rems(0.75))
            .cursor_pointer()
            .when(on, |d| {
                d.bg(t.primary.opacity(0.15)).child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .bg(t.primary),
                )
            })
            .when(!on, |d| d.hover(move |s| s.bg(hover)))
            .when(keys_here && !on, |d| d.bg(hover))
            .when(keys_here, |d| d.border_1().border_color(t.ring))
            .child(
                Icon::new(IconName::GitCommitHorizontal)
                    .xsmall()
                    .text_color(t.subtle),
            )
            .child(
                div().min_w_0().truncate().child(
                    marks
                        .text(("subject", ix), c.subject.clone(), cx)
                        .within(within),
                ),
            )
            .when(!c.body.is_empty(), |d| {
                d.child(
                    div()
                        .id(("commit-body", ix))
                        .flex_none()
                        .px(px(4.))
                        .rounded(px(3.))
                        .bg(if open { t.active } else { t.hover })
                        .text_size(rems(0.625))
                        .cursor_pointer()
                        .child("…")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if !this.open_bodies.remove(&body_sha) {
                                this.open_bodies.insert(body_sha.clone());
                            }
                            cx.notify();
                        })),
                )
            })
            .when(self.is_new(&c.sha), |d| {
                d.child(
                    div()
                        .flex_none()
                        .px(px(6.))
                        .rounded_full()
                        .bg(t.primary.opacity(0.15))
                        .text_color(t.primary)
                        .text_size(rems(0.625))
                        .child("New"),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .max_w(px(140.))
                    .truncate()
                    .text_size(rems(0.69))
                    .text_color(t.subtle)
                    .child(
                        marks
                            .text(("commit-author", ix), c.author_name.clone(), cx)
                            .within(within),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(rems(0.69))
                    .text_color(t.subtle)
                    .child(
                        marks
                            .text(("commit-time", ix), committed(c), cx)
                            .within(within),
                    ),
            )
            .children(ci_badge(("commit-ci", ix), ci_state, &t))
            .child(
                text_button(
                    ("sha", ix),
                    marks.text(("sha", ix), short, cx).within(within),
                    &t,
                )
                .font_family(t.fonts.code.clone())
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    copy(&copy_sha, "SHA copied", window, cx)
                }),
            )
            .on_click(cx.listener(move |this, ev: &gpui_kit::ClickEvent, _, cx| {
                this.click_commit(sha.clone(), ev.modifiers().shift, cx)
            }))
            .context_menu(move |menu, _, _| {
                let (copy_sha, url, sha, weak) = (
                    menu_sha.clone(),
                    url.clone(),
                    menu_sha.clone(),
                    weak.clone(),
                );
                let pick_sha = sha.clone();
                let pick_weak = weak.clone();
                menu.item(
                    PopupMenuItem::new("Copy SHA")
                        .icon(IconName::Copy)
                        .on_click(move |_, window, cx| copy(&copy_sha, "SHA copied", window, cx)),
                )
                .item(
                    PopupMenuItem::new("Open on GitHub")
                        .icon(IconName::ExternalLink)
                        .on_click(move |_, _, cx| open_url(&url, cx)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("Checkout commit")
                        .icon(IconName::GitCommitHorizontal)
                        .on_click(move |_, window, cx| {
                            let (sha, weak) = (sha.clone(), weak.clone());
                            let short = sha[..sha.len().min(7)].to_string();
                            confirm(
                                "Checkout commit",
                                format!(
                                    "Check out {short} without a branch (detached HEAD)? \
                                     New commits made there belong to no branch until \
                                     you create one."
                                ),
                                "Checkout",
                                window,
                                cx,
                                move |_, cx| {
                                    let sha = sha.clone();
                                    weak.update(cx, |this, cx| {
                                        this.act(
                                            "Checkout",
                                            format!("Checked out {short}"),
                                            move |core| {
                                                done(api::history::checkout_commit(core, sha))
                                            },
                                            |_, _| {},
                                            cx,
                                        )
                                    })
                                    .ok();
                                },
                            );
                        }),
                )
                // Your branch has it already (the PR's own, checked out): picking it changes nothing.
                .item(
                    PopupMenuItem::new("Cherry-pick onto current branch")
                        .icon(IconName::Cherry)
                        .disabled(!not_in_head)
                        .on_click(move |_, _, cx| {
                            let sha = pick_sha.clone();
                            pick_weak
                                .update(cx, |this, cx| {
                                    this.act(
                                        "Cherry-pick",
                                        format!("Cherry-picked {}", &sha[..sha.len().min(7)]),
                                        move |core| api::history::cherry_pick(core, sha),
                                        |_, _| {},
                                        cx,
                                    )
                                })
                                .ok();
                        }),
                )
            });
        if !open {
            return row.into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .child(row)
            .child(
                div()
                    .border_b_1()
                    .border_color(t.border)
                    .bg(t.panel.opacity(0.5))
                    .py(px(8.))
                    .pr(px(12.))
                    .pl(px(34.))
                    .font_family(t.fonts.code.clone())
                    .text_size(rems(0.72))
                    .text_color(t.muted_foreground)
                    .children(lines(&c.body).map(|r| {
                        div().child(marks.slice(("body", ix), &c.body, r, cx).within(within))
                    })),
            )
            .into_any_element()
    }

    fn pick_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (range, _) = self.picked()?;
        let t = theme(cx);
        let marks = self.find.marks();
        let commits = &self.commits()[range.clone()];
        let one = commits.len() == 1;
        let n = commits.len();
        let can = |dir: isize| {
            let at = if dir < 0 {
                range.start as isize - 1
            } else {
                range.end as isize
            };
            at >= 0 && (at as usize) < self.commits().len()
        };
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(32.))
                .px(px(12.))
                .border_b_1()
                .border_color(t.border)
                .bg(t.primary.opacity(0.08))
                .text_size(rems(0.72))
                .child(
                    Icon::new(IconName::GitCommitHorizontal)
                        .xsmall()
                        .text_color(t.primary),
                )
                .child(
                    div()
                        .flex_none()
                        .text_color(t.muted_foreground)
                        .child(marks.text(("pick-from", 0), showing_from(n), cx)),
                )
                .child(
                    div()
                        .flex_none()
                        .font_family(t.fonts.code.clone())
                        .child(marks.text(("pick-label", 0), pick_label(commits), cx)),
                )
                .when(one, |d| {
                    d.child(div().min_w_0().truncate().child(marks.text(
                        ("pick-subject", 0),
                        commits[0].subject.clone(),
                        cx,
                    )))
                })
                .child(div().flex_1())
                .when(one, |d| {
                    d.child(
                        Button::new("prev-commit")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ChevronLeft)
                            .tooltip("Previous commit (←)")
                            .disabled(!can(-1))
                            .on_click(cx.listener(|this, _, _, cx| this.step(-1, cx))),
                    )
                    .child(
                        Button::new("next-commit")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ChevronRight)
                            .tooltip("Next commit (→)")
                            .disabled(!can(1))
                            .on_click(cx.listener(|this, _, _, cx| this.step(1, cx))),
                    )
                })
                .child(
                    Button::new("show-all")
                        .ghost()
                        .xsmall()
                        .label("Show all")
                        .on_click(cx.listener(|this, _, _, cx| this.set_pick(None, cx))),
                )
                .into_any_element(),
        )
    }

    /// The files Files changed lists: the PR's, or the picked commits'; None until they're read.
    fn shown_files(&self) -> Option<Shown> {
        match self.picked() {
            Some((range, true)) => self.picked_files.as_ref().map(|(b, h, files)| {
                (
                    files.clone(),
                    Some((b.clone(), h.clone(), pick_label(&self.commits()[range]))),
                )
            }),
            _ => self.files.as_ref().map(|_| (self.file_list.clone(), None)),
        }
    }

    /// What Files changed says while it has no files to list, and whether that's an error.
    fn files_note(&self) -> Option<(String, bool)> {
        if let Some((files, _)) = self.shown_files() {
            return files
                .is_empty()
                .then(|| ("No files changed.".into(), false));
        }
        let narrowed = self.picked().is_some_and(|(_, n)| n);
        let error = if narrowed {
            &self.picked_error
        } else {
            &self.files_error
        };
        Some(match error {
            Some(e) if narrowed => (format!("Could not load the commits' files: {e}"), true),
            Some(e) => (format!("Could not load the PR's files: {e}"), true),
            None if narrowed => ("Loading…".into(), false),
            None => ("Fetching the PR's commits…".into(), false),
        })
    }

    fn files_section(&self, d: Option<&PullDetail>, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx);
        let marks = self.find.marks();
        let narrowed = self.picked().is_some_and(|(_, n)| n);
        let shown = self.shown_files();
        let aside = match (&shown, d) {
            (Some((files, _)), _) => Some(files.len().to_string().into()),
            (None, Some(d)) if !narrowed => Some(d.changed_files.to_string().into()),
            _ => None,
        };
        let mut s = section("Files changed", aside, &t).children(self.pick_bar(cx));
        if let Some((note, error)) = self.files_note() {
            let note = if error {
                error_text("files-error", &note, t.removed, cx).into_any_element()
            } else {
                marks.text(("files-note", 0), note, cx).into_any_element()
            };
            return s
                .child(
                    div()
                        .px(px(12.))
                        .py(px(8.))
                        .text_size(rems(0.75))
                        .text_color(t.subtle)
                        .child(note),
                )
                .into_any_element();
        }
        if let Some((files, range)) = shown {
            let weak = cx.weak_entity();
            let count = files.len();
            let list = uniform_list("pull-files", count, move |r, _, cx| {
                let t = theme(cx);
                r.map(|ix| {
                    let f = &files[ix];
                    let (file, range, weak) = (f.clone(), range.clone(), weak.clone());
                    let hover = t.hover;
                    div()
                        .id(("file", ix))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .h(px(28.))
                        .px(px(12.))
                        .text_size(rems(0.75))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .child(FileIcon::file(&f.path))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(path_label(&marks, ix, &f.path, cx)),
                        )
                        .child(LineCounts::new(f.additions, f.deletions))
                        .child(StatusLetter(gitviber_ui::widgets::ChangeKind::from_letter(
                            f.status.chars().next().unwrap_or('M'),
                        )))
                        .on_click(move |ev, window, cx| {
                            let pin = ev.click_count() > 1;
                            weak.update(cx, |this, cx| {
                                this.open_file(&file, range.clone(), !pin, window, cx)
                            })
                            .ok();
                        })
                })
                .collect::<Vec<_>>()
            })
            .track_scroll(&self.files_scroll)
            .h(px(28. * count.min(14) as f32));
            s = s.child(list);
        }
        s.into_any_element()
    }

    /// Comment `i`, as its item of the page's list: a slice of the Conversation section.
    fn comment(&self, d: &PullDetail, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(c) = d.comments.get(i) else {
            return div().into_any_element();
        };
        let t = theme(cx);
        let marks = self.find.marks();
        let md = self.comment_view(d, i);
        let verdict = c.review.as_deref().map(|r| {
            let color = match r {
                "APPROVED" => t.added,
                "CHANGES_REQUESTED" => t.removed,
                "DISMISSED" => t.subtle,
                _ => t.muted_foreground,
            };
            div()
                .text_size(rems(0.69))
                .text_color(color)
                .child(marks.text(("verdict", i), review_badge(r), cx))
        });
        let title = (i == 0).then(|| ("Conversation", Some(d.comments.len().to_string().into())));
        section_part(title, i + 1 == d.comments.len(), &t)
            .child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(rems(0.72))
                            .child(div().font_weight(t.semibold()).child(marks.text(
                                ("comment-author", i),
                                c.author.clone(),
                                cx,
                            )))
                            .children(verdict)
                            .child(div().text_color(t.subtle).child(marks.text(
                                ("comment-time", i),
                                ago(&c.created_at),
                                cx,
                            ))),
                    )
                    .children(md.map(|md| div().mt(px(6.)).child(md))),
            )
            .into_any_element()
    }
}

impl FindHost for PullView {
    /// The page top to bottom: the title and the line under it, the merge box, checks and the
    /// failures shown, commits with their messages (folded or not), the files, the description
    /// and the conversation.
    fn find_parts(&self, parts: &mut Parts, cx: &App) {
        let Some(p) = self.item() else { return };
        let d = self.detail.as_deref();
        let account = self.account(cx);
        parts.text(("title", 0), &p.title);
        parts.text(("number", 0), &format!("#{}", p.number));
        let (who, verb) = who_and_verb(&p, d);
        parts.text(("who", 0), &who);
        parts.text(("verb", 0), verb);
        parts.text(("head", 0), &p.head_ref);
        parts.text(("into", 0), "into");
        parts.text(("base", 0), &p.base_ref);
        if let Some(d) = d {
            parts.text(("commit-count", 0), &commit_count(d.commits));
        }
        if let Some(d) = d {
            if p.state == PullState::Open {
                let (_, title, note) = merge_words(d);
                parts.text(("merge-title", 0), &title);
                parts.text(("merge-note", 0), note);
                let rights = pull_rights(&p, account.as_deref(), d.closed_by.as_deref());
                if let Some(side) = merge_side(d, &rights) {
                    parts.text(("merge-side", 0), side);
                }
            }
            let checks: Vec<Check> = d.ci.checks.iter().map(Check::from).collect();
            let now = gitviber_ui::widgets::now_unix();
            for (row, c) in sort_checks(&checks).into_iter().enumerate() {
                parts.text(("check", row), &c.name);
                parts.text(("check-note", row), &check_note(c, now));
                if let Some(f) = c.id.and_then(|id| self.failures.get(&id)) {
                    self.failure_parts(row, f, parts);
                }
            }
        }
        let commits = self.commits();
        if !commits.is_empty() {
            let total = d.map_or(0, |d| d.commits).max(commits.len() as u64);
            if total as usize > commits.len() {
                parts.text(("latest", 0), &latest(commits.len(), total));
            }
            let days = by_day(commits);
            for (n, (day, range)) in days.into_iter().enumerate() {
                parts.text(("day", n), &commits_on(&day));
                for ix in range {
                    let c = &commits[ix];
                    parts.text(("subject", ix), &c.subject);
                    parts.text(("commit-author", ix), &c.author_name);
                    parts.text(("commit-time", ix), &committed(c));
                    parts.text(("sha", ix), &c.short_sha);
                    parts.text(("body", ix), &c.body);
                }
            }
        }
        if let Some((range, _)) = self.picked() {
            let picked = &commits[range];
            parts.text(("pick-from", 0), &showing_from(picked.len()));
            parts.text(("pick-label", 0), &pick_label(picked));
            if let [one] = picked {
                parts.text(("pick-subject", 0), &one.subject);
            }
        }
        match (self.files_note(), self.shown_files()) {
            (Some((_, true)), _) => {}
            (Some((note, false)), _) => parts.text(("files-note", 0), &note),
            (None, Some((files, _))) => {
                for (ix, f) in files.iter().enumerate() {
                    parts.text(("file", ix), &f.path);
                }
            }
            (None, None) => {}
        }
        match (self.body_view(), d) {
            (Some(v), _) => parts.view(&v),
            (None, Some(_)) => parts.text(("no-description", 0), NO_DESCRIPTION),
            (None, None) => {}
        }
        for (i, c) in d.into_iter().flat_map(|d| d.comments.iter().enumerate()) {
            parts.text(("comment-author", i), &c.author);
            if let Some(r) = &c.review {
                parts.text(("verdict", i), &review_badge(r));
            }
            parts.text(("comment-time", i), &ago(&c.created_at));
            if let Some(v) = d.and_then(|d| self.comment_view(d, i)) {
                parts.view(&v);
            }
        }
    }

    /// The page's list draws the item it's in; a commit's line is drawn wherever it is in its
    /// list and its message opens; a file's row comes into the list's view.
    fn show_part(&mut self, (what, ix): Key, cx: &mut Context<Self>) {
        match what {
            "body" => {
                if let Some(c) = self.commits().get(ix) {
                    let sha = c.sha.clone();
                    self.open_bodies.insert(sha);
                }
                self.commits_keep = Some(CommitLine::Row(ix));
            }
            "subject" | "commit-author" | "commit-time" | "sha" => {
                self.commits_keep = Some(CommitLine::Row(ix))
            }
            "day" => self.commits_keep = Some(CommitLine::Day(ix)),
            "file" => self.files_scroll.scroll_to_item(ix, ScrollStrategy::Center),
            _ => {}
        }
        let item = match what {
            "comment-author" | "verdict" | "comment-time" => ix + 1,
            _ => 0,
        };
        self.reveal_item(item);
        cx.notify();
    }

    /// The description is in the page's first item, comment n's view in item n + 1.
    fn show_view(&mut self, view: EntityId, cx: &mut Context<Self>) {
        if let Some(item) = self.blocks.position(view) {
            self.reveal_item(item);
            cx.notify();
        }
    }
}

impl PullView {
    /// What a shown failure (check row `row`'s) says, as its view draws it.
    fn failure_parts(&self, row: usize, f: &Failure, parts: &mut Parts) {
        let Some(data) = &f.data else {
            if f.error.is_none() {
                parts.text(("failure-wait", row), READING);
            }
            return;
        };
        parts.text(("failure-title", row), &data.title);
        parts.text(("failure-summary", row), &data.summary);
        for (i, a) in data.annotations.iter().enumerate() {
            let at = nested(row, i);
            parts.text(("annotation-level", at), &a.level);
            if shows_place(a) {
                parts.text(("annotation-place", at), &annotation_place(a));
            }
            if !a.title.is_empty() {
                parts.text(("annotation-title", at), &format!("{}:", a.title));
            }
            parts.text(("annotation-message", at), &a.message);
        }
        if let Some(log) = &data.log {
            parts.text(("log", row), log_tail(log));
        }
        if no_output(data) {
            parts.text(("no-output", row), NO_OUTPUT);
        }
    }
}

impl PullView {
    /// The page's first item: the PR, where it stands, its checks, commits, files and
    /// description.
    fn head(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(p) = self.item() else {
            return div().into_any_element();
        };
        let t = theme(cx);
        let marks = self.find.marks();
        let account = self.account(cx);
        let d = self.detail.clone();
        let rights = pull_rights(
            &p,
            account.as_deref(),
            d.as_ref().and_then(|d| d.closed_by.as_deref()),
        );
        let source = pull_source(&p, account.as_deref());
        let busy = self.busy.is_some();
        let open = p.state == PullState::Open;
        let (who, verb) = who_and_verb(&p, d.as_deref());
        let head_ci = ci::state(cx, &self.target, &p.head_sha);
        let meta = div()
            .mt(px(6.))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .text_size(rems(0.75))
            .text_color(t.muted_foreground)
            .child(pull_pill(p.state, p.draft, &t))
            .child(
                div()
                    .text_color(t.foreground.opacity(0.85))
                    .child(marks.text(("who", 0), who, cx)),
            )
            .child(marks.text(("verb", 0), verb, cx))
            .child(branch_chip(
                marks.text(("head", 0), p.head_ref.clone(), cx),
                &t,
            ))
            .child(marks.text(("into", 0), "into", cx))
            .child(branch_chip(
                marks.text(("base", 0), p.base_ref.clone(), cx),
                &t,
            ))
            .children(ci_badge("head-ci", head_ci.filter(|_| open), &t))
            .when_some(d.clone(), |m, d| {
                m.child(div().text_color(t.subtle).child("·"))
                    .child(marks.text(("commit-count", 0), commit_count(d.commits), cx))
                    .child(LineCounts::new(
                        Some(d.additions as u32),
                        Some(d.deletions as u32),
                    ))
            });
        let url = p.url.clone();
        let copy_url = p.url.clone();
        let actions = div()
            .mt(px(16.))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .when(open, |a| {
                a.child(
                    Button::new("checkout")
                        .small()
                        .icon(IconName::GitBranch)
                        .label("Checkout")
                        .disabled(busy || account.is_none())
                        .on_click(cx.listener(|this, _, _, cx| this.checkout(cx))),
                )
                .child({
                    let source = source.clone();
                    Button::new("worktree")
                        .small()
                        .icon(IconName::FolderGit2)
                        .label("Check out in new worktree…")
                        .disabled(busy || source.is_none())
                        .on_click(move |_, window, cx| {
                            if let Some(s) = source.clone() {
                                request(GithubRequest::NewWorktreeForPull(s), window, cx)
                            }
                        })
                })
                .child(
                    Button::new("review")
                        .small()
                        .icon(IconName::MessageSquare)
                        .label("Review")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| this.review(window, cx))),
                )
            })
            .when(open && rights.close, |a| {
                a.child(
                    Button::new("close")
                        .small()
                        .icon(IconName::GitPullRequestClosed)
                        .label("Close")
                        .disabled(busy)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.set_open(false, window, cx)),
                        ),
                )
            })
            .when(p.state == PullState::Closed && rights.reopen, |a| {
                a.child(
                    Button::new("reopen")
                        .small()
                        .icon(IconName::GitPullRequest)
                        .label("Reopen")
                        .disabled(busy)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.set_open(true, window, cx)),
                        ),
                )
            })
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
            });

        let mut page = div()
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(div().mt(px(6.)).child(pull_icon(p.state, p.draft, &t)))
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
                                    .font_weight(t.semibold())
                                    .child(marks.text(("title", 0), p.title.clone(), cx))
                                    .child(
                                        div()
                                            .text_color(t.subtle)
                                            .font_weight(FontWeight::NORMAL)
                                            .child(marks.text(
                                                ("number", 0),
                                                format!("#{}", p.number),
                                                cx,
                                            )),
                                    ),
                            )
                            .child(meta),
                    ),
            )
            .child(actions)
            // Cached data stays on screen; a failed refresh (a rate limit, say) is noted under it.
            .when_some(self.refresh_error(), |page, e| {
                page.child(
                    div()
                        .mt(px(8.))
                        .text_size(rems(0.75))
                        .debug_selector(|| "refresh-error".into())
                        .child(error_text("refresh-error", &e, t.removed, cx)),
                )
            });
        if let Some(d) = &d {
            if open {
                page = page.child(self.merge_box(d, account.as_deref(), cx));
            }
            page = page.children(self.checks_section(d, cx));
        }
        let total = d.as_ref().map_or(0, |d| d.commits);
        page =
            page.children(self.commits_section(total.max(self.commits().len() as u64), window, cx));
        page = page.child(self.files_section(d.as_deref(), cx));
        let desc = match (self.body_view(), &d) {
            (Some(md), _) => div().px(px(12.)).py(px(10.)).child(md).into_any_element(),
            (None, Some(_)) => div()
                .px(px(12.))
                .py(px(10.))
                .text_size(rems(0.75))
                .text_color(t.subtle)
                .italic()
                .child(marks.text(("no-description", 0), NO_DESCRIPTION, cx))
                .into_any_element(),
            (None, None) => div().h(px(8.)).into_any_element(),
        };
        page.child(section("Description", None, &t).child(desc))
            .into_any_element()
    }

    /// Item `ix` of the page's list: the head, then a comment each.
    fn page_item(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let comments = self.detail.as_ref().map_or(0, |d| d.comments.len());
        let item = match (ix, self.detail.clone()) {
            (0, _) => self.head(window, cx),
            (n, Some(d)) => self.comment(&d, n - 1, cx),
            (_, None) => div().into_any_element(),
        };
        // The page's column, centered in the list's width.
        div()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(900.))
                    .px(px(24.))
                    .when(ix == 0, |d| d.pt(px(20.)))
                    .when(ix == comments, |d| d.pb(px(20.)))
                    .child(item),
            )
            .into_any_element()
    }
}

impl Render for PullView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        // Shown again after a wake: read again if the last read is old.
        let (wakes, generation) = (Cache::wakes(cx), Cache::generation(cx));
        // Another repo or account (a restored tab reads before its project opened): read again now.
        let reset = generation != self.generation;
        if wakes != self.wakes || reset {
            self.wakes = wakes;
            self.generation = generation;
            let old = self.detail_at.is_none_or(|at| at.elapsed() > MIN_AGE);
            if (reset || old || self.detail.is_none()) && !self.loading {
                self.load(cx);
            }
        }
        self.sync_items();
        if self.text_stale {
            self.sync_text(window, cx);
        }
        self.find.sync(self, cx);
        if let Some(req) = self.pending_request.take() {
            window.defer(cx, move |window, cx| request(req, window, cx));
        }
        // The first read failed (offline, GitHub down): the list's copy alone isn't the page.
        if let (None, Some(e)) = (&self.detail, &self.detail_error) {
            let retry = cx.listener(|this, _, _, cx| this.load(cx));
            return div()
                .id("pull")
                .track_focus(&self.focus)
                .size_full()
                .child(load_failed("pull request", e, self.loading, retry, &t, cx))
                .into_any_element();
        }
        if self.item().is_none() {
            return div()
                .id("pull")
                .track_focus(&self.focus)
                .size_full()
                .p(px(32.))
                .text_center()
                .text_size(rems(0.78))
                .text_color(t.muted_foreground)
                .child("Loading the pull request…")
                .into_any_element();
        }
        let weak = cx.weak_entity();
        let page = list(self.page.clone(), move |ix, window, cx| {
            weak.update(cx, |this, cx| this.page_item(ix, window, cx))
                .unwrap_or_else(|_| div().into_any_element())
        })
        .size_full();
        let scroller = div()
            .id("pull")
            .key_context("PullView")
            .track_focus(&self.focus)
            .size_full()
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                let m = ev.keystroke.modifiers;
                if m.alt || m.platform || m.control || m.shift || this.pick.is_none() {
                    return;
                }
                // ←/→ step through the commits while Files changed shows one; Esc shows them all.
                match ev.keystroke.key.as_str() {
                    "left" => this.step(-1, cx),
                    "right" => this.step(1, cx),
                    "escape" => this.set_pick(None, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(page);
        div()
            .relative()
            .size_full()
            .on_action(cx.listener(|this, _: &editor::Find, window, cx| this.open_find(window, cx)))
            .child(scroller)
            .children(self.find.bar(cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gitviber_core::git::Commit;
    use gitviber_core::github::{Comment, CommitChecks};
    use gpui_kit::{TestAppContext, VisualTestContext};

    use super::*;
    use crate::leak_tests::{file, pull, setup};

    /// The workspace, which the page holds weakly, and the page.
    struct Host(Entity<Workspace>, Entity<PullView>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(900.)).h(px(600.)).child(self.1.clone())
        }
    }

    fn commit(i: usize, subject: &str, body: &str) -> Commit {
        Commit {
            sha: format!("{i:040}"),
            short_sha: format!("{i:07}"),
            author_name: "Ann".into(),
            author_email: "ann@example.com".into(),
            timestamp: 1_700_000_000,
            committer_name: "Ann".into(),
            committed_at: 1_700_000_000,
            parents: vec![],
            refs: vec![],
            subject: subject.into(),
            body: body.into(),
            unpushed: false,
            on_origin: true,
            not_in_head: false,
            file: None,
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    /// Fills the page as a read would: a failing check shown, two commits (one message folded),
    /// thirty files, a description and a review.
    fn fill(v: &mut PullView) {
        let mut p = crate::ui::core_pull(&pull(1));
        p.title = "zeta: the fix".into();
        v.detail = Some(Arc::new(PullDetail {
            pull: p,
            body: "The zeta body".into(),
            additions: 1,
            deletions: 1,
            changed_files: 30,
            commits: 2,
            mergeable: Some(true),
            mergeable_state: "clean".into(),
            ci: CommitChecks {
                checks: vec![gitviber_core::github::Check {
                    name: "zeta-lint".into(),
                    state: "failure".into(),
                    status: String::new(),
                    description: String::new(),
                    app: "GitHub Actions".into(),
                    started_at: None,
                    completed_at: None,
                    url: None,
                    id: Some(7),
                }],
                checks_error: None,
            },
            comments: vec![Comment {
                author: "zeta-bot".into(),
                body: "zeta again".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
                review: Some("APPROVED".into()),
            }],
            closed_by: None,
            merged_by: None,
        }));
        v.detail_error = None;
        v.detail_at = Some(Instant::now());
        v.text_stale = true;
        let files: Vec<FileChange> = (0..30)
            .map(|i| match i {
                25 => file("src/zeta.rs"),
                _ => file(&format!("src/f{i}.rs")),
            })
            .collect();
        v.file_list = Arc::new(files.clone());
        v.files = Some(Arc::new(PullFiles {
            base: "def".into(),
            head: "abc".into(),
            files,
            commits: vec![
                commit(0, "first", ""),
                commit(1, "zeta fix", "line one\nzeta two"),
            ],
        }));
        let log: Vec<String> = (0..60)
            .map(|i| format!("step {i}"))
            .chain(["zeta at the end".to_string()])
            .collect();
        v.failures.insert(
            7,
            Failure {
                data: Some(Arc::new(CheckFailure {
                    title: "zeta failed".into(),
                    summary: String::new(),
                    annotations: vec![Annotation {
                        path: "src/zeta.rs".into(),
                        line: 3,
                        level: "failure".into(),
                        title: String::new(),
                        message: "no zeta".into(),
                    }],
                    annotations_error: None,
                    log: Some(log.join("\n")),
                    log_error: None,
                })),
                error: None,
                log: ScrollHandle::new(),
                _task: Task::ready(()),
            },
        );
    }

    /// A long-running PR: 250 commits and `n` comments of a few paragraphs, a list and code.
    fn fill_long(v: &mut PullView, n: usize) {
        fill(v);
        let body = "A paragraph long enough to wrap once or twice at the page's width, about \
                    what changed and why it matters for the review.\n\n- one point\n- another \
                    point\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n";
        let d = Arc::get_mut(v.detail.as_mut().unwrap()).unwrap();
        d.comments = (0..n)
            .map(|i| Comment {
                author: format!("user{i}"),
                body: format!("Comment {i}. {body}"),
                created_at: "2026-01-01T00:00:00Z".into(),
                review: None,
            })
            .collect();
        d.commits = 250;
        Arc::get_mut(v.files.as_mut().unwrap()).unwrap().commits = (0..250)
            .map(|i| commit(i, &format!("commit {i}"), ""))
            .collect();
    }

    fn open_long(n: usize, cx: &mut TestAppContext) -> (Entity<PullView>, &mut VisualTestContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|window, cx| {
            let ws = cx.new(Workspace::new);
            Workspace::register_window(&ws, window, cx);
            let p = pull(1);
            let view =
                cx.new(|cx| PullView::new(ws.downgrade(), p.url.clone(), Some(p), window, cx));
            Host(ws, view)
        });
        cx.run_until_parked();
        let view = host.read_with(cx, |h, _| h.1.clone());
        view.update(cx, |v, cx| {
            fill_long(v, n);
            cx.notify();
        });
        draw(cx);
        draw(cx);
        (view, cx)
    }

    /// Frame times on a long page, scrolling top to bottom: `--run-ignored only`.
    #[gpui_kit::test]
    #[ignore = "a measurement, not a check"]
    fn long_page_frame_times(cx: &mut TestAppContext) {
        for n in [0, 300] {
            let (view, cx) = open_long(n, cx);
            let mut frames = vec![];
            for _ in 0..60 {
                view.update(cx, |v, cx| {
                    v.page.scroll_by(px(400.));
                    cx.notify();
                });
                let at = Instant::now();
                draw(cx);
                frames.push(at.elapsed());
            }
            frames.sort();
            eprintln!(
                "{n} comments, 250 commits: median {:?}, p90 {:?}, max {:?}",
                frames[frames.len() / 2],
                frames[frames.len() * 9 / 10],
                frames[frames.len() - 1]
            );
        }
    }

    /// The children the commits list drew: lines in view and the gaps between, not every row.
    fn commit_children(v: &PullView) -> usize {
        (0..)
            .take_while(|&i| v.commits_scroll.bounds_for_item(i).is_some())
            .count()
    }

    #[gpui_kit::test]
    fn a_long_page_builds_only_what_is_in_view(cx: &mut TestAppContext) {
        let (view, cx) = open_long(300, cx);
        view.read_with(cx, |v, _| {
            assert_eq!(v.page.item_count(), 301);
            // The last comments were never built; the commits list drew a screenful, not 250.
            assert!(v.page.bounds_for_item(300).is_none());
            assert!(commit_children(v) < 60, "{} children", commit_children(v));
        });
        // End in the commits list: the last commit is drawn and scrolled to, the gaps above it
        // as tall as the rows they stand for.
        let focus = view.read_with(cx, |v, _| v.commits_focus.clone());
        cx.update(|window, cx| focus.focus(window, cx));
        cx.simulate_keystrokes("end");
        draw(cx);
        draw(cx);
        view.read_with(cx, |v, _| {
            let (at, max) = (v.commits_scroll.offset().y, v.commits_scroll.max_offset().y);
            assert!(
                at < px(0.) && (at + max).abs() < px(1.),
                "{at:?} of {max:?}"
            );
            assert_eq!(v.commit_cursor, Some(format!("{:040}", 249)));
            assert!(commit_children(v) < 60);
        });
        // The end of the page: the last comment is drawn there.
        view.update(cx, |v, cx| {
            v.reveal_item(300);
            cx.notify();
        });
        draw(cx);
        view.read_with(cx, |v, _| {
            let view = v.page.viewport_bounds();
            assert!(
                v.page
                    .bounds_for_item(300)
                    .is_some_and(|b| b.intersects(&view))
            );
        });
    }

    #[gpui_kit::test]
    fn find_reaches_a_comment_far_down_a_long_thread(cx: &mut TestAppContext) {
        let (view, cx) = open_long(300, cx);
        view.update(cx, |v, cx| {
            let d = Arc::get_mut(v.detail.as_mut().unwrap()).unwrap();
            d.comments[250].body = "the needle is here".into();
            d.comments[280].author = "needle-bot".into();
            v.text_stale = true;
            cx.notify();
        });
        draw(cx);
        cx.update(|window, cx| view.update(cx, |v, cx| v.open_find(window, cx)));
        cx.simulate_input("needle");
        draw(cx);
        draw(cx);
        let shown = |item: usize, cx: &mut VisualTestContext| {
            view.read_with(cx, |v, _| {
                let view = v.page.viewport_bounds();
                v.page
                    .bounds_for_item(item)
                    .is_some_and(|b| b.intersects(&view))
            })
        };
        let finder = view.read_with(cx, |v, _| v.find.finder()).expect("open");
        assert_eq!(finder.read_with(cx, |f, _| f.state().0), Some(0));
        // The first: in comment 250's text, which its view scrolled to.
        assert!(shown(251, cx));
        assert_eq!(
            view.read_with(cx, |v, cx| v.blocks.get(251).unwrap().read(cx).matches()),
            (Some(0), 1)
        );
        // The next: comment 280's author, drawn as the list brought it in.
        cx.simulate_keystrokes("enter");
        draw(cx);
        draw(cx);
        assert!(shown(281, cx));
        assert_eq!(finder.read_with(cx, |f, _| f.state()), (Some(1), 2, None));
    }

    fn open_page(cx: &mut TestAppContext) -> (Entity<PullView>, &mut VisualTestContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|window, cx| {
            let ws = cx.new(Workspace::new);
            Workspace::register_window(&ws, window, cx);
            let p = pull(1);
            let view =
                cx.new(|cx| PullView::new(ws.downgrade(), p.url.clone(), Some(p), window, cx));
            Host(ws, view)
        });
        // The read finds no repository, as it would find no network.
        cx.run_until_parked();
        let view = host.read_with(cx, |h, _| h.1.clone());
        (view, cx)
    }

    #[gpui_kit::test]
    fn a_first_read_that_fails_says_why_and_offers_retry(cx: &mut TestAppContext) {
        let (view, cx) = open_page(cx);
        draw(cx);
        // The list's copy of the PR is in hand, but the page says what went wrong.
        view.read_with(cx, |v, _| {
            assert!(v.item.is_some() && v.detail.is_none() && v.detail_error.is_some());
        });
        let retry = cx.debug_bounds("load-retry").expect("Retry shown");
        let before = view.read_with(cx, |v, _| v.detail_at);
        cx.simulate_click(retry.center(), gpui_kit::Modifiers::none());
        // Read again (and failed again: there's still no repository).
        assert!(view.read_with(cx, |v, _| v.detail_at) > before);
        cx.run_until_parked();
        draw(cx);
        assert!(cx.debug_bounds("load-retry").is_some());
        // Once a read lands, the page; a later failure is only a note beside it.
        view.update(cx, |v, cx| {
            fill(v);
            v.detail_error = Some("GitHub 502: Bad Gateway".into());
            cx.notify();
        });
        draw(cx);
        assert!(cx.debug_bounds("load-retry").is_none());
        assert!(cx.debug_bounds("refresh-error").is_some());
    }

    #[gpui_kit::test]
    fn a_failed_refresh_is_shown_whole_under_the_actions(cx: &mut TestAppContext) {
        let (view, cx) = open_page(cx);
        let saml = "GitHub 403: Resource protected by organization SAML enforcement. You must \
                    grant your Personal Access token access to this organization. Authorize \
                    this token for the organization's single sign-on at \
                    https://github.com/orgs/acme/sso?authorization_request=AAAAAAAAAAAAAAAAAAAAAA";
        view.update(cx, |v, cx| {
            fill(v);
            v.detail_error = Some(saml.into());
            cx.notify();
        });
        draw(cx);
        draw(cx);
        // Wrapped onto lines of its own, not cut to the actions row's width.
        let note = cx.debug_bounds("refresh-error").expect("shown");
        assert!(note.size.height > px(30.), "{note:?}");
        assert!(note.size.width > px(800.), "{note:?}");
    }

    #[gpui_kit::test]
    fn every_match_on_the_page_is_drawn_and_scrolled_to(cx: &mut TestAppContext) {
        setup(cx);
        let (host, cx) = cx.add_window_view(|window, cx| {
            let ws = cx.new(Workspace::new);
            Workspace::register_window(&ws, window, cx);
            let p = pull(1);
            let view =
                cx.new(|cx| PullView::new(ws.downgrade(), p.url.clone(), Some(p), window, cx));
            Host(ws, view)
        });
        // The read finds no repository; the page then shows what a read would have brought.
        cx.run_until_parked();
        let (_ws, view) = host.read_with(cx, |h, _| (h.0.clone(), h.1.clone()));
        view.update(cx, |v, cx| {
            fill(v);
            cx.notify();
        });
        draw(cx);
        cx.update(|window, cx| view.update(cx, |v, cx| v.open_find(window, cx)));
        cx.simulate_input("zeta");
        draw(cx);
        let finder = view.read_with(cx, |v, _| v.find.finder()).expect("open");
        // The title; the check, its failure's title, place, message and log; a commit's subject
        // and folded message; a file out of the list's view; the description; the reviewer and
        // their review.
        assert_eq!(finder.read_with(cx, |f, _| f.state()), (Some(0), 12, None));
        for n in 1..=12 {
            cx.simulate_keystrokes("enter");
            draw(cx);
            draw(cx);
            let (current, _, waiting) = finder.read_with(cx, |f, _| f.state());
            assert_eq!(current, Some(n % 12));
            assert_eq!(waiting, None, "match {n} was never drawn");
        }
        // Stepping into the folded message opened it; the file's row came into the list's view.
        view.read_with(cx, |v, _| {
            assert!(v.open_bodies.contains(&format!("{:040}", 1)));
            assert!(v.files_scroll.0.borrow().base_handle.offset().y < px(0.));
        });
        // Scrolled down the page and back up to the title.
        let top = view.read_with(cx, |v, _| v.page.logical_scroll_top());
        assert_eq!((top.item_ix, top.offset_in_item), (0, px(0.)));
    }
}
