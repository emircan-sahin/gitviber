//! A guide tab: a change explained by the user's agent CLI, kept once written. The guide on the
//! left (overview, diagram, risks, a card a section in reading order, each with its category,
//! importance, check and a reviewed mark); its files' diffs on the right, one stacked diff in
//! the sections' order that reads only the files near the screen. Scrolling the diffs follows
//! the section they're of.

mod diagram;
mod parts;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gitviber_core::api;
use gitviber_core::git::FileChange;
use gitviber_diff::{
    Annotation, Compare, DiffSource, LinePos, NoticeLevel, Side, StackedDiff, StackedEvent,
    StackedFile,
};
use gitviber_refs::{RepoEvent, RepoState};
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{now_unix, relative_time};
use gitviber_ui::workspace::{
    GuideOf, ProjectEvent, PullRange, Selection, TabIcon, TabItem, Toast, Workspace,
    WorkspaceEvent, selection_key,
};
use gitviber_ui::{ActiveAppTheme as _, AppTheme, Settings};
use gpui_kit::assets::IconName;
use gpui_kit::base::text::{TextView, TextViewState};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, rems,
};

use crate::model::agent::{ReviewAgent, agent_name};
use crate::model::guide::{
    Category, Guide, Importance, NoteSide, Placed, Section, match_path, parse_guide, place_files,
    section_badge,
};
use crate::model::handoff::{About, EMPTY_TREE, HandoffInput, is_checked_out};
use crate::model::risks::{Risk, parse_risks};
use crate::store::{Guides, Run, RunKind, SavedGuide, SavedRun, guide_id};
use parts::{category_icon, category_tag, category_tone, count, icon, importance_tag, notice};

/// After the repo changes: a burst of saves asks once.
const STAMP_WAIT: Duration = Duration::from_millis(800);

/// The category shown, and whether only the high importance sections.
#[derive(Clone, Copy, Default, PartialEq)]
struct Filter {
    category: Option<Category>,
    high: bool,
}

/// What an Ask Agent box is open for.
#[derive(Clone, Copy, PartialEq)]
enum AskAt {
    Whole,
    /// A section, from 0.
    Section(usize),
}

struct Asking {
    at: AskAt,
    input: Entity<InputState>,
    _events: Subscription,
}

/// The guide's files, read for the range its run read.
enum Files {
    None,
    Loading {
        range: (String, String),
        _task: Task<()>,
    },
    Ready {
        range: (String, String),
        list: Arc<Vec<FileChange>>,
    },
    Failed {
        range: (String, String),
        error: String,
    },
}

impl Files {
    fn range(&self) -> Option<&(String, String)> {
        match self {
            Files::None => None,
            Files::Loading { range, .. }
            | Files::Ready { range, .. }
            | Files::Failed { range, .. } => Some(range),
        }
    }
}

pub struct GuideView {
    of: GuideOf,
    sel: Selection,
    workspace: WeakEntity<Workspace>,
    repo: Option<Entity<RepoState>>,
    root: PathBuf,
    id: Option<String>,
    saved: Option<SavedGuide>,
    guide: Option<Rc<Guide>>,
    found: Option<SavedRun>,
    /// The risks found; None when the answer wasn't the JSON asked for.
    risks: Option<Rc<Vec<Risk>>>,
    files: Files,
    placed: Option<Rc<Placed>>,
    stacked: Entity<StackedDiff>,
    /// Each file in the diff: its section (from 1; 0 for none) and its key there.
    in_diff: Vec<(String, usize, String)>,
    /// The section whose files are at the top of the diffs.
    current: Option<usize>,
    /// How many of the change's files are marked viewed.
    viewed: usize,
    filter: Filter,
    asking: Option<Asking>,
    /// Summaries shown whole.
    open: HashSet<usize>,
    /// The Markdown on show, parsed: cleared when the guide or the risks are written again.
    md: RefCell<HashMap<SharedString, Entity<TextViewState>>>,
    /// The uncommitted changes' stamp now, to tell a guide of them is outdated.
    stamp: Option<String>,
    scroll: ScrollHandle,
    _stamp: Option<Task<()>>,
    _tick: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
    _live: Live,
}

fn short(s: &str) -> &str {
    &s[..s.len().min(7)]
}

/// What a viewed mark remembers of a file: a new edit changes it, and the mark lapses.
fn file_sig(f: &FileChange) -> String {
    match &f.oid {
        Some(oid) => format!("{}:{oid}", f.status),
        None => format!(
            "{}:{}:{}",
            f.status,
            f.additions.map_or("null".into(), |n| n.to_string()),
            f.deletions.map_or("null".into(), |n| n.to_string())
        ),
    }
}

impl GuideView {
    pub fn new(of: GuideOf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let stacked = cx.new(|cx| StackedDiff::new(vec![], cx));
        let mut subscriptions = vec![
            cx.subscribe_in(&stacked, window, |this, _, e: &StackedEvent, window, cx| {
                this.stacked_event(e, window, cx)
            }),
            cx.observe(&stacked, |this, _, cx| this.follow_diffs(cx)),
            cx.observe_global::<Guides>(|this, cx| this.reload(cx)),
            cx.observe_global::<Settings>(|_, cx| cx.notify()),
        ];
        subscriptions.shrink_to_fit();
        let workspace = Workspace::for_window(window, cx)
            .map(|w| w.downgrade())
            .unwrap_or_else(WeakEntity::new_invalid);
        // The opener runs inside the workspace's update: it's read once that's over.
        cx.defer_in(window, |this, window, cx| this.attach(window, cx));
        GuideView {
            sel: Selection::Guide { of: of.clone() },
            of,
            workspace,
            repo: None,
            root: PathBuf::new(),
            id: None,
            saved: None,
            guide: None,
            found: None,
            risks: None,
            files: Files::None,
            placed: None,
            stacked,
            in_diff: vec![],
            current: None,
            viewed: 0,
            filter: Filter::default(),
            asking: None,
            open: HashSet::new(),
            md: RefCell::default(),
            stamp: None,
            scroll: ScrollHandle::new(),
            _stamp: None,
            _tick: None,
            _subscriptions: subscriptions,
            _live: Live::new("GuideView"),
        }
    }

    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self
            .workspace
            .upgrade()
            .and_then(|ws| ws.read(cx).project().cloned())
        else {
            return;
        };
        self.root = project.read(cx).root().to_path_buf();
        let repo = RepoState::get(&project, cx);
        self._subscriptions
            .push(cx.subscribe(&repo, |this, _, e: &RepoEvent, cx| {
                if matches!(e, RepoEvent::Changed) {
                    this.sync(cx);
                    cx.notify();
                }
            }));
        // Viewed marks set elsewhere (a file's own tab) show here too.
        if let Some(ws) = self.workspace.upgrade() {
            self._subscriptions
                .push(cx.subscribe(&ws, |this, _, e: &WorkspaceEvent, cx| {
                    if matches!(e, WorkspaceEvent::TabsChanged) {
                        this.fill_diff(cx);
                    }
                }));
        }
        let changes = matches!(self.of, GuideOf::Changes);
        self._subscriptions.push(
            cx.subscribe(&project, move |this, _, e: &ProjectEvent, cx| {
                if changes && matches!(e, ProjectEvent::RepoChanged(_)) {
                    this.read_stamp(STAMP_WAIT, cx);
                }
            }),
        );
        self.repo = Some(repo);
        if changes {
            self.read_stamp(Duration::ZERO, cx);
        }
        self.sync(cx);
    }

    /// The guide's id once what it needs is known (a branch's needs HEAD's branch), and what's
    /// kept under it.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let Some(repo) = &self.repo else { return };
        let repo = repo.read(cx);
        if matches!(self.of, GuideOf::Branch { .. }) && repo.status.is_none() {
            return;
        }
        let id = guide_id(&self.root.to_string_lossy(), &self.of, repo.branch());
        if self.id.as_deref() != Some(id.as_str()) {
            self.id = Some(id);
            self.saved = None;
            self.found = None;
            self.reload(cx);
            self.take_ask(cx);
        }
    }

    /// Writes the guide when its tab was opened to ask for one, unless there's one already or
    /// one being written.
    pub(crate) fn take_ask(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.id.clone() else { return };
        if crate::take_request(cx, &selection_key(&self.sel))
            && self.saved.is_none()
            && !Guides::is_running(cx, RunKind::Guide, &id)
        {
            self.generate(cx);
        }
    }

    /// What the store has now: a new answer is parsed, a new range's files read.
    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.id.clone() else { return };
        let saved = Guides::guide(cx, &id);
        if saved != self.saved {
            let text_changed = saved.as_ref().map(|s| (&s.run.text, s.run.at))
                != self.saved.as_ref().map(|s| (&s.run.text, s.run.at));
            if text_changed {
                self.guide = saved
                    .as_ref()
                    .and_then(|s| parse_guide(&s.run.text))
                    .map(Rc::new);
                self.placed = None;
                self.md.borrow_mut().clear();
                // Its sections may be others now: a box open on one is gone.
                self.asking = None;
            }
            self.saved = saved;
            self.load_files(cx);
            self.place(cx);
        }
        let found = Guides::risks(cx, &id);
        if found != self.found {
            self.risks = found
                .as_ref()
                .and_then(|f| parse_risks(&f.text))
                .map(Rc::new);
            self.md.borrow_mut().remove("risks-raw");
            self.found = found;
            self.annotate(cx);
        }
        let running = [RunKind::Guide, RunKind::Risks]
            .iter()
            .any(|k| Guides::is_running(cx, *k, &id));
        if running && self._tick.is_none() {
            // The run's time, once a second.
            self._tick = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }));
        } else if !running {
            self._tick = None;
        }
        cx.notify();
    }

    fn read_stamp(&mut self, wait: Duration, cx: &mut Context<Self>) {
        let core = gitviber_ui::core(cx);
        self._stamp = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let stamp = cx
                .background_spawn(async move { api::changes::changes_stamp(&core).ok() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.stamp = stamp;
                cx.notify();
            });
        }));
    }

    /// The range the guide read: the commit's, or base..head.
    fn compare(&self) -> Option<Compare> {
        let s = self.saved.as_ref()?;
        Some(match &self.of {
            GuideOf::Commit { commit } => Compare::Commit {
                sha: commit.sha.clone(),
            },
            _ => Compare::Range {
                base: s.run.base.clone(),
                head: s.run.head.clone(),
            },
        })
    }

    fn load_files(&mut self, cx: &mut Context<Self>) {
        let Some(s) = &self.saved else {
            self.files = Files::None;
            return;
        };
        let range = (s.run.base.clone(), s.run.head.clone());
        if self.files.range() == Some(&range) {
            return;
        }
        let core = gitviber_ui::core(cx);
        let commit = match &self.of {
            GuideOf::Commit { commit } => Some(commit.sha.clone()),
            _ => None,
        };
        let (base, head) = range.clone();
        let task = cx.spawn(async move |this, cx| {
            let files = cx
                .background_spawn(async move {
                    match commit {
                        Some(sha) => api::history::commit_files(&core, sha),
                        // A snapshot's tree reads back as a commit's would.
                        None => api::history::range_files(&core, base, head),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let range = this.files.range().cloned().unwrap_or_default();
                this.files = match files {
                    Ok(list) => Files::Ready {
                        range,
                        list: Arc::new(list),
                    },
                    Err(error) => Files::Failed { range, error },
                };
                this.placed = None;
                this.place(cx);
                cx.notify();
            });
        });
        self.files = Files::Loading { range, _task: task };
    }

    fn file_list(&self) -> Option<Arc<Vec<FileChange>>> {
        match &self.files {
            Files::Ready { list, .. } => Some(list.clone()),
            _ => None,
        }
    }

    /// Where each file shows, then the diffs in the sections' order.
    fn place(&mut self, cx: &mut Context<Self>) {
        if self.placed.is_none()
            && let (Some(guide), Some(list)) = (&self.guide, self.file_list())
        {
            let paths: Vec<String> = list.iter().map(|f| f.path.clone()).collect();
            self.placed = Some(Rc::new(place_files(guide, &paths)));
        }
        self.fill_diff(cx);
    }

    fn total(&self) -> usize {
        self.guide.as_ref().map_or(0, |g| g.sections.len())
    }

    fn highs(&self) -> usize {
        self.guide.as_ref().map_or(0, |g| {
            g.sections
                .iter()
                .filter(|s| s.importance == Importance::High)
                .count()
        })
    }

    fn categories(&self) -> Vec<(Category, usize)> {
        let mut out: Vec<(Category, usize)> = vec![];
        for s in self.guide.iter().flat_map(|g| g.sections.iter()) {
            match out.iter_mut().find(|(c, _)| *c == s.category) {
                Some((_, n)) => *n += 1,
                None => out.push((s.category, 1)),
            }
        }
        out
    }

    /// The filter as it applies now: a regenerated guide with none of the category shows all.
    fn active_filter(&self) -> Filter {
        let cats = self.categories();
        let (highs, total) = (self.highs(), self.total());
        Filter {
            category: self
                .filter
                .category
                .filter(|c| cats.len() > 1 && cats.iter().any(|(k, _)| k == c)),
            high: self.filter.high && highs > 0 && highs < total,
        }
    }

    fn fits(&self, s: &Section) -> bool {
        let f = self.active_filter();
        f.category.is_none_or(|c| s.category == c) && (!f.high || s.importance == Importance::High)
    }

    fn is_done(&self, i: usize) -> bool {
        self.saved.as_ref().is_some_and(|s| s.done.contains(&i))
    }

    fn file_selection(&self, f: &FileChange) -> Option<Selection> {
        let s = self.saved.as_ref()?;
        let file = Arc::new(f.clone());
        Some(match &self.of {
            GuideOf::Commit { commit } => Selection::Commit {
                commit: commit.clone(),
                file,
                url: None,
            },
            GuideOf::Pull { pull, .. } => Selection::PrFile {
                range: PullRange {
                    number: Some(pull.number),
                    pull_url: Some(pull.url.clone()),
                    label: None,
                    base: s.run.base.clone(),
                    head: s.run.head.clone(),
                },
                file,
            },
            GuideOf::Changes => Selection::PrFile {
                range: PullRange {
                    number: None,
                    pull_url: None,
                    label: Some("Uncommitted".into()),
                    base: s.run.base.clone(),
                    head: s.run.head.clone(),
                },
                file,
            },
            GuideOf::Branch { label, .. } => Selection::PrFile {
                range: PullRange {
                    number: None,
                    pull_url: None,
                    label: Some(format!("{label}...{}", short(&s.run.head))),
                    base: s.run.base.clone(),
                    head: s.run.head.clone(),
                },
                file,
            },
        })
    }

    /// Viewed, under the key its own tab marks it by; a commit's or a range's file never changes.
    fn is_viewed(&self, f: &FileChange, cx: &App) -> bool {
        let (Some(ws), Some(sel)) = (self.workspace.upgrade(), self.file_selection(f)) else {
            return false;
        };
        ws.read(cx).viewed(&selection_key(&sel)) == Some(file_sig(f).as_str())
    }

    /// The stacked diff's files: each shown section's in reading order, then those no section
    /// has. A reviewed section's go, as its card folds.
    fn fill_diff(&mut self, cx: &mut Context<Self>) {
        let (Some(placed), Some(list), Some(compare)) =
            (self.placed.clone(), self.file_list(), self.compare())
        else {
            self.in_diff.clear();
            self.stacked.update(cx, |s, cx| s.set_files(vec![], cx));
            return;
        };
        self.viewed = list.iter().filter(|f| self.is_viewed(f, cx)).count();
        let guide = self.guide.clone();
        let filtered = self.active_filter() != Filter::default();
        let mut order: Vec<(String, usize)> = vec![];
        for (i, shown) in placed.shown.iter().enumerate() {
            let fits = guide.as_ref().is_some_and(|g| self.fits(&g.sections[i]));
            if fits && !self.is_done(i) {
                order.extend(shown.iter().map(|p| (p.clone(), i + 1)));
            }
        }
        if !filtered {
            order.extend(placed.rest.iter().map(|p| (p.clone(), 0)));
        }
        let mut files = vec![];
        self.in_diff.clear();
        for (path, n) in order {
            let Some(f) = list.iter().find(|f| f.path == path) else {
                continue;
            };
            let source = DiffSource::from_change(compare.clone(), f);
            self.in_diff.push((path, n, source.key()));
            files.push(StackedFile {
                source,
                additions: f.additions,
                deletions: f.deletions,
                sig: file_sig(f),
                viewed: self.is_viewed(f, cx),
            });
        }
        self.stacked.update(cx, |s, cx| s.set_files(files, cx));
        self.annotate(cx);
        self.follow_diffs(cx);
    }

    /// The agent's notes and the risks found, under the lines they're about.
    fn annotate(&mut self, cx: &mut Context<Self>) {
        let t = cx.app_theme().clone();
        let paths: HashSet<&str> = self.in_diff.iter().map(|(p, _, _)| p.as_str()).collect();
        let mut by_file: Vec<(String, Vec<Annotation>)> = self
            .in_diff
            .iter()
            .map(|(_, _, key)| (key.clone(), vec![]))
            .collect();
        let mut add =
            |path: &str, side: NoteSide, line: u64, text: String, critical: bool, id: String| {
                let Some(path) = match_path(path, &paths) else {
                    return;
                };
                let Some(i) = self.in_diff.iter().position(|(p, _, _)| p == path) else {
                    return;
                };
                let Ok(line) = u32::try_from(line) else {
                    return;
                };
                let side = if side == NoteSide::Old {
                    Side::Old
                } else {
                    Side::New
                };
                let t = t.clone();
                by_file[i].1.push(Annotation::new(
                    id,
                    LinePos { side, line },
                    px(34.),
                    move |_, _| note_box(&text, critical, &t),
                ));
            };
        for (si, s) in self
            .guide
            .iter()
            .flat_map(|g| g.sections.iter())
            .enumerate()
        {
            for (ni, n) in s.line_notes.iter().enumerate() {
                add(
                    &n.path,
                    n.side,
                    n.line,
                    n.text.clone(),
                    n.critical,
                    format!("g{si}-{ni}"),
                );
            }
        }
        // Risks found on the files shown: read at another head, their lines would be off.
        let same_head =
            self.found.as_ref().map(|f| &f.head) == self.saved.as_ref().map(|s| &s.run.head);
        if same_head && let Some(risks) = &self.risks {
            for (ri, r) in risks.iter().enumerate() {
                if let Some(line) = r.line {
                    let why = if r.why.is_empty() {
                        String::new()
                    } else {
                        format!(". {}", r.why)
                    };
                    add(
                        &r.path,
                        r.side,
                        line,
                        format!("Risk: {}{why}", r.title),
                        r.severity == crate::model::risks::Severity::High,
                        format!("r{ri}"),
                    );
                }
            }
        }
        self.stacked.update(cx, |s, cx| {
            for (key, notes) in by_file {
                s.set_annotations(&key, notes, cx);
            }
        });
    }

    /// The section of the file at the top of the diffs, named over them.
    fn follow_diffs(&mut self, cx: &mut Context<Self>) {
        let top = self.stacked.read(cx).top_file();
        let n = top.and_then(|f| self.in_diff.get(f)).map(|(_, n, _)| *n);
        if n == self.current {
            return;
        }
        self.current = n;
        let title = match (n, &self.guide) {
            (Some(0), _) => Some("Other changes".to_string()),
            (Some(n), Some(g)) => g.sections.get(n - 1).map(|s| {
                let title = if s.title.is_empty() {
                    "Untitled"
                } else {
                    &s.title
                };
                format!(
                    "{} / {} · {title}",
                    section_badge(n),
                    section_badge(g.sections.len())
                )
            }),
            _ => None,
        };
        self.stacked
            .update(cx, |s, cx| s.set_title(title.map(SharedString::from), cx));
        cx.notify();
    }

    /// Section `n` (from 1; 0 for the files no section has): its card in view, its first diff at
    /// the top. One the filter hides shows them all first.
    fn go_to(&mut self, n: usize, cx: &mut Context<Self>) {
        let hidden = n > 0
            && self
                .guide
                .as_ref()
                .and_then(|g| g.sections.get(n - 1))
                .is_some_and(|s| !self.fits(s));
        if (hidden || n == 0) && self.active_filter() != Filter::default() {
            self.filter = Filter::default();
            self.fill_diff(cx);
        }
        if let Some((_, _, key)) = self.in_diff.iter().find(|(_, s, _)| *s == n) {
            let key = key.clone();
            self.stacked
                .update(cx, |s, cx| s.restore_scroll(&key, px(0.), cx));
        }
        if let Some(ix) = self.card_index(n) {
            self.scroll.scroll_to_top_of_item(ix);
        }
        cx.notify();
    }

    /// Where section `n`'s card is among the guide column's children.
    fn card_index(&self, n: usize) -> Option<usize> {
        let g = self.guide.as_ref()?;
        let shown: Vec<usize> = (0..g.sections.len())
            .filter(|i| self.fits(&g.sections[*i]))
            .collect();
        let lead = self.lead_count();
        if n == 0 {
            return Some(lead + shown.len());
        }
        shown.iter().position(|i| *i == n - 1).map(|p| lead + p)
    }

    /// The children of the guide column ahead of the cards: overview, diagram, risks, filter, notice.
    fn lead_count(&self) -> usize {
        let Some(g) = &self.guide else { return 0 };
        let mut n = 0;
        if !g.summary.is_empty() {
            n += 1;
        }
        if g.has_diagram() {
            n += 1;
        }
        if self.found.is_some() {
            n += 1;
        }
        if self.filter_shown() {
            n += 1;
        }
        if self.rest_notice() {
            n += 1;
        }
        n
    }

    fn filter_shown(&self) -> bool {
        let (highs, total) = (self.highs(), self.total());
        self.categories().len() > 1 || (highs > 0 && highs < total)
    }

    fn rest_notice(&self) -> bool {
        self.placed.as_ref().is_some_and(|p| !p.rest.is_empty())
            && self.active_filter() == Filter::default()
    }

    fn stacked_event(&mut self, e: &StackedEvent, window: &mut Window, cx: &mut Context<Self>) {
        match e {
            StackedEvent::ToggleViewed(source) => {
                let Some(list) = self.file_list() else { return };
                let Some(f) = list.iter().find(|f| f.path == source.path) else {
                    return;
                };
                let (Some(ws), Some(sel)) = (self.workspace.upgrade(), self.file_selection(f))
                else {
                    return;
                };
                let on = !self.is_viewed(f, cx);
                let sig = on.then(|| file_sig(f));
                ws.update(cx, |ws, cx| ws.set_viewed(selection_key(&sel), sig, cx));
                self.fill_diff(cx);
            }
            StackedEvent::OpenFile(source) => {
                let Some(list) = self.file_list() else { return };
                let Some(sel) = list
                    .iter()
                    .find(|f| f.path == source.path)
                    .and_then(|f| self.file_selection(f))
                else {
                    return;
                };
                if let Some(ws) = self.workspace.upgrade() {
                    ws.update(cx, |ws, cx| ws.open(sel, false, window, cx));
                }
            }
            StackedEvent::Notice(n) => {
                let toast = match n.level {
                    NoticeLevel::Error => Toast::error(n.detail.clone().unwrap_or_default()),
                    NoticeLevel::Success => Toast::success(n.detail.clone().unwrap_or_default()),
                    NoticeLevel::Info => Toast::info(n.detail.clone().unwrap_or_default()),
                }
                .title(n.title.clone());
                self.toast(toast, cx);
            }
            // A guide's diffs take no review notes of the user's.
            StackedEvent::AddNote { .. } => {}
        }
    }

    fn toast(&self, toast: Toast, cx: &mut Context<Self>) {
        if let Some(ws) = self.workspace.upgrade() {
            ws.update(cx, |ws, cx| ws.toast(toast, cx));
        }
    }

    fn tab(&self) -> Option<(Entity<Workspace>, Selection)> {
        Some((self.workspace.upgrade()?, self.sel.clone()))
    }

    fn start(&mut self, kind: RunKind, cx: &mut Context<Self>) {
        let Some(id) = self.id.clone() else { return };
        if let Some(stopped) = Guides::start(cx, kind, id, self.of.clone(), self.tab()) {
            self.toast(
                Toast::info(format!(
                    "{stopped} was still being written; one is written at a time."
                ))
                .title(format!("Stopped a {}", kind.noun())),
                cx,
            );
        }
        self.reload(cx);
    }

    pub fn generate(&mut self, cx: &mut Context<Self>) {
        self.start(RunKind::Guide, cx);
    }

    pub fn find_risks(&mut self, cx: &mut Context<Self>) {
        self.start(RunKind::Risks, cx);
    }

    fn cancel(&mut self, kind: RunKind, cx: &mut Context<Self>) {
        if let Some(id) = &self.id {
            Guides::cancel(cx, kind, id);
        }
    }

    fn mark_done(&mut self, i: usize, done: bool, cx: &mut Context<Self>) {
        if let Some(id) = &self.id {
            Guides::mark_done(cx, id, i, done);
        }
    }

    fn set_filter(&mut self, f: Filter, cx: &mut Context<Self>) {
        self.filter = f;
        self.fill_diff(cx);
        cx.notify();
    }

    /// Whether what a run read (its head, or the changes' stamp) is behind.
    fn moved_from(&self, head: &str, stamp: Option<&str>, cx: &App) -> bool {
        match &self.of {
            GuideOf::Changes => {
                self.stamp.is_some() && stamp.is_some() && stamp != self.stamp.as_deref()
            }
            GuideOf::Branch { .. } => self
                .repo
                .as_ref()
                .and_then(|r| r.read(cx).head().map(str::to_string))
                .is_some_and(|h| !head.starts_with(&h)),
            GuideOf::Pull { pull, .. } => head != pull.head_sha,
            GuideOf::Commit { .. } => false,
        }
    }

    fn handoff(&self, about: About, range: &SavedRun, cx: &App) -> HandoffInput {
        let repo = self.repo.as_ref().map(|r| r.read(cx));
        HandoffInput {
            of: self.of.clone(),
            branch: repo.and_then(|r| r.branch().map(str::to_string)),
            base: range.base.clone(),
            head: range.head.clone(),
            guide: self.guide.as_deref().cloned(),
            about,
            checked_out: is_checked_out(&self.of, repo.and_then(|r| r.head()), &range.head),
            moved: self.moved_from(&range.head, range.stamp.as_deref(), cx),
            language: Settings::get(cx).review_language.clone(),
        }
    }

    fn worktrees(&self, cx: &App) -> Rc<Vec<gitviber_core::git::Worktree>> {
        self.repo
            .as_ref()
            .map(|r| r.read(cx).worktrees.clone())
            .unwrap_or_default()
    }

    /// Section `i` as asked about; None once a guide written again has no such section.
    fn about_section(&self, i: usize) -> Option<About> {
        Some(About::Section {
            section: self.guide.as_ref()?.sections.get(i)?.clone(),
            n: i + 1,
            total: self.total(),
        })
    }

    fn ask(&mut self, at: AskAt, question: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(saved) = self.saved.clone() else {
            return;
        };
        let about = match at {
            AskAt::Whole => About::Whole,
            AskAt::Section(i) => match self.about_section(i) {
                Some(about) => about,
                None => return,
            },
        };
        let input = self.handoff(about, &saved.run, cx);
        let worktrees = self.worktrees(cx);
        self.asking = None;
        crate::agent::ask_agent(&self.root, &worktrees, input, &question, window, cx);
        cx.notify();
    }

    fn open_ask(&mut self, at: AskAt, window: &mut Window, cx: &mut Context<Self>) {
        if self.asking.as_ref().is_some_and(|a| a.at == at) {
            self.asking = None;
            cx.notify();
            return;
        }
        let agent = agent_name(&ReviewAgent::from_settings(Settings::get(cx)).command);
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(format!(
                "Ask {agent}… (Enter sends, empty leaves it to you)"
            ))
        });
        input.update(cx, |i, cx| i.focus(window, cx));
        let events = cx.subscribe_in(
            &input,
            window,
            move |this, input, e: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = e {
                    let q = input.read(cx).value().to_string();
                    this.ask(at, q, window, cx);
                }
            },
        );
        self.asking = Some(Asking {
            at,
            input,
            _events: events,
        });
        cx.notify();
    }

    fn risk_ask(&mut self, i: usize, how: How, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(found), Some(risks)) = (self.found.clone(), self.risks.clone()) else {
            return;
        };
        let Some(r) = risks.get(i) else { return };
        let input = self.handoff(About::Risk(r.clone()), &found, cx);
        self.hand(how, input, window, cx);
    }

    fn hand(&mut self, how: How, input: HandoffInput, window: &mut Window, cx: &mut Context<Self>) {
        let worktrees = self.worktrees(cx);
        match how {
            How::Ask => crate::agent::ask_agent(&self.root, &worktrees, input, "", window, cx),
            How::Paste => {
                crate::agent::paste_to_running_agent(&self.root, &worktrees, input, window, cx)
            }
            How::Copy => crate::agent::copy_as_prompt(&input, window, cx),
        }
    }

    fn other_way(&mut self, at: AskAt, how: How, window: &mut Window, cx: &mut Context<Self>) {
        let Some(saved) = self.saved.clone() else {
            return;
        };
        let about = match at {
            AskAt::Whole => About::Whole,
            AskAt::Section(i) => match self.about_section(i) {
                Some(about) => about,
                None => return,
            },
        };
        let input = self.handoff(about, &saved.run, cx);
        self.hand(how, input, window, cx);
    }

    // For tests and the demo.
    #[doc(hidden)]
    pub fn guide(&self) -> Option<&Guide> {
        self.guide.as_deref()
    }

    #[doc(hidden)]
    pub fn diff_files(&self) -> Vec<String> {
        self.in_diff.iter().map(|(p, _, _)| p.clone()).collect()
    }

    #[doc(hidden)]
    pub fn stacked(&self) -> &Entity<StackedDiff> {
        &self.stacked
    }

    #[doc(hidden)]
    pub fn show_section(&mut self, n: usize, cx: &mut Context<Self>) {
        self.go_to(n, cx);
    }

    #[doc(hidden)]
    pub fn mark_section(&mut self, i: usize, done: bool, cx: &mut Context<Self>) {
        self.mark_done(i, done, cx);
    }
}

/// The other ways to the agent, from a button's menu.
#[derive(Clone, Copy)]
enum How {
    /// A new session, no question asked first: a risk's Send to Agent.
    Ask,
    Paste,
    Copy,
}

fn note_box(text: &str, critical: bool, t: &AppTheme) -> AnyElement {
    div()
        .px(px(12.))
        .py(px(3.))
        .child(
            div()
                .max_w(px(760.))
                .flex()
                .items_start()
                .gap(px(8.))
                .px(px(10.))
                .py(px(6.))
                .rounded(px(6.))
                .border_1()
                .border_color(if critical {
                    t.destructive.opacity(0.6)
                } else {
                    t.border
                })
                .bg(t.panel)
                .text_color(t.foreground)
                .font_family(t.fonts.ui.clone())
                .text_size(rems(0.75))
                .child(
                    div()
                        .pt(px(1.))
                        .child(icon(IconName::MessageSquareText, t.subtle)),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .when(critical, |d| {
                            d.child(
                                div()
                                    .text_color(t.destructive)
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .child("Review carefully"),
                            )
                        })
                        .child(text.to_string()),
                ),
        )
        .into_any_element()
}

impl TabItem for GuideView {
    fn tab_title(&self, _: &App) -> SharedString {
        gitviber_ui::workspace::selection_path(&self.sel).into()
    }

    fn tab_icon(&self, _: &App) -> TabIcon {
        TabIcon::Icon(IconName::Sparkles)
    }

    fn tab_tooltip(&self, _: &App) -> Option<SharedString> {
        self.guide
            .as_ref()
            .map(|g| g.title.clone().into())
            .filter(|t: &SharedString| !t.is_empty())
    }

    fn tab_focus(&self, cx: &App) -> Option<FocusHandle> {
        Some(self.stacked.read(cx).focus_handle(cx))
    }
}

mod render;
