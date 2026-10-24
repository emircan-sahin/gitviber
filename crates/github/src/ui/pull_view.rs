//! A pull request's tab: what it is and where it stands, merge, review, checkout, its checks
//! (why one failed, copied for the agent), its commits (pick one or a run of them to narrow
//! Files changed), its files, and the description and conversation. The detail, files and
//! threads live here only: closing the tab frees them.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gitviber_core::api;
use gitviber_core::git::FileChange;
use gitviber_core::github::{CheckFailure, PullDetail, PullFiles};
use gitviber_ui::AppTheme;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{FileIcon, LineCounts, PathLabel, StatusLetter};
use gitviber_ui::workspace::{PullRange, TabIcon, TabItem, Toast, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity,
    Window, div, prelude::FluentBuilder as _, px, rems, uniform_list,
};

use crate::cache::{Cache, MIN_AGE, Wakes};
use crate::ci::{self, CiWatch};
use crate::logic::actions::{
    MergeMethod, ReviewEvent, allowed_methods, pull_rights, pull_source, review_badge,
};
use crate::logic::checks::{check_note, checks_summary, failure_report, is_failing, sort_checks};
use crate::logic::pull_commits::{
    CommitRun, by_day, is_whole_pr, pick_label, picked_commits, run_to,
};
use crate::model::{Account, Check, PullItem, PullState, Target, repo_of};
use crate::requests::{GithubRequest, PullSource, request};
use crate::store::Store;
use crate::ui::common::{
    Attachments, Blocks, ago, ci_badge, copy, open_url, pull_icon, pull_pill, section, text_button,
    theme,
};
use crate::ui::dialogs::{ReviewDialog, confirm};

/// The files only change with the two commits: rarely worth reading again.
const FILES_AGE: Duration = Duration::from_secs(600);
/// What a merge or cherry-pick that stopped on conflicts says.
const CONFLICTS: &str = "Stopped on conflicts: resolve them in Changes, then push.";

/// The files Files changed lists, and the picked commits' range (base, head, label) they're from.
type Shown = (Arc<Vec<FileChange>>, Option<(String, String, String)>);

struct Failure {
    data: Option<Arc<CheckFailure>>,
    error: Option<String>,
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
    review: Option<Entity<TextareaState>>,
    review_pick: Option<ReviewEvent>,
    /// Goes up when a review lands; the dialog closes on it.
    reviews_done: u64,
    polls: u32,
    wakes: Wakes,
    generation: u64,
    focus: FocusHandle,
    scroll: ScrollHandle,
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
            review_pick: None,
            reviews_done: 0,
            polls: 0,
            wakes: Cache::wakes(cx),
            generation: Cache::generation(cx),
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
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
        self.blocks.sync(&texts, &web, &url, &signed, window, cx);
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
                    TextareaState::new(window, cx)
                        .rows(4)
                        .placeholder("Leave a comment (markdown)")
                });
                // Text in it keeps the tab: a preview tab would be replaced, a hidden one released.
                let sub = cx.subscribe(&b, |this: &mut Self, b, ev: &InputEvent, cx| {
                    if matches!(ev, InputEvent::Change) && !b.read(cx).value().trim().is_empty() {
                        let key = this.key();
                        this.ws.update(cx, |ws, cx| ws.pin_tab(&key, cx)).ok();
                    }
                });
                self._subs.push(sub);
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
        let text = body.read(cx).value().trim().to_string();
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

/// "Check out in new worktree…" without the worktree dialog: core makes it in `dir` (None:
/// beside the main one), and the window switches to it.
pub(crate) fn checkout_worktree(
    source: PullSource,
    dir: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
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
                    dir,
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
            .is_some_and(|b| !b.read(cx).value().trim().is_empty())
    }
}

fn branch_chip(name: &str, t: &AppTheme) -> AnyElement {
    div()
        .flex_none()
        .px(px(6.))
        .rounded(px(3.))
        .bg(t.primary.opacity(0.12))
        .text_color(t.primary)
        .font_family(t.fonts.code.clone())
        .text_size(rems(0.69))
        .child(name.to_string())
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

impl PullView {
    fn merge_box(
        &self,
        d: &PullDetail,
        account: Option<&Account>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = theme(cx);
        let p = PullItem::from(&d.pull);
        let rights = pull_rights(&p, account, d.closed_by.as_deref());
        let st = d.mergeable_state.as_str();
        let conflicts = d.mergeable == Some(false) || st == "dirty";
        let base = &d.pull.base_ref;
        let (tone, title, note): (gpui_kit::Hsla, String, &str) = if d.mergeable.is_none() {
            (
                t.border,
                "Checking mergeability…".into(),
                "GitHub is computing whether this can merge. Refresh in a moment.",
            )
        } else if d.pull.draft && !conflicts {
            (
                t.border,
                "This is a draft".into(),
                "GitHub won't merge it until it's marked ready for review.",
            )
        } else if conflicts {
            (
                t.conflict.opacity(0.6),
                format!("This branch has conflicts with {base}"),
                "Resolve them locally: GitViber merges the base into this branch and opens the conflicts.",
            )
        } else if st == "blocked" {
            (
                t.modified.opacity(0.6),
                "Merging is blocked".into(),
                "Required reviews or checks haven't passed yet.",
            )
        } else if st == "behind" {
            (
                t.modified.opacity(0.6),
                format!("This branch is behind {base}"),
                "It can still merge, but the base has newer commits.",
            )
        } else if st == "unstable" {
            (
                t.modified.opacity(0.6),
                "Checks are failing or pending".into(),
                "You can still merge if the repository allows it.",
            )
        } else {
            (
                t.added.opacity(0.6),
                "Ready to merge".into(),
                "No conflicts with the base branch.",
            )
        };
        let busy = self.busy.is_some();
        let side = |s: &str| {
            div()
                .max_w(px(220.))
                .text_right()
                .text_size(rems(0.72))
                .text_color(t.muted_foreground)
                .child(s.to_string())
                .into_any_element()
        };
        let action: AnyElement = if conflicts {
            // A fork's branch lives in another repo; pushing the fix here would go to the wrong place.
            if rights.resolve {
                Button::new("resolve")
                    .small()
                    .primary()
                    .icon(IconName::GitMerge)
                    .label("Resolve locally")
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, window, cx| this.resolve_locally(window, cx)))
                    .into_any_element()
            } else {
                side("From a fork: resolve it in the fork's repository.")
            }
        } else if !rights.merge {
            side("Only people with write access can merge.")
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
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .text_size(rems(0.72))
                            .text_color(t.muted_foreground)
                            .child(note),
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
                    .text_color(t.removed)
                    .child(format!("Could not load all checks: {e}")),
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
                .child(div().min_w_0().truncate().child(c.name.clone()))
                .when(!note.is_empty(), |d| {
                    d.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(rems(0.69))
                            .text_color(t.subtle)
                            .child(note),
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
                body = body.child(self.failure_view(id, c, cx));
            }
        }
        Some(body.into_any_element())
    }

    fn failure_view(&self, id: u64, check: &Check, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx);
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
                            .text_color(t.removed)
                            .child(
                                div()
                                    .flex_1()
                                    .child(format!("Could not read the check: {e}")),
                            )
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
                    .child(
                        div()
                            .text_color(t.subtle)
                            .child("Reading its output and log…"),
                    )
                    .into_any_element(),
            };
        };
        if !data.title.is_empty() {
            col = col.child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(data.title.clone()),
            );
        }
        if !data.summary.is_empty() {
            col = col.child(
                div()
                    .py(px(4.))
                    .text_color(t.muted_foreground)
                    .child(data.summary.clone()),
            );
        }
        for (i, a) in data.annotations.iter().enumerate() {
            let color = match a.level.as_str() {
                "failure" => t.removed,
                "warning" => t.modified,
                _ => t.subtle,
            };
            let place = if a.line > 0 {
                format!("{}:{}", a.path, a.line)
            } else {
                a.path.clone()
            };
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
                            .child(a.level.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(4.))
                            // Actions puts its own errors ("exit code 1") on .github, a folder.
                            .when(!a.path.is_empty() && a.path != ".github", |d| {
                                d.child(
                                    text_button(("annotation", i), place, &t)
                                        .text_color(t.primary)
                                        .font_family(t.fonts.code.clone())
                                        .on_click(move |_, window, cx| {
                                            request(
                                                GithubRequest::OpenFile {
                                                    path: path.clone(),
                                                    line,
                                                },
                                                window,
                                                cx,
                                            )
                                        }),
                                )
                            })
                            .when(!a.title.is_empty(), |d| {
                                d.child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(format!("{}:", a.title)),
                                )
                            })
                            .child(a.message.clone()),
                    ),
            );
        }
        if let Some(log) = &data.log {
            // The error is at the log's end: only the tail shows, the whole copies.
            let tail: Vec<&str> = log
                .lines()
                .rev()
                .take(40)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            col = col.child(
                div()
                    .id(("log", id as usize))
                    .mt(px(6.))
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .rounded(px(3.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.background)
                    .p(px(8.))
                    .font_family(t.fonts.code.clone())
                    .text_size(rems(0.69))
                    .children(tail.into_iter().map(|l| div().child(l.to_string()))),
            );
        }
        if let Some(e) = &data.annotations_error {
            col = col.child(
                div()
                    .mt(px(4.))
                    .text_color(t.removed)
                    .child(format!("Could not read the annotations: {e}")),
            );
        }
        if let Some(e) = &data.log_error {
            col = col.child(
                div()
                    .mt(px(4.))
                    .text_color(t.removed)
                    .child(format!("Could not read the job's log: {e}")),
            );
        }
        if data.title.is_empty()
            && data.summary.is_empty()
            && data.annotations.is_empty()
            && data.log.is_none()
            && data.log_error.is_none()
            && data.annotations_error.is_none()
        {
            col = col.child(
                div()
                    .text_color(t.subtle)
                    .child("The check left no output; its Details page may say more."),
            );
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

    fn commits_section(&self, total: u64, cx: &mut Context<Self>) -> Option<AnyElement> {
        let commits = self.commits();
        if commits.is_empty() {
            return None;
        }
        let t = theme(cx);
        let fresh = commits.iter().filter(|c| self.is_new(&c.sha)).count();
        let aside = if fresh > 0 {
            format!("{fresh} new · {}", commits.len())
        } else {
            commits.len().to_string()
        };
        let picked = self.picked().map(|(r, _)| r);
        let shas: Vec<String> = commits.iter().map(|c| c.sha.clone()).collect();
        let states = ci::states(cx, &self.target, &shas);
        let mut rows: Vec<AnyElement> = Vec::new();
        for (day, range) in by_day(commits) {
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .h(px(24.))
                    .px(px(12.))
                    .bg(t.panel)
                    .border_b_1()
                    .border_color(t.border)
                    .text_size(rems(0.69))
                    .text_color(t.subtle)
                    .child(format!("Commits on {day}"))
                    .into_any_element(),
            );
            for ix in range {
                let c = &commits[ix];
                let on = picked.as_ref().is_some_and(|r| r.contains(&ix));
                let open = self.open_bodies.contains(&c.sha);
                let (sha, body_sha, copy_sha, menu_sha) =
                    (c.sha.clone(), c.sha.clone(), c.sha.clone(), c.sha.clone());
                let hover = t.hover;
                let (url, not_in_head, short) = (
                    format!("{}/commits/{}", self.url, c.sha),
                    c.not_in_head,
                    c.short_sha.clone(),
                );
                let weak = cx.weak_entity();
                rows.push(
                    div()
                        .id(("commit", ix))
                        .relative()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .h(px(28.))
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
                        .child(
                            Icon::new(IconName::GitCommitHorizontal)
                                .xsmall()
                                .text_color(t.subtle),
                        )
                        .child(div().min_w_0().truncate().child(c.subject.clone()))
                        .when(!c.body.is_empty(), |d| {
                            d.child(
                                div()
                                    .id(("commit-body", ix))
                                    .flex_none()
                                    .px(px(4.))
                                    .rounded(px(3.))
                                    .bg(if open { t.active } else { t.hover })
                                    .text_size(rems(0.625))
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
                                .child(c.author_name.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(rems(0.69))
                                .text_color(t.subtle)
                                .child(gitviber_ui::widgets::relative_time(
                                    c.committed_at,
                                    gitviber_ui::widgets::now_unix(),
                                )),
                        )
                        .children(ci_badge(("commit-ci", ix), states.get(&c.sha).copied(), &t))
                        .child(
                            text_button(("sha", ix), short, &t)
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
                                    .on_click(move |_, window, cx| {
                                        copy(&copy_sha, "SHA copied", window, cx)
                                    }),
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
                                    .on_click(move |_, _, cx| {
                                        let sha = sha.clone();
                                        weak.update(cx, |this, cx| {
                                            this.act(
                                                "Checkout",
                                                format!("Checked out {}", &sha[..sha.len().min(7)]),
                                                move |core| {
                                                    done(api::history::checkout_commit(core, sha))
                                                },
                                                |_, _| {},
                                                cx,
                                            )
                                        })
                                        .ok();
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
                                                    format!(
                                                        "Cherry-picked {}",
                                                        &sha[..sha.len().min(7)]
                                                    ),
                                                    move |core| {
                                                        api::history::cherry_pick(core, sha)
                                                    },
                                                    |_, _| {},
                                                    cx,
                                                )
                                            })
                                            .ok();
                                    }),
                            )
                        })
                        .into_any_element(),
                );
                if open {
                    rows.push(
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
                            .children(c.body.lines().map(|l| div().child(l.to_string())))
                            .into_any_element(),
                    );
                }
            }
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
                    .child(format!("The latest {} of {total} commits.", commits.len())),
            );
        }
        // A long list scrolls on its own, so Files changed stays right under it.
        Some(
            s.child(
                div()
                    .id("commits")
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .children(rows),
            )
            .into_any_element(),
        )
    }

    fn pick_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (range, _) = self.picked()?;
        let t = theme(cx);
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
                        .child(format!(
                            "Showing changes from {n} commit{}",
                            if n == 1 { "" } else { "s" }
                        )),
                )
                .child(
                    div()
                        .flex_none()
                        .font_family(t.fonts.code.clone())
                        .child(pick_label(commits)),
                )
                .when(one, |d| {
                    d.child(div().min_w_0().truncate().child(commits[0].subject.clone()))
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

    fn files_section(&self, d: Option<&PullDetail>, cx: &mut Context<Self>) -> AnyElement {
        let t = theme(cx);
        let picked = self.picked();
        let narrowed = picked.as_ref().is_some_and(|(_, n)| *n);
        let shown: Option<Shown> = if narrowed {
            let (range, _) = picked.clone().expect("narrowed");
            self.picked_files.as_ref().map(|(b, h, files)| {
                (
                    files.clone(),
                    Some((b.clone(), h.clone(), pick_label(&self.commits()[range]))),
                )
            })
        } else {
            self.files.as_ref().map(|_| (self.file_list.clone(), None))
        };
        let aside = match (&shown, d) {
            (Some((files, _)), _) => Some(files.len().to_string().into()),
            (None, Some(d)) if !narrowed => Some(d.changed_files.to_string().into()),
            _ => None,
        };
        let mut s = section("Files changed", aside, &t).children(self.pick_bar(cx));
        let error = if narrowed {
            self.picked_error.clone()
        } else {
            self.files_error.clone()
        };
        match shown {
            None => {
                let text = match error {
                    Some(e) => format!(
                        "Could not load {} files: {e}",
                        if narrowed { "the commits'" } else { "the PR's" }
                    ),
                    None => {
                        if narrowed {
                            "Loading…".into()
                        } else {
                            "Fetching the PR's commits…".into()
                        }
                    }
                };
                s = s.child(
                    div()
                        .px(px(12.))
                        .py(px(8.))
                        .text_size(rems(0.75))
                        .text_color(t.subtle)
                        .child(text),
                );
            }
            Some((files, _)) if files.is_empty() => {
                s = s.child(
                    div()
                        .px(px(12.))
                        .py(px(8.))
                        .text_size(rems(0.75))
                        .text_color(t.subtle)
                        .child("No files changed."),
                )
            }
            Some((files, range)) => {
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
                                    .child(PathLabel(f.path.clone().into())),
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
                .h(px(28. * count.min(14) as f32));
                s = s.child(list);
            }
        }
        s.into_any_element()
    }

    fn conversation(&self, d: &PullDetail, cx: &mut Context<Self>) -> Option<AnyElement> {
        if d.comments.is_empty() {
            return None;
        }
        let t = theme(cx);
        let mut s = section(
            "Conversation",
            Some(d.comments.len().to_string().into()),
            &t,
        );
        for (i, c) in d.comments.iter().enumerate() {
            let md = self.blocks.get(i + 1).filter(|_| !c.body.trim().is_empty());
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
                    .child(review_badge(r))
            });
            s = s.child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(t.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(rems(0.72))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(c.author.clone()),
                            )
                            .children(verdict)
                            .child(div().text_color(t.subtle).child(ago(&c.created_at))),
                    )
                    .when(!c.body.trim().is_empty(), |d| {
                        d.children(md.map(|md| div().mt(px(6.)).child(md)))
                    }),
            );
        }
        Some(s.into_any_element())
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
        if self.text_stale {
            self.sync_text(window, cx);
        }
        if let Some(req) = self.pending_request.take() {
            window.defer(cx, move |window, cx| request(req, window, cx));
        }
        let account = self.account(cx);
        let d = self.detail.clone();
        let Some(p) = self.item() else {
            let text = self
                .detail_error
                .clone()
                .unwrap_or_else(|| "Loading the pull request…".into());
            return div()
                .id("pull")
                .track_focus(&self.focus)
                .size_full()
                .p(px(32.))
                .text_center()
                .text_size(rems(0.78))
                .text_color(t.muted_foreground)
                .child(text)
                .into_any_element();
        };
        if let (Some(e), None) = (&self.detail_error, &d)
            && self.item.is_none()
        {
            return div()
                .id("pull")
                .size_full()
                .p(px(32.))
                .text_center()
                .text_color(t.muted_foreground)
                .child(e.clone())
                .into_any_element();
        }
        let rights = pull_rights(
            &p,
            account.as_deref(),
            d.as_ref().and_then(|d| d.closed_by.as_deref()),
        );
        let source = pull_source(&p, account.as_deref());
        let busy = self.busy.is_some();
        let open = p.state == PullState::Open;
        let who = if p.state == PullState::Merged {
            d.as_ref()
                .and_then(|d| d.merged_by.clone())
                .unwrap_or(p.author.clone())
        } else {
            p.author.clone()
        };
        let verb = match p.state {
            PullState::Merged => "merged",
            PullState::Closed => "wanted to merge",
            PullState::Open => "wants to merge",
        };
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
            .child(div().text_color(t.foreground.opacity(0.85)).child(who))
            .child(verb)
            .child(branch_chip(&p.head_ref, &t))
            .child("into")
            .child(branch_chip(&p.base_ref, &t))
            .children(ci_badge("head-ci", head_ci.filter(|_| open), &t))
            .when_some(d.clone(), |m, d| {
                m.child(div().text_color(t.subtle).child("·"))
                    .child(format!(
                        "{} commit{}",
                        d.commits,
                        if d.commits == 1 { "" } else { "s" }
                    ))
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
            })
            // Cached data stays on screen; a failed refresh (a rate limit, say) is noted beside it.
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
            );

        let mut page = div()
            .mx_auto()
            .max_w(px(900.))
            .w_full()
            .px(px(24.))
            .py(px(20.))
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
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(p.title.clone())
                                    .child(
                                        div()
                                            .text_color(t.subtle)
                                            .font_weight(FontWeight::NORMAL)
                                            .child(format!("#{}", p.number)),
                                    ),
                            )
                            .child(meta),
                    ),
            )
            .child(actions);
        if let Some(d) = &d {
            if open {
                page = page.child(self.merge_box(d, account.as_deref(), cx));
            }
            page = page.children(self.checks_section(d, cx));
        }
        let total = d.as_ref().map_or(0, |d| d.commits);
        page = page.children(self.commits_section(total.max(self.commits().len() as u64), cx));
        page = page.child(self.files_section(d.as_deref(), cx));
        let body = d
            .as_ref()
            .filter(|d| !d.body.trim().is_empty())
            .and_then(|_| self.blocks.get(0));
        let desc = match (body, &d) {
            (Some(md), _) => div().px(px(12.)).py(px(10.)).child(md).into_any_element(),
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
        if let Some(d) = &d {
            page = page.children(self.conversation(d, cx));
        }
        div()
            .id("pull")
            .key_context("PullView")
            .track_focus(&self.focus)
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
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
            .child(page)
            .into_any_element()
    }
}
