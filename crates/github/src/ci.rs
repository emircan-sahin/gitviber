//! CI's verdict on commits, from GitHub's status rollup: for the PR list, a PR's commits, and for
//! other features (History, the worktree picker, the top bar). Asked in one request per list, kept
//! a while, and asked again sooner while running. Data only: badges are drawn by whoever shows them.
//!
//! ```ignore
//! let watch = gitviber_github::ci::CiWatch::new(None, shas, true, cx); // keep it on the view
//! cx.observe_global::<gitviber_github::ci::Ci>(|_, cx| cx.notify()).detach();
//! let state = gitviber_github::ci::state(cx, &None, &sha);
//! ```

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use gpui_kit::{App, AppContext as _, Context, Global, Task};

use crate::model::{CiState, Target};

const RUNNING: Duration = Duration::from_secs(30);
const SETTLED: Duration = Duration::from_secs(600);
/// No GitHub account or no access: not asked again for a while.
const FAILED: Duration = Duration::from_secs(300);
/// GitHub's rollup takes this many at once.
const BATCH: usize = 100;
/// Verdicts kept; the oldest answers go first.
const MAX_KNOWN: usize = 5000;

type Key = (Target, String);

#[derive(Default)]
pub struct Ci {
    known: HashMap<Key, (Option<CiState>, Instant)>,
    failed_at: HashMap<Target, Instant>,
    asking: HashSet<Key>,
    tasks: HashMap<u64, Task<()>>,
    next: u64,
    version: u64,
}

impl Global for Ci {}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(Ci::default());
}

/// Another repo, or origin moved: the old one's answers and failures don't hold for it.
pub(crate) fn reset(cx: &mut App) {
    if cx.try_global::<Ci>().is_some() {
        cx.set_global(Ci::default());
    }
}

fn stale(ci: &Ci, key: &Key, now: Instant) -> bool {
    match ci.known.get(key) {
        None => true,
        Some((state, at)) => {
            now.duration_since(*at)
                > if *state == Some(CiState::Pending) {
                    RUNNING
                } else {
                    SETTLED
                }
        }
    }
}

/// Asks GitHub for those of `shas` whose answer is old, 100 at a time; quiet on failure.
pub fn ask(cx: &mut App, target: &Target, shas: &[String]) {
    let Some(ci) = cx.try_global::<Ci>() else {
        return;
    };
    let now = Instant::now();
    if ci
        .failed_at
        .get(target)
        .is_some_and(|at| now.duration_since(*at) < FAILED)
    {
        return;
    }
    let wanted: Vec<String> = shas
        .iter()
        .filter(|s| {
            let key = (target.clone(), (*s).clone());
            stale(ci, &key, now) && !ci.asking.contains(&key)
        })
        .take(BATCH)
        .cloned()
        .collect();
    if wanted.is_empty() {
        return;
    }
    let core = crate::core(cx);
    let ci = cx.global_mut::<Ci>();
    for s in &wanted {
        ci.asking.insert((target.clone(), s.clone()));
    }
    ci.next += 1;
    let id = ci.next;
    let (to, list) = (target.clone(), wanted.clone());
    let target = target.clone();
    let task = cx.spawn(async move |cx| {
        let found = cx
            .background_spawn(async move { gitviber_core::api::github::ci_states(&core, to, list) })
            .await;
        cx.update(|cx| {
            let Some(ci) = cx.try_global::<Ci>() else {
                return;
            };
            if !ci.tasks.contains_key(&id) {
                return;
            }
            let ci = cx.global_mut::<Ci>();
            ci.tasks.remove(&id);
            for s in &wanted {
                ci.asking.remove(&(target.clone(), s.clone()));
            }
            match found {
                Ok(found) => {
                    let at = Instant::now();
                    for s in wanted {
                        let state = found.get(&s).and_then(|v| CiState::parse(v));
                        ci.known.insert((target.clone(), s), (state, at));
                    }
                    trim(ci);
                    ci.version += 1;
                }
                Err(_) => {
                    ci.failed_at.insert(target, Instant::now());
                }
            }
        });
    });
    cx.global_mut::<Ci>().tasks.insert(id, task);
}

fn trim(ci: &mut Ci) {
    if ci.known.len() <= MAX_KNOWN {
        return;
    }
    let mut by_age: Vec<(Key, Instant)> = ci
        .known
        .iter()
        .map(|(k, (_, at))| (k.clone(), *at))
        .collect();
    by_age.sort_by_key(|(_, at)| *at);
    for (k, _) in by_age.into_iter().take(ci.known.len() - MAX_KNOWN) {
        ci.known.remove(&k);
    }
}

/// CI's verdict on `sha` as last asked; None when it has no checks or isn't known yet.
pub fn state(cx: &App, target: &Target, sha: &str) -> Option<CiState> {
    cx.try_global::<Ci>()?
        .known
        .get(&(target.clone(), sha.to_string()))?
        .0
}

/// Each of `shas` that has a verdict.
pub fn states(cx: &App, target: &Target, shas: &[String]) -> HashMap<String, CiState> {
    shas.iter()
        .filter_map(|s| Some((s.clone(), state(cx, target, s)?)))
        .collect()
}

/// A commit's verdict from a newer read of all its checks, so its badges agree.
pub fn note(cx: &mut App, target: &Target, sha: &str, state: CiState) {
    let key = (target.clone(), sha.to_string());
    if cx
        .try_global::<Ci>()
        .and_then(|c| c.known.get(&key))
        .is_some_and(|(s, _)| *s == Some(state))
    {
        return;
    }
    let ci = cx.default_global::<Ci>();
    ci.known.insert(key, (Some(state), Instant::now()));
    ci.version += 1;
}

/// Bumped as answers land: compare to skip redraws.
pub fn version(cx: &App) -> u64 {
    cx.try_global::<Ci>().map_or(0, |c| c.version)
}

/// Keeps the verdicts of some commits current while its owner lives: every half minute while
/// `live`, else only on the GitHub views' wakes (focus, the 5-minute poll), for a badge that's
/// always on screen. Drop it (with its view) and the asking stops.
pub struct CiWatch {
    _task: Task<()>,
}

impl CiWatch {
    pub fn new<V: 'static>(
        target: Target,
        shas: Vec<String>,
        live: bool,
        cx: &mut Context<V>,
    ) -> Self {
        if shas.is_empty() {
            return CiWatch {
                _task: Task::ready(()),
            };
        }
        ask(cx, &target, &shas);
        let task = cx.spawn(async move |_, cx| {
            let mut wakes = cx.update(|cx| crate::cache::Cache::wakes(cx));
            loop {
                cx.background_executor().timer(RUNNING).await;
                let now = cx.update(|cx| crate::cache::Cache::wakes(cx));
                if live || now != wakes {
                    wakes = now;
                    cx.update(|cx| ask(cx, &target, &shas));
                }
            }
        });
        CiWatch { _task: task }
    }
}

/// One commit's checks in detail, for its header in the commit view: read only once its rollup
/// says it has some (most commits have none, and no access fails there, quietly and backed off),
/// again every half minute while one runs, and rarely once all are done. Create it with
/// `cx.new(|cx| CommitChecksModel::new(..))` and observe it.
pub struct CommitChecksModel {
    target: Target,
    sha: String,
    checks: Option<crate::model::CommitChecks>,
    error: Option<String>,
    read_at: Option<Instant>,
    _watch: CiWatch,
    _read: Task<()>,
    _tick: Task<()>,
    _ci: gpui_kit::Subscription,
}

impl CommitChecksModel {
    /// `url`: the commit's GitHub page; `web`: origin's page, whose commits are asked of origin.
    pub fn new(sha: String, url: &str, web: Option<&str>, cx: &mut Context<Self>) -> Self {
        let target: Target = match web {
            Some(web) if url.starts_with(&format!("{web}/commit/")) => None,
            _ => Some(crate::model::repo_of(url)),
        };
        let watch = CiWatch::new(target.clone(), vec![sha.clone()], false, cx);
        let ci = cx.observe_global::<Ci>(|this: &mut Self, cx| this.maybe_read(cx));
        let tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(RUNNING).await;
                if this.update(cx, |this, cx| this.maybe_read(cx)).is_err() {
                    break;
                }
            }
        });
        let mut model = CommitChecksModel {
            target,
            sha,
            checks: None,
            error: None,
            read_at: None,
            _watch: watch,
            _read: Task::ready(()),
            _tick: tick,
            _ci: ci,
        };
        model.maybe_read(cx);
        model
    }

    /// The verdict a full read gives, else the rollup's.
    pub fn state(&self, cx: &App) -> Option<CiState> {
        self.listed().or_else(|| state(cx, &self.target, &self.sha))
    }

    fn listed(&self) -> Option<CiState> {
        let c = self.checks.as_ref()?;
        if c.checks_error.is_some() || c.checks.is_empty() {
            return None;
        }
        crate::logic::checks::checks_verdict(&c.checks)
    }

    pub fn checks(&self) -> Option<&crate::model::CommitChecks> {
        self.checks.as_ref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The badge's text: "3/5 passed · 1 failing", or the rollup's label.
    pub fn summary(&self, cx: &App) -> Option<String> {
        let state = self.state(cx)?;
        Some(crate::logic::checks::commit_summary(
            self.checks.as_ref(),
            state.label(),
        ))
    }

    fn maybe_read(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.state(cx) else { return };
        let age = if state == CiState::Pending {
            RUNNING / 2
        } else {
            SETTLED
        };
        if self.read_at.is_some_and(|at| at.elapsed() < age) {
            return;
        }
        self.read_at = Some(Instant::now());
        let (core, target, sha) = (crate::core(cx), self.target.clone(), self.sha.clone());
        self._read = cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move {
                    gitviber_core::api::github::commit_checks(&core, target, sha)
                })
                .await;
            this.update(cx, |this, cx| {
                match read {
                    Ok(c) => {
                        this.checks = Some((&c).into());
                        this.error = None;
                        if let Some(fresh) = this.listed() {
                            let (target, sha) = (this.target.clone(), this.sha.clone());
                            note(cx, &target, &sha, fresh);
                        }
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        });
    }
}
