//! Stale-while-revalidate for GitHub's lists: the account, PR and issue lists and counts, labels.
//! A view shows the last answer at once and asks again in the background at most every
//! `MIN_AGE`; views asking for the same key share one request. Bounded to `MAX_ENTRIES`, the
//! least recently used going first. Bodies, threads and files never come here: their tab reads
//! and holds them, so closing it frees them.
//!
//! GitHub doesn't say when something changes there (an issue closed by a push, a teammate's
//! review), so shown views recheck on window focus and every `POLL`, each within its max age;
//! an unchanged list is a 304 that costs no rate limit.

use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gitviber_core::Core;
use gpui_kit::{App, AppContext as _, Global, Task};

use crate::model::is_not_connected;

pub const MIN_AGE: Duration = Duration::from_secs(30);
/// Enough for every list a session shows; beyond it the stalest go.
const MAX_ENTRIES: usize = 200;
const POLL: Duration = Duration::from_secs(300);
// `gh pr create` pushes, then opens the PR a moment later; `gh pr merge -d` moves refs too. So a
// git change rereads the PR lists once things settle, and an agent's run of commits asks GitHub
// at most every PULLS_GAP.
const PULLS_SETTLE: Duration = Duration::from_secs(3);
const PULLS_GAP: Duration = Duration::from_secs(15);

type Data = Arc<dyn Any + Send + Sync>;
type Fetch = Arc<dyn Fn(&Core) -> Result<Data, String> + Send + Sync>;
type Same = fn(&Data, &Data) -> bool;

struct Entry {
    data: Option<Data>,
    error: Option<String>,
    /// When `data` arrived.
    at: Option<Instant>,
    /// When `error` arrived.
    failed_at: Option<Instant>,
    pending: Option<Task<()>>,
    /// A forced read asked for while one was on its way, which may predate what it must see.
    again: Option<(Fetch, Same)>,
    /// Changes with what a view would show: data, error, loading.
    rev: u64,
    used: u64,
}

impl Entry {
    fn new() -> Self {
        Entry {
            data: None,
            error: None,
            at: None,
            failed_at: None,
            pending: None,
            again: None,
            rev: 0,
            used: 0,
        }
    }
}

/// What a key holds now.
pub struct Read<T> {
    pub data: Option<Arc<T>>,
    pub error: Option<String>,
    pub loading: bool,
}

impl<T> Read<T> {
    fn none() -> Self {
        Read {
            data: None,
            error: None,
            loading: false,
        }
    }
}

/// Bumped by the wakes: views compare theirs, and recheck what they show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wakes {
    pub all: u64,
    /// The PR lists only: issues, a PR's page and checks don't follow from a push.
    pub pulls: u64,
    /// The issue lists, after an issue view wrote.
    pub issues: u64,
}

#[derive(Default)]
pub struct Cache {
    entries: HashMap<String, Entry>,
    clock: u64,
    /// Bumped by a reset: a reply that lands after it belongs to the previous repo.
    generation: u64,
    wakes: Wakes,
    repo: Option<PathBuf>,
    /// Origin's URL the entries were read for; None until a status says.
    origin: Option<Option<String>>,
    poll: Option<Task<()>>,
    pulls_timer: Option<Task<()>>,
    pulls_at: Option<Instant>,
}

impl Global for Cache {}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(Cache::default());
    let poll = cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(POLL).await;
            cx.update(Cache::wake);
        }
    });
    cx.global_mut::<Cache>().poll = Some(poll);
}

fn same<T: PartialEq + 'static>(a: &Data, b: &Data) -> bool {
    match (a.downcast_ref::<T>(), b.downcast_ref::<T>()) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

impl Cache {
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    pub fn read<T: Send + Sync + 'static>(cx: &App, key: &str) -> Read<T> {
        let Some(e) = cx.try_global::<Cache>().and_then(|c| c.entries.get(key)) else {
            return Read::none();
        };
        Read {
            data: e.data.clone().and_then(|d| d.downcast::<T>().ok()),
            error: e.error.clone(),
            loading: e.pending.is_some(),
        }
    }

    /// Changes whenever what `keys` hold does: a view keeps it to skip redraws for others' news.
    pub fn stamp(cx: &App, keys: &[&str]) -> u64 {
        let Some(c) = cx.try_global::<Cache>() else {
            return 0;
        };
        keys.iter().fold(c.generation, |acc, k| {
            acc.wrapping_mul(1_000_003)
                .wrapping_add(c.entries.get(*k).map_or(0, |e| e.rev))
        })
    }

    pub fn wakes(cx: &App) -> Wakes {
        cx.try_global::<Cache>()
            .map(|c| c.wakes)
            .unwrap_or_default()
    }

    /// Reads `key` with `fetch` unless its answer is younger than `max_age` (zero forces a
    /// read). A read on its way is shared; a forced one waits for it, then reads again.
    pub fn load<T, F>(cx: &mut App, key: &str, max_age: Duration, fetch: F)
    where
        T: PartialEq + Send + Sync + 'static,
        F: Fn(&Core) -> Result<T, String> + Send + Sync + 'static,
    {
        let fetch: Fetch = Arc::new(move |core| fetch(core).map(|t| Arc::new(t) as Data));
        Cache::start(cx, key, max_age, fetch, same::<T>);
    }

    fn start(cx: &mut App, key: &str, max_age: Duration, fetch: Fetch, same: Same) {
        let now = Instant::now();
        let fresh = |at: Option<Instant>| at.is_some_and(|at| now.duration_since(at) < max_age);
        if let Some(e) = cx.try_global::<Cache>().and_then(|c| c.entries.get(key)) {
            if e.pending.is_some() && !max_age.is_zero() {
                return;
            }
            if e.pending.is_none() && e.data.is_some() && e.error.is_none() && fresh(e.at) {
                return;
            }
            // No sign-in is no answer to retry on every look: finding a token runs `gh` and git's
            // credential helper each time. It holds like data does; the Connect screen forces.
            if e.pending.is_none()
                && e.error.as_deref().is_some_and(is_not_connected)
                && fresh(e.failed_at)
            {
                return;
            }
        }
        let core = crate::core(cx);
        let cache = cx.default_global::<Cache>();
        let generation = cache.generation;
        let used = cache.tick();
        let rev = cache.tick();
        let entry = cache
            .entries
            .entry(key.to_string())
            .or_insert_with(Entry::new);
        entry.used = used;
        if entry.pending.is_some() {
            entry.again = Some((fetch, same));
            return;
        }
        entry.rev = rev;
        let owned = key.to_string();
        let task = cx.spawn(async move |cx| {
            let result = cx.background_spawn(async move { fetch(&core) }).await;
            cx.update(|cx| Cache::landed(cx, &owned, generation, result, same));
        });
        if let Some(e) = cx.global_mut::<Cache>().entries.get_mut(key) {
            e.pending = Some(task);
        }
        Cache::evict(cx);
    }

    fn landed(cx: &mut App, key: &str, generation: u64, result: Result<Data, String>, same: Same) {
        let cache = cx.global_mut::<Cache>();
        if cache.generation != generation {
            return;
        }
        let rev = cache.tick();
        let Some(e) = cache.entries.get_mut(key) else {
            return;
        };
        e.pending = None;
        e.rev = rev;
        match result {
            Ok(data) => {
                // A recheck with the same answer (a 304, mostly) keeps the old copy: what reads it
                // compares by pointer and doesn't redraw every half minute for nothing.
                if !e.data.as_ref().is_some_and(|old| same(old, &data)) {
                    e.data = Some(data);
                }
                e.error = None;
                e.at = Some(Instant::now());
            }
            Err(error) => {
                e.error = Some(error);
                e.failed_at = Some(Instant::now());
            }
        }
        if let Some((fetch, same)) = e.again.take() {
            Cache::start(cx, key, Duration::ZERO, fetch, same);
        }
    }

    fn evict(cx: &mut App) {
        let cache = cx.global_mut::<Cache>();
        while cache.entries.len() > MAX_ENTRIES {
            let Some(oldest) = cache
                .entries
                .iter()
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            cache.entries.remove(&oldest);
        }
    }

    /// Marks every key starting with `prefix` stale, so the next look reads it again.
    pub fn invalidate(cx: &mut App, prefix: &str) {
        let cache = cx.global_mut::<Cache>();
        for (key, e) in cache.entries.iter_mut() {
            if key.starts_with(prefix) {
                e.at = None;
            }
        }
    }

    /// Drops every answer and has the shown views read again: another account, or origin moved.
    pub fn refetch_all(cx: &mut App) {
        let cache = cx.global_mut::<Cache>();
        cache.entries.clear();
        cache.generation += 1;
        cache.wakes.all += 1;
        cache.wakes.pulls += 1;
        cache.wakes.issues += 1;
        crate::ci::reset(cx);
    }

    /// A project's views say which repository they read; another one starts over, so one repo's
    /// answers never show in another's workspace.
    pub fn use_repo(cx: &mut App, root: &Path) {
        if cx
            .try_global::<Cache>()
            .is_some_and(|c| c.repo.as_deref() == Some(root))
        {
            return;
        }
        let cache = cx.global_mut::<Cache>();
        cache.repo = Some(root.to_path_buf());
        cache.origin = None;
        Cache::refetch_all(cx);
    }

    pub fn repo_is(cx: &App, root: &Path) -> bool {
        cx.try_global::<Cache>()
            .is_some_and(|c| c.repo.as_deref() == Some(root))
    }

    /// Origin's URL from each status read. A new one (`git remote set-url`) is another
    /// repository on GitHub: what was read for the old one goes.
    pub fn set_origin(cx: &mut App, url: Option<String>) {
        let Some(cache) = cx.try_global::<Cache>() else {
            return;
        };
        if cache.origin.as_ref() == Some(&url) {
            return;
        }
        let known = cache.origin.is_some();
        cx.global_mut::<Cache>().origin = Some(url);
        if known {
            Cache::refetch_all(cx);
        }
    }

    pub fn origin(cx: &App) -> Option<String> {
        cx.try_global::<Cache>()
            .and_then(|c| c.origin.clone())
            .flatten()
    }

    /// Window focus or the poll: the shown views recheck.
    pub fn wake(cx: &mut App) {
        let cache = cx.global_mut::<Cache>();
        cache.wakes.all += 1;
    }

    /// After git changed the repo on disk (a terminal, an agent): the PR lists may be behind.
    pub fn recheck_pulls_soon(cx: &mut App) {
        let Some(cache) = cx.try_global::<Cache>() else {
            return;
        };
        if cache.pulls_timer.is_some() {
            return;
        }
        let gap = cache.pulls_at.map_or(Duration::ZERO, |at| {
            (at + PULLS_GAP).saturating_duration_since(Instant::now())
        });
        let wait = PULLS_SETTLE.max(gap);
        let timer = cx.spawn(async move |cx| {
            cx.background_executor().timer(wait).await;
            cx.update(|cx| {
                Cache::invalidate(cx, "pulls:");
                let cache = cx.global_mut::<Cache>();
                cache.pulls_timer = None;
                cache.pulls_at = Some(Instant::now());
                cache.wakes.pulls += 1;
            });
        });
        cx.global_mut::<Cache>().pulls_timer = Some(timer);
    }

    /// The rows of every list cached under `prefix`, for a view that reads what others loaded.
    pub fn rows<T: Clone + Send + Sync + 'static>(cx: &App, prefix: &str) -> Vec<T> {
        let Some(cache) = cx.try_global::<Cache>() else {
            return vec![];
        };
        cache
            .entries
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .filter_map(|(_, e)| e.data.as_ref()?.downcast_ref::<Vec<T>>())
            .flat_map(|rows| rows.iter().cloned())
            .collect()
    }

    /// Whether a cached list under `prefix` shows the item at `url` other than a detail read just
    /// did (closed or edited since): the detail and the list are fetched apart and can disagree.
    pub fn list_is_behind<T: Send + Sync + 'static>(
        cx: &App,
        prefix: &str,
        url: &str,
        updated_at: &str,
        row: impl Fn(&T) -> (&str, &str),
    ) -> bool {
        let Some(cache) = cx.try_global::<Cache>() else {
            return false;
        };
        cache
            .entries
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .filter_map(|(_, e)| e.data.as_ref()?.downcast_ref::<Vec<T>>())
            .flat_map(|rows| rows.iter())
            .any(|r| {
                let (u, at) = row(r);
                u == url && at != updated_at
            })
    }

    /// A PR view wrote (merged, closed, created): the PR lists read again.
    pub fn pulls_changed(cx: &mut App) {
        Cache::invalidate(cx, "pulls:");
        cx.global_mut::<Cache>().wakes.pulls += 1;
    }

    /// An issue view wrote (closed, edited, commented): the issue lists read again.
    pub fn issues_changed(cx: &mut App) {
        Cache::invalidate(cx, "issues:");
        cx.global_mut::<Cache>().wakes.issues += 1;
    }

    /// How many answers are kept, for the memory checks.
    pub fn len(cx: &App) -> usize {
        cx.try_global::<Cache>().map_or(0, |c| c.entries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn setup(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gitviber_ui::install_core(cx);
            cx.set_global(Cache::default());
            crate::ci::init(cx);
        });
    }

    #[gpui_kit::test]
    fn readers_of_one_key_share_a_request_and_a_fresh_answer_isnt_asked_again(
        cx: &mut TestAppContext,
    ) {
        setup(cx);
        let calls = Arc::new(AtomicUsize::new(0));
        for _ in 0..3 {
            let calls = calls.clone();
            cx.update(|cx| {
                Cache::load(cx, "k", MIN_AGE, move |_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(vec![1u32, 2])
                })
            });
        }
        cx.run_until_parked();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let read = cx.update(|cx| Cache::read::<Vec<u32>>(cx, "k"));
        assert_eq!(read.data.as_deref(), Some(&vec![1, 2]));
        assert!(!read.loading);
        let again = calls.clone();
        cx.update(|cx| {
            Cache::load(cx, "k", MIN_AGE, move |_| {
                Ok(vec![again.fetch_add(1, Ordering::SeqCst) as u32])
            })
        });
        cx.run_until_parked();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[gpui_kit::test]
    fn the_same_answer_keeps_the_old_copy_and_a_failure_keeps_the_data(cx: &mut TestAppContext) {
        setup(cx);
        cx.update(|cx| Cache::load(cx, "k", MIN_AGE, |_| Ok(vec![1u32])));
        cx.run_until_parked();
        let first = cx
            .update(|cx| Cache::read::<Vec<u32>>(cx, "k"))
            .data
            .unwrap();
        cx.update(|cx| Cache::load(cx, "k", Duration::ZERO, |_| Ok(vec![1u32])));
        cx.run_until_parked();
        let second = cx
            .update(|cx| Cache::read::<Vec<u32>>(cx, "k"))
            .data
            .unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        cx.update(|cx| {
            Cache::load::<Vec<u32>, _>(cx, "k", Duration::ZERO, |_| Err("rate limited".into()))
        });
        cx.run_until_parked();
        let read = cx.update(|cx| Cache::read::<Vec<u32>>(cx, "k"));
        assert_eq!(read.error.as_deref(), Some("rate limited"));
        assert!(read.data.is_some());
    }

    #[gpui_kit::test]
    fn no_login_holds_like_data_until_forced(cx: &mut TestAppContext) {
        setup(cx);
        let calls = Arc::new(AtomicUsize::new(0));
        let ask = |cx: &mut TestAppContext, age: Duration| {
            let calls = calls.clone();
            cx.update(|cx| {
                Cache::load::<u32, _>(cx, "account", age, move |_| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err(gitviber_core::github::NOT_CONNECTED.into())
                })
            });
            cx.run_until_parked();
        };
        ask(cx, MIN_AGE);
        ask(cx, MIN_AGE);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        ask(cx, Duration::ZERO);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[gpui_kit::test]
    fn entries_are_bounded_and_another_repo_starts_over(cx: &mut TestAppContext) {
        setup(cx);
        for i in 0..(MAX_ENTRIES + 25) {
            cx.update(|cx| Cache::load(cx, &format!("pulls:{i}"), MIN_AGE, move |_| Ok(vec![i])));
        }
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| Cache::len(cx)), MAX_ENTRIES);
        // The first ones were the least recently used.
        assert!(
            cx.update(|cx| Cache::read::<Vec<usize>>(cx, "pulls:0"))
                .data
                .is_none()
        );
        assert_eq!(
            cx.update(|cx| Cache::rows::<usize>(cx, "pulls:")).len(),
            MAX_ENTRIES
        );
        cx.update(|cx| Cache::use_repo(cx, Path::new("/a")));
        assert_eq!(cx.update(|cx| Cache::len(cx)), 0);
    }

    #[gpui_kit::test]
    fn a_reply_after_a_reset_is_dropped(cx: &mut TestAppContext) {
        setup(cx);
        cx.update(|cx| {
            Cache::load(cx, "k", MIN_AGE, |_| Ok(1u32));
            Cache::refetch_all(cx);
        });
        cx.run_until_parked();
        assert!(cx.update(|cx| Cache::read::<u32>(cx, "k")).data.is_none());
    }
}
