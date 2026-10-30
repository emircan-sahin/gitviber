//! The left sidebar's PRs: origin's pull requests (and for a fork, its original's beside them),
//! narrowed by the chips, with CI on each open one. Pages of 100 load as asked; the list draws
//! only the rows on screen.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gitviber_core::api;
use gitviber_ui::actions::editor;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::CountBadge;
use gitviber_ui::workspace::{Panel, PanelContext, ProjectEvent, Slot, WorkspaceEvent};
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    rems, uniform_list,
};

use crate::cache::{Cache, MIN_AGE, Wakes};
use crate::ci::{self, CiWatch};
use crate::logic::actions::pull_source;
use crate::logic::narrow::{Filter, ListKind, Narrow, compact_count, empty_text};
use crate::model::{Account, Counts, PullItem, PullState, Target, is_not_connected};
use crate::requests::{GithubRequest, request};
use crate::ui::common::{ago, ci_badge, empty_note, list_error, pull_icon, signed_in_as, theme};
use crate::ui::connect::{ConnectView, gate_view};
use crate::ui::labels::LabelPicker;
use crate::ui::list::{
    ListControls, ListPanel, filter_tabs, find_box, narrow_bar, selected_labels,
};
use crate::ui::rows::{ROW, RowKey, Rows, focus_page, row_key};

/// github/client.rs reads at most this many pages; a search stops at GitHub's 1000 results.
const MAX_PAGES: usize = 30;
const MAX_SEARCH_PAGES: usize = 10;
const PAGE: usize = 100;

/// What the branch status says that the panel needs.
#[derive(Clone, Default)]
pub(crate) struct BranchStatus {
    pub branch: Option<String>,
    /// A checked-out PR (#7) whose branch is someone else's.
    pub follows: Option<String>,
    /// Where `git push` sends the branch, when that isn't origin.
    pub push_elsewhere: Option<String>,
    /// Pushed already and nothing new since: GitHub has the branch as it is.
    pub pushed: bool,
}

impl BranchStatus {
    /// From core's status: `git push`'s target says where the branch goes and whether GitHub
    /// has it as it is.
    pub(crate) fn read(
        branch: Option<String>,
        follows: Option<String>,
        push: Option<&gitviber_core::git::PushTarget>,
    ) -> Self {
        BranchStatus {
            branch,
            follows,
            push_elsewhere: push
                .filter(|p| p.remote != "origin")
                .map(|p| p.remote.clone()),
            pushed: push.is_some_and(|p| p.branch.is_some() && p.ahead == 0),
        }
    }

    /// A PR needs the branch on GitHub as it is: unpublished, or with commits since.
    pub(crate) fn needs_push(&self) -> bool {
        !self.pushed
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Origin,
    Parent,
}

pub struct PullsPanel {
    pub(crate) ctx: PanelContext,
    _root: PathBuf,
    controls: ListControls,
    pages: [usize; 2],
    status: BranchStatus,
    /// What the lists were last asked for, so a render asks again only when it changed.
    asked: Option<(String, Wakes, Option<String>)>,
    stamp: u64,
    ci_version: u64,
    ci_shas: Vec<String>,
    connect: Option<Entity<ConnectView>>,
    focus: FocusHandle,
    /// Each pane's rows and their keyboard: origin's, and a fork's original's.
    rows: [Rows; 2],
    _ci: Vec<CiWatch>,
    _status: Task<()>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl ListPanel for PullsPanel {
    fn controls(&mut self) -> &mut ListControls {
        &mut self.controls
    }

    fn narrowed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.pages = [1, 1];
        self.controls.save_picked(cx);
        self.ask(false, cx);
    }

    fn focus_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rows[0].focus.focus(window, cx);
    }
}

impl Panel for PullsPanel {
    fn badge(&self, _: &App) -> Option<SharedString> {
        None
    }

    fn panel_focus(&self, _: &App) -> Option<FocusHandle> {
        Some(self.rows[0].focus.clone())
    }
}

/// The repository's main worktree, which every worktree of it shares its chips with.
pub(crate) fn main_of(root: &std::path::Path) -> String {
    gitviber_core::git::main_worktree(root).unwrap_or_else(|| root.to_string_lossy().into_owned())
}

/// The lists the branch and worktree badges read (`bridges::pull_of`), whether or not the PRs
/// panel ever shows: origin's open PRs, and its 100 most recently updated of any state, so a
/// squash-merged branch shows merged and Clean up offers its worktree; a fork's original's
/// too. Under the panel's own keys, so each reuses the other's; an answer redraws the windows,
/// as the pickers don't follow the cache.
pub(crate) fn load_branch_lists(cx: &mut App) {
    let upstream = crate::account::account(cx).and_then(|a| a.upstream());
    let lists = [(None, Filter::Open), (None, Filter::All)]
        .into_iter()
        .chain(upstream.map(|u| (Some(u), Filter::All)));
    for (target, filter) in lists {
        let name = target.as_deref().unwrap_or("origin");
        let key = PullsPanel::list_key(name, filter, &Narrow::default(), 1);
        Cache::load_held(cx, &key, MIN_AGE, move |core| {
            api::github::pr_list(
                core,
                target.clone(),
                filter.as_str().into(),
                1,
                Narrow::default().to_core(),
            )
            .map(|ps| ps.iter().map(PullItem::from).collect::<Vec<_>>())
        });
    }
}

/// Whether `load_branch_lists` would ask anything now: some list is older than `MIN_AGE`.
pub(crate) fn branch_lists_stale(cx: &App) -> bool {
    let upstream = crate::account::account(cx).and_then(|a| a.upstream());
    [
        ("origin".to_string(), Filter::Open),
        ("origin".into(), Filter::All),
    ]
    .into_iter()
    .chain(upstream.map(|u| (u, Filter::All)))
    .any(|(name, filter)| {
        let key = PullsPanel::list_key(&name, filter, &Narrow::default(), 1);
        Cache::is_stale(cx, &key, MIN_AGE)
    })
}

/// As a project's GitHub views first show: its account, and the cache for its repository.
pub(crate) fn use_project(root: &std::path::Path, cx: &mut App) {
    let fresh = Cache::repo_is(cx, root);
    Cache::use_repo(cx, root);
    if !fresh {
        // Its failure is told as the project opens (`follow_projects`).
        let _ = crate::account::apply_project_account(&main_of(root), cx);
    }
}

impl PullsPanel {
    pub fn new(ctx: &PanelContext, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let root = ctx
            .project
            .as_ref()
            .map(|p| p.read(cx).root().to_path_buf())
            .unwrap_or_default();
        use_project(&root, cx);
        let controls = ListControls::new(ListKind::Pulls, main_of(&root), window, cx);
        let mut subs = vec![
            cx.observe_global::<Cache>(|this: &mut Self, cx| {
                let stamp = this.stamp_now(cx);
                if stamp != this.stamp || this.asked.as_ref().map(|a| a.1) != Some(Cache::wakes(cx))
                {
                    this.stamp = stamp;
                    cx.notify();
                }
            }),
            cx.observe_global::<ci::Ci>(|this: &mut Self, cx| {
                let v = ci::version(cx);
                if v != this.ci_version {
                    this.ci_version = v;
                    cx.notify();
                }
            }),
            cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    Cache::wake(cx);
                }
            }),
        ];
        if let Some(ws) = ctx.workspace.upgrade() {
            // The open tab's row is drawn highlighted.
            subs.push(cx.subscribe(&ws, |_, _, e: &WorkspaceEvent, cx| {
                if matches!(e, WorkspaceEvent::TabsChanged) {
                    cx.notify();
                }
            }));
        }
        if let Some(project) = &ctx.project {
            subs.push(
                cx.subscribe(project, |this: &mut Self, _, ev: &ProjectEvent, cx| {
                    if matches!(ev, ProjectEvent::RepoChanged(_)) {
                        this.read_status(cx);
                    }
                }),
            );
        }
        let mut panel = PullsPanel {
            ctx: ctx.clone(),
            _root: root,
            controls,
            pages: [1, 1],
            status: BranchStatus::default(),
            asked: None,
            stamp: 0,
            ci_version: 0,
            ci_shas: vec![],
            connect: None,
            focus: cx.focus_handle(),
            rows: [Rows::new(cx), Rows::new(cx)],
            _ci: vec![],
            _status: Task::ready(()),
            _subs: subs,
            _live: Live::new("PullsPanel"),
        };
        panel.read_status(cx);
        panel
    }

    fn read_status(&mut self, cx: &mut Context<Self>) {
        let core = crate::core(cx);
        self._status = cx.spawn(async move |this, cx| {
            let Ok(s) = cx
                .background_spawn(async move { api::changes::status(&core) })
                .await
            else {
                return;
            };
            this.update(cx, |this, cx| {
                if let Some(origin) = s.origin.clone() {
                    Cache::set_origin(cx, origin);
                }
                this.status =
                    BranchStatus::read(s.branch.clone(), s.follows.clone(), s.push.as_ref());
                cx.notify();
            })
            .ok();
        });
    }

    /// The left panel shows PRs: what their CI is kept current for.
    fn shown(&self, cx: &App) -> bool {
        self.ctx.workspace.upgrade().is_some_and(|ws| {
            let ws = ws.read(cx);
            ws.is_open(Slot::Left) && ws.active_panel(Slot::Left, cx) == Some("pulls")
        })
    }

    fn account(&self, cx: &App) -> Option<Arc<Account>> {
        crate::account::account(cx)
    }

    fn upstream(&self, cx: &App) -> Option<String> {
        self.account(cx).and_then(|a| a.upstream())
    }

    fn me_usable(&self, cx: &App) -> bool {
        let read = crate::account::current_account(cx);
        read.error.is_none() || read.data.is_some()
    }

    fn narrow(&self, cx: &App) -> Narrow {
        self.controls.narrow(self.me_usable(cx))
    }

    pub(crate) fn list_key(name: &str, filter: Filter, narrow: &Narrow, pages: usize) -> String {
        let tag = narrow.key();
        // Unnarrowed lists keep their key: the worktrees' PR badges read it too.
        let tag = if tag.is_empty() {
            tag
        } else {
            format!("{tag}:")
        };
        format!("pulls:{name}:{}:{tag}{pages}", filter.as_str())
    }

    fn counts_key(name: &str, narrow: &Narrow) -> String {
        format!("pulls:counts:{name}:{}", narrow.key())
    }

    fn keys(&self, cx: &App) -> Vec<String> {
        let narrow = self.narrow(cx);
        let mut keys = vec![crate::account::KEY.to_string()];
        let filter = self.controls.filter;
        keys.push(Self::list_key("origin", filter, &narrow, self.pages[0]));
        keys.push(Self::list_key(
            "origin",
            filter,
            &narrow,
            self.pages[0].saturating_sub(1),
        ));
        keys.push(Self::counts_key("origin", &narrow));
        if let Some(up) = self.upstream(cx) {
            keys.push(Self::list_key(&up, filter, &narrow, self.pages[1]));
            keys.push(Self::list_key(
                &up,
                filter,
                &narrow,
                self.pages[1].saturating_sub(1),
            ));
            keys.push(Self::counts_key(&up, &narrow));
        }
        keys
    }

    fn stamp_now(&self, cx: &App) -> u64 {
        let keys = self.keys(cx);
        Cache::stamp(cx, &keys.iter().map(String::as_str).collect::<Vec<_>>())
    }

    /// Asks for what the panel shows; `force` ignores the answers' age (⟳, retry, a write).
    fn ask(&mut self, force: bool, cx: &mut Context<Self>) {
        let age = if force { Duration::ZERO } else { MIN_AGE };
        crate::account::load(cx, force);
        let narrow = self.narrow(cx);
        let filter = self.controls.filter;
        let upstream = self.upstream(cx);
        let lists: Vec<(Option<String>, String, usize)> =
            std::iter::once((None, "origin".to_string(), self.pages[0]))
                .chain(
                    upstream
                        .clone()
                        .map(|u| (Some(u.clone()), u, self.pages[1])),
                )
                .collect();
        for (target, name, pages) in lists {
            let (t, n, f) = (target.clone(), narrow.clone(), filter);
            Cache::load(
                cx,
                &Self::list_key(&name, filter, &narrow, pages),
                age,
                move |core| {
                    api::github::pr_list(core, t.clone(), f.as_str().into(), pages, n.to_core())
                        .map(|ps| ps.iter().map(PullItem::from).collect::<Vec<_>>())
                },
            );
            let (t, n) = (target, narrow.clone());
            Cache::load(cx, &Self::counts_key(&name, &narrow), age, move |core| {
                api::github::pr_counts(core, t.clone(), n.to_core()).map(|c| Counts::from(&c))
            });
        }
        self.asked = Some((self.signature(cx), Cache::wakes(cx), upstream));
    }

    fn signature(&self, cx: &App) -> String {
        format!(
            "{}|{}|{:?}",
            self.narrow(cx).key(),
            self.controls.filter.as_str(),
            self.pages
        )
    }

    /// A list's rows: `pages` of it, or while one more loads the pages before.
    fn rows(
        &self,
        name: &str,
        pages: usize,
        cx: &App,
    ) -> (Option<Arc<Vec<PullItem>>>, usize, bool, Option<String>) {
        let narrow = self.narrow(cx);
        let read = Cache::read::<Vec<PullItem>>(
            cx,
            &Self::list_key(name, self.controls.filter, &narrow, pages),
        );
        if read.data.is_none() && pages > 1 {
            let shorter = Cache::read::<Vec<PullItem>>(
                cx,
                &Self::list_key(name, self.controls.filter, &narrow, pages - 1),
            );
            if shorter.data.is_some() {
                return (shorter.data, pages - 1, read.loading, read.error);
            }
        }
        (read.data, pages, read.loading, read.error)
    }

    fn counts(&self, name: &str, cx: &App) -> Option<Counts> {
        Cache::read::<Counts>(cx, &Self::counts_key(name, &self.narrow(cx)))
            .data
            .map(|c| *c)
    }

    /// CI for the open PRs shown, kept current while they are.
    fn watch_ci(&mut self, lists: Vec<(Target, Vec<String>)>, cx: &mut Context<Self>) {
        let shas: Vec<String> = lists
            .iter()
            .flat_map(|(t, s)| s.iter().map(move |s| format!("{t:?}{s}")))
            .collect();
        if shas == self.ci_shas {
            return;
        }
        self.ci_shas = shas;
        self._ci = lists
            .into_iter()
            .map(|(t, s)| CiWatch::new(t, s, true, |this: &Self, cx| this.shown(cx), cx))
            .collect();
    }

    fn open(&self, item: PullItem, preview: bool, window: &mut Window, cx: &mut App) {
        crate::ui::open_pull(&self.ctx.workspace, item, preview, window, cx);
    }

    fn active_key(&self, cx: &App) -> Option<SharedString> {
        self.ctx
            .workspace
            .upgrade()?
            .read(cx)
            .tabs()
            .active_key()
            .cloned()
    }

    fn create(&mut self, to_parent: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account(cx) else {
            return;
        };
        let target = if to_parent {
            account.parent.clone()
        } else {
            account.origin.clone()
        };
        let (Some(target), Some(origin), Some(branch)) =
            (target, account.origin.clone(), self.status.branch.clone())
        else {
            return;
        };
        crate::ui::dialogs::CreatePullDialog::open(
            self.ctx.workspace.clone(),
            target,
            origin,
            branch,
            self.status.clone(),
            window,
            cx,
        );
    }

    fn pick_labels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let upstream = self.upstream(cx);
        let repos = std::iter::once(None).chain(upstream.map(Some)).collect();
        let weak = cx.weak_entity();
        let hint =
            (self.controls.labels.len() > 1).then(|| "Pull requests with all of them".into());
        LabelPicker::open(
            repos,
            self.controls.labels.clone(),
            hint,
            std::rc::Rc::new(move |labels, window, cx| {
                weak.update(cx, |this, cx| {
                    this.controls.labels = labels;
                    this.narrowed(window, cx);
                    cx.notify();
                })
                .ok();
            }),
            None,
            window,
            cx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    /// A key on a pane's rows (see `rows`).
    fn row_key(
        &mut self,
        pane: Pane,
        rows: &[PullItem],
        more: bool,
        ev: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let urls: Vec<&str> = rows.iter().map(|p| p.url.as_str()).collect();
        let active = self
            .active_key(cx)
            .and_then(|k| k.strip_prefix("pull:").map(str::to_string));
        let nav = &mut self.rows[pane as usize];
        let at = nav.at(&urls, active.as_deref());
        let Some(key) = row_key(ev, at, urls.len(), more) else {
            return;
        };
        cx.stop_propagation();
        let at = at.unwrap_or(0);
        match key {
            RowKey::Move(to) => {
                nav.put(&urls, to);
                self.open(rows[to].clone(), true, window, cx);
            }
            RowKey::Keep => self.open(rows[at].clone(), false, window, cx),
            RowKey::Preview => self.open(rows[at].clone(), true, window, cx),
            RowKey::Page => {
                if active.as_deref() != Some(urls[at]) {
                    self.open(rows[at].clone(), true, window, cx);
                }
                focus_page(&self.ctx.workspace, window, cx);
            }
            RowKey::More => {
                self.pages[pane as usize] += 1;
                self.ask(false, cx);
            }
            RowKey::Menu => {
                nav.put(&urls, at);
                nav.open_menu(at, window, cx);
            }
        }
        cx.notify();
    }

    #[allow(clippy::too_many_arguments)]
    fn rows_view(
        &self,
        pane: Pane,
        target: Option<String>,
        loaded: Option<Arc<Vec<PullItem>>>,
        shown_pages: usize,
        loading: bool,
        error: Option<String>,
        roomy: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = theme(cx);
        let narrow = self.narrow(cx);
        let max_pages = if narrow.is_narrowed() {
            MAX_SEARCH_PAGES
        } else {
            MAX_PAGES
        };
        let full = loaded
            .as_ref()
            .is_some_and(|l| l.len() == shown_pages * PAGE);
        let rows: Arc<Vec<PullItem>> = match &loaded {
            Some(l) if !self.controls.needle.is_empty() => Arc::new(
                l.iter()
                    .filter(|p| {
                        self.controls.matches(&[
                            &p.title,
                            &format!("#{}", p.number),
                            &p.author,
                            &p.head_ref,
                        ])
                    })
                    .cloned()
                    .collect(),
            ),
            Some(l) => l.clone(),
            None => Arc::new(vec![]),
        };
        let mut col = div().flex().flex_col().size_full().min_h_0();
        if let Some(e) = &error {
            col = col.child(list_error(
                ("list-error", pane as usize),
                e,
                loaded.is_some(),
                &t,
                cx,
            ));
        }
        if loaded.is_some() && rows.is_empty() {
            let text = if !self.controls.needle.is_empty()
                && loaded.as_ref().is_some_and(|l| !l.is_empty())
            {
                format!(
                    "None of the {} loaded pull requests match.",
                    loaded.as_ref().map_or(0, |l| l.len())
                )
            } else {
                empty_text(ListKind::Pulls, self.controls.filter, &narrow)
            };
            col = col.child(empty_note(text, roomy, &t));
            if narrow.is_narrowed() && self.controls.needle.is_empty() {
                col = col.child(
                    div().flex().justify_center().child(
                        Button::new(("clear-narrow", pane as usize))
                            .ghost()
                            .xsmall()
                            .label("Clear filter")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.controls.clear(cx);
                                this.narrowed(window, cx);
                            })),
                    ),
                );
            }
        }
        let active = self.active_key(cx);
        let weak = cx.weak_entity();
        let account = self.account(cx);
        let count = rows.len();
        let list_target = target.clone();
        let nav = &self.rows[pane as usize];
        let cursor = nav.cursor.clone().filter(|_| nav.is_focused(window));
        let (key_rows, more) = (rows.clone(), full && shown_pages < max_pages);
        let list = uniform_list(("pulls", pane as usize), count, move |range, _, cx| {
            let t = theme(cx);
            range
                .map(|ix| {
                    pull_row(
                        ix,
                        pane,
                        &rows[ix],
                        list_target.clone(),
                        active.as_ref(),
                        cursor.as_deref(),
                        account.clone(),
                        weak.clone(),
                        &t,
                        cx,
                    )
                })
                .collect::<Vec<_>>()
        })
        .track_scroll(&nav.scroll)
        .flex_1()
        .min_h_0();
        col = col.child(
            div()
                .id(("pull-rows", pane as usize))
                .track_focus(&nav.focus)
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .on_key_down(
                    cx.listener(move |this, ev: &gpui::KeyDownEvent, window, cx| {
                        this.row_key(pane, &key_rows, more, ev, window, cx)
                    }),
                )
                .child(list),
        );
        if full && shown_pages < max_pages {
            col = col.child(
                div().p(px(8.)).flex_none().child(
                    Button::new(("more", pane as usize))
                        .small()
                        .w_full()
                        .label(if loading { "Loading…" } else { "Load more" })
                        .disabled(loading)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.pages[pane as usize] += 1;
                            this.ask(false, cx);
                            cx.notify();
                        })),
                ),
            );
        }
        if full && shown_pages >= max_pages {
            let n = loaded.as_ref().map_or(0, |l| l.len());
            col = col.child(
                div()
                    .flex_none()
                    .px(px(16.))
                    .py(px(8.))
                    .text_center()
                    .text_size(rems(0.69))
                    .text_color(t.subtle)
                    .child(format!("Showing the {n} most recently updated")),
            );
        }
        col.into_any_element()
    }
}

#[allow(clippy::too_many_arguments)]
fn pull_row(
    ix: usize,
    pane: Pane,
    p: &PullItem,
    target: Option<String>,
    active: Option<&SharedString>,
    cursor: Option<&str>,
    account: Option<Arc<Account>>,
    panel: WeakEntity<PullsPanel>,
    t: &gitviber_ui::AppTheme,
    cx: &App,
) -> AnyElement {
    let key = crate::ui::pull_key(&p.url);
    let on = active.is_some_and(|a| *a == key);
    let keys_here = cursor == Some(p.url.as_str());
    let (author, author_panel) = (p.author.clone(), panel.clone());
    let underline = t.foreground;
    let state = if p.state == PullState::Open {
        ci::state(cx, &target, &p.head_sha)
    } else {
        None
    };
    let hover = t.hover;
    let item = p.clone();
    let (menu_item, menu_panel, menu_target) = (p.clone(), panel.clone(), target);
    let meta = div()
        .mt(px(2.))
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(rems(0.656))
        .text_color(t.subtle)
        .child(
            div()
                .flex_none()
                .font_family(t.fonts.code.clone())
                .child(format!("#{}", p.number)),
        )
        .child("·")
        // A click on the author narrows the list to theirs, as GitHub's lists do.
        .child(
            div()
                .id(("author", ix))
                .flex_none()
                .cursor_pointer()
                .hover(move |s| s.text_color(underline).underline())
                .child(p.author.clone())
                .on_click(move |ev, window, cx| {
                    let m = ev.modifiers();
                    if m.platform || m.control || m.shift || m.alt {
                        return;
                    }
                    cx.stop_propagation();
                    let author = author.clone();
                    author_panel
                        .update(cx, |this, cx| {
                            this.controls.set_author(Some(author), cx);
                            this.narrowed(window, cx);
                            cx.notify();
                        })
                        .ok();
                }),
        )
        .child("·")
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_family(t.fonts.code.clone())
                .child(p.head_ref.clone()),
        )
        .child(div().flex_1())
        .children(ci_badge(("ci", ix), state, t))
        .child(div().flex_none().child(ago(&p.updated_at)));
    div()
        .id(("pull", ix))
        .relative()
        // A list row is laid out on its own: without the full width it shrinks to its text,
        // and a long title neither truncates nor leaves the hover the list's width.
        .w_full()
        .h(px(ROW))
        .flex()
        .gap(px(10.))
        .py(px(6.))
        .pl(px(12.))
        .pr(px(8.))
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
        .child(div().mt(px(2.)).child(pull_icon(p.state, p.draft, t)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(rems(0.75))
                        .text_color(t.foreground.opacity(0.9))
                        .child(p.title.clone()),
                )
                .child(meta),
        )
        .on_click(move |ev, window, cx| {
            let pin = ev.click_count() > 1;
            let item = item.clone();
            panel
                .update(cx, |this, cx| {
                    this.rows[pane as usize].cursor = Some(item.url.clone());
                    this.open(item, !pin, window, cx)
                })
                .ok();
        })
        .context_menu(move |menu, _, _| {
            let p = menu_item.clone();
            let source = if p.state == PullState::Open {
                pull_source(&p, account.as_deref())
            } else {
                None
            };
            let (guide, author, panel) = (p.clone(), p.author.clone(), menu_panel.clone());
            let target = menu_target.clone();
            let mut menu = menu.item(
                PopupMenuItem::new("Guided Review")
                    .icon(IconName::Sparkles)
                    .on_click(move |_, window, cx| {
                        request(
                            GithubRequest::GuidedReview {
                                pull: guide.clone(),
                                target: target.clone(),
                            },
                            window,
                            cx,
                        )
                    }),
            );
            if let Some(source) = source {
                menu = menu.item(
                    PopupMenuItem::new("Check out in new worktree…")
                        .icon(IconName::FolderGit2)
                        .on_click(move |_, window, cx| {
                            request(
                                GithubRequest::NewWorktreeForPull(source.clone()),
                                window,
                                cx,
                            )
                        }),
                );
            }
            let (copy_url, open_url) = (p.url.clone(), p.url.clone());
            menu.item(
                PopupMenuItem::new(format!("Filter by author {author}"))
                    .icon(IconName::ListFilter)
                    .on_click(move |_, window, cx| {
                        let author = author.clone();
                        panel
                            .update(cx, |this, cx| {
                                this.controls.set_author(Some(author), cx);
                                this.narrowed(window, cx);
                                cx.notify();
                            })
                            .ok();
                    }),
            )
            .separator()
            .item(
                PopupMenuItem::new("Copy link")
                    .icon(IconName::Link)
                    .on_click(move |_, window, cx| {
                        crate::ui::common::copy(&copy_url, "Link copied", window, cx)
                    }),
            )
            .item(
                PopupMenuItem::new("Open on GitHub")
                    .icon(IconName::ExternalLink)
                    .on_click(move |_, _, cx| crate::ui::common::open_url(&open_url, cx)),
            )
        })
        .into_any_element()
}

fn pane_header(
    title: &'static str,
    detail: String,
    count: Option<u64>,
    new: Option<AnyElement>,
    t: &gitviber_ui::AppTheme,
) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(28.))
        .px(px(10.))
        .bg(t.panel)
        .border_b_1()
        .border_color(t.border)
        .text_size(rems(0.72))
        .child(div().font_weight(t.semibold()).child(title))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(t.subtle)
                .child(detail),
        )
        .when_some(count, |d, n| d.child(CountBadge::new(compact_count(n))))
        .child(div().flex_1())
        .children(new)
        .into_any_element()
}

impl Render for PullsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let account = self.account(cx);
        let upstream = account.as_ref().and_then(|a| a.upstream());
        // Asked again when what it shows changed: the narrowing, the page, a wake, the fork.
        let signature = self.signature(cx);
        if self
            .asked
            .as_ref()
            .is_none_or(|(s, w, u)| *s != signature || *w != Cache::wakes(cx) || *u != upstream)
        {
            self.ask(false, cx);
        }
        let acct = crate::account::current_account(cx);
        let (own, own_pages, own_loading, own_error) = self.rows("origin", self.pages[0], cx);
        let failure = acct.error.clone().or(own_error.clone());
        if let Some(gate) = gate_view(
            &mut self.connect,
            "pull requests",
            &acct,
            own_error.as_deref(),
            self.ctx.workspace.clone(),
            &t,
            cx,
        ) {
            return div()
                .id("pulls-panel")
                .track_focus(&self.focus)
                .size_full()
                .child(gate)
                .into_any_element();
        }
        let up = upstream.as_ref().map(|u| self.rows(u, self.pages[1], cx));
        let open_heads = |rows: Option<&Arc<Vec<PullItem>>>| -> Vec<String> {
            rows.map(|r| {
                r.iter()
                    .filter(|p| p.state == PullState::Open)
                    .map(|p| p.head_sha.clone())
                    .collect()
            })
            .unwrap_or_default()
        };
        let mut watch = vec![(None, open_heads(own.as_ref()))];
        if let Some((rows, ..)) = &up {
            watch.push((upstream.clone(), open_heads(rows.as_ref())));
        }
        self.watch_ci(watch, cx);
        let shown = || {
            own.iter()
                .chain(up.iter().filter_map(|u| u.0.as_ref()))
                .flat_map(|r| r.iter())
        };
        let loading = acct.loading || own_loading || up.as_ref().is_some_and(|u| u.2);

        // This branch's open PR, in either repository. Another fork's same-named branch isn't it.
        let origin_name = account
            .as_ref()
            .and_then(|a| a.origin.as_ref())
            .map(|o| o.full_name().to_lowercase());
        let current = self.status.branch.as_ref().and_then(|b| {
            shown().find(|p| {
                &p.head_ref == b
                    && p.state == PullState::Open
                    && origin_name.as_ref().is_none_or(|o| {
                        p.head_repo.as_ref().is_some_and(|h| h.to_lowercase() == *o)
                    })
            })
        });
        let new_label: SharedString = if let Some(f) = &self.status.follows {
            format!("This branch is pull request {f}").into()
        } else if let Some(p) = current {
            format!("#{} already open for this branch", p.number).into()
        } else {
            "New pull request".into()
        };
        let can_create = self.status.branch.is_some()
            && self.status.follows.is_none()
            && current.is_none()
            && account.as_ref().is_some_and(|a| a.origin.is_some());
        let fork = upstream.is_some() && account.as_ref().is_some_and(|a| a.parent.is_some());
        let own_counts = self.counts("origin", cx);
        let header_counts = if account.is_none() || fork {
            None
        } else {
            own_counts
        };
        let me_off = (!self.me_usable(cx)).then(|| {
            if acct.error.as_deref().is_some_and(is_not_connected) {
                "Sign in to GitHub to filter by you"
            } else {
                "Filters by you need your GitHub account, which couldn't be read"
            }
        });

        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.))
            .h(px(32.))
            .px(px(8.))
            .border_b_1()
            .border_color(t.border)
            .child(filter_tabs(&self.controls, header_counts, cx))
            .child(div().flex_1())
            .child(
                Button::new("pick-labels")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Tag)
                    .label(if self.controls.labels.is_empty() {
                        "Label".to_string()
                    } else {
                        format!("Label {}", self.controls.labels.len())
                    })
                    .tooltip("Filter by label")
                    .on_click(cx.listener(|this, _, window, cx| this.pick_labels(window, cx))),
            )
            .child(
                Button::new("refresh")
                    .ghost()
                    .xsmall()
                    .icon(IconName::RefreshCw)
                    .loading(loading)
                    .tooltip("Refresh")
                    .on_click(cx.listener(|this, _, _, cx| this.ask(true, cx))),
            )
            .when(!fork, |d| {
                d.child(
                    Button::new("new-pull")
                        .small()
                        .icon(IconName::Plus)
                        .label("New")
                        .tooltip(new_label.clone())
                        .disabled(!can_create)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.create(false, window, cx)),
                        ),
                )
            });

        let body: AnyElement = if fork {
            let (up_rows, up_pages, up_loading, up_error) =
                up.clone().unwrap_or((None, 1, false, None));
            let up_name = upstream.clone().unwrap_or_default();
            let new = |id: &'static str, parent: bool, cx: &mut Context<Self>| {
                Button::new(id)
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .tooltip(new_label.clone())
                    .disabled(!can_create)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.create(parent, window, cx)),
                    )
                    .into_any_element()
            };
            let origin_detail = account
                .as_ref()
                .and_then(|a| a.origin.as_ref())
                .map(|o| o.full_name())
                .unwrap_or_default();
            let filter = self.controls.filter;
            let pick = |c: Option<Counts>| {
                c.map(|c| match filter {
                    Filter::Open => c.open,
                    Filter::Closed => c.closed,
                    Filter::All => c.open + c.closed,
                })
            };
            let up_counts = self.counts(&up_name, cx);
            let new_own = new("new-own", false, cx);
            let new_up = new("new-up", true, cx);
            let own_view = self.rows_view(
                Pane::Origin,
                None,
                own.clone(),
                own_pages,
                own_loading,
                failure.clone(),
                false,
                window,
                cx,
            );
            let up_view = self.rows_view(
                Pane::Parent,
                upstream.clone(),
                up_rows,
                up_pages,
                up_loading,
                up_error,
                false,
                window,
                cx,
            );
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(pane_header(
                    "Your fork",
                    origin_detail,
                    pick(own_counts),
                    Some(new_own),
                    &t,
                ))
                .child(div().flex_1().min_h_0().child(own_view))
                .child(pane_header(
                    "Original",
                    up_name,
                    pick(up_counts),
                    Some(new_up),
                    &t,
                ))
                .child(div().flex_1().min_h_0().child(up_view))
                .into_any_element()
        } else {
            let view = self.rows_view(
                Pane::Origin,
                None,
                own.clone(),
                own_pages,
                own_loading,
                failure.clone(),
                true,
                window,
                cx,
            );
            div()
                .flex_1()
                .min_h_0()
                .pt(px(4.))
                .child(view)
                .into_any_element()
        };
        let _ = window;
        div()
            .id("pulls-panel")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &editor::Find, window, cx| {
                this.controls.open_find(window, cx);
                cx.notify();
            }))
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(narrow_bar(&self.controls, me_off, cx))
            .children(selected_labels(&self.controls.labels, cx))
            .children(find_box(&self.controls, cx))
            .child(body)
            .children(account.map(|a| signed_in_as(&a.login, &a.source, &t)))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::BranchStatus;
    use gitviber_core::git::PushTarget;

    fn push(remote: &str, branch: Option<&str>, ahead: u32) -> PushTarget {
        PushTarget {
            remote: remote.into(),
            branch: branch.map(str::to_string),
            ahead,
        }
    }

    #[test]
    fn a_pr_pushes_first_unless_github_has_the_branch_as_it_is() {
        let s = |p: Option<&PushTarget>| BranchStatus::read(Some("fix".into()), None, p);
        assert!(!s(Some(&push("origin", Some("origin/fix"), 0))).needs_push());
        // An agent committed while the dialog was open.
        assert!(s(Some(&push("origin", Some("origin/fix"), 2))).needs_push());
        // Never published.
        assert!(s(Some(&push("origin", None, 0))).needs_push());
        assert!(s(None).needs_push());
        assert_eq!(
            s(Some(&push("upstream", Some("upstream/fix"), 0)))
                .push_elsewhere
                .as_deref(),
            Some("upstream")
        );
    }
}
