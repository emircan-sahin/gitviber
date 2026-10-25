//! The one `gitviber_core::Core` of the app and its events on the UI thread. Core calls block:
//! run them with `cx.background_spawn`, holding `core(cx)`'s Arc, never on the UI thread.
//!
//! ```ignore
//! let core = gitviber_ui::core(cx);
//! self.load = Some(cx.spawn(async move |this, cx| {
//!     let status = cx.background_spawn(async move { api::changes::status(&core) }).await;
//!     this.update(cx, |this, cx| { this.status = status.ok(); cx.notify() }).ok();
//! }));
//! cx.subscribe(&CoreEvents::global(cx), |this, _, event: &Event, cx| ..)
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt as _;
use futures::channel::{mpsc, oneshot};
use gitviber_core::api::repo::OpenedRepo;
use gitviber_core::{Core, Event, EventSink, Res, api};
use gpui_kit::{App, AppContext as _, Entity, EventEmitter, Global, Task};

/// Every core `Event`, emitted on the UI thread: subscribe with
/// `cx.subscribe(&CoreEvents::global(cx), ..)`.
pub struct CoreEvents;

impl EventEmitter<Event> for CoreEvents {}

impl CoreEvents {
    pub fn global(cx: &App) -> Entity<CoreEvents> {
        cx.global::<CoreGlobal>().events.clone()
    }
}

struct CoreGlobal {
    core: Arc<Core>,
    events: Entity<CoreEvents>,
    /// Takes events off the channel; dropped with the app.
    _pump: Task<()>,
}

impl Global for CoreGlobal {}

/// The app's core.
pub fn core(cx: &App) -> Arc<Core> {
    cx.global::<CoreGlobal>().core.clone()
}

pub fn has_core(cx: &App) -> bool {
    cx.has_global::<CoreGlobal>()
}

/// Makes the app's core, starts it (`Core::start`: the shell's PATH, logs, askpass, notifications,
/// the launch's paths) and puts its events on the UI thread. Once, before the first window.
pub fn start_core(cx: &mut App) -> Arc<Core> {
    let (tx, mut rx) = mpsc::unbounded::<Event>();
    let sink: EventSink = Arc::new(move |event| {
        let _ = tx.unbounded_send(event);
    });
    let core = Arc::new(Core::new(sink));
    let events = cx.new(|_| CoreEvents);
    let weak = events.downgrade();
    let pump = cx.spawn(async move |cx| {
        while let Some(event) = rx.next().await {
            if weak.update(cx, |_, cx| cx.emit(event)).is_err() {
                break;
            }
        }
    });
    cx.set_global(CoreGlobal {
        core: core.clone(),
        events,
        _pump: pump,
    });
    core.start();
    core
}

/// A core for tests: nothing starts in the background, and its events go nowhere. Core's own
/// threads (its repo watcher) would wake the test's deterministic executor whenever the disk
/// settled, failing it at random under load. A test that needs a refresh calls
/// `Project::repo_changed`, as the watcher's news would.
pub fn install_core(cx: &mut App) -> Arc<Core> {
    let core = Arc::new(Core::new(Arc::new(|_| {})));
    let events = cx.new(|_| CoreEvents);
    cx.set_global(CoreGlobal {
        core: core.clone(),
        events,
        _pump: Task::ready(()),
    });
    core
}

/// Which repo core has open, changed one `open_repo` / `close_repo` at a time: a request made
/// while one runs waits, and of the waiting ones only the newest runs. A started open can't be
/// stopped (it's blocking git), so without this A→B→C quickly could leave core on B.
#[derive(Default)]
struct RepoSession {
    generation: u64,
    /// The newest request not started yet: open this root, or close (None).
    waiting: Option<Option<PathBuf>>,
    running: bool,
    answers: Vec<(u64, oneshot::Sender<Option<Res<OpenedRepo>>>)>,
    worker: Option<Task<()>>,
}

impl Global for RepoSession {}

/// Asks core to open the repo at `root`, after whatever it's doing. The answer is None when a
/// newer request overtook this one; core then follows that one instead.
pub fn open_repo(cx: &mut App, root: PathBuf) -> oneshot::Receiver<Option<Res<OpenedRepo>>> {
    let (tx, rx) = oneshot::channel();
    let generation = request(cx, Some(root));
    cx.global_mut::<RepoSession>()
        .answers
        .push((generation, tx));
    rx
}

/// Asks core to close its repo: its watcher stops. Overtakes any open still waiting.
pub fn close_repo(cx: &mut App) {
    request(cx, None);
}

fn request(cx: &mut App, want: Option<PathBuf>) -> u64 {
    let session = cx.default_global::<RepoSession>();
    session.generation += 1;
    session.waiting = Some(want);
    let generation = session.generation;
    if !session.running {
        session.running = true;
        let worker = cx.spawn(async move |cx| work(cx).await);
        cx.global_mut::<RepoSession>().worker = Some(worker);
    }
    generation
}

async fn work(cx: &mut gpui_kit::AsyncApp) {
    loop {
        let next = cx.update(|cx| {
            let session = cx.global_mut::<RepoSession>();
            match session.waiting.take() {
                Some(want) => Some((session.generation, want, core(cx))),
                None => {
                    session.running = false;
                    None
                }
            }
        });
        let Some((generation, want, core)) = next else {
            break;
        };
        let opened = cx
            .background_spawn(async move {
                match want {
                    Some(root) => Some(api::repo::open_repo(
                        &core,
                        root.to_string_lossy().into_owned(),
                    )),
                    None => api::repo::close_repo(&core).err().map(Err),
                }
            })
            .await;
        cx.update(|cx| {
            let session = cx.global_mut::<RepoSession>();
            // Something newer asked meanwhile: this one's answer is that it was overtaken.
            let latest = session.generation == generation;
            let mut opened = Some(opened);
            for (asked, tx) in std::mem::take(&mut session.answers) {
                if asked > generation {
                    session.answers.push((asked, tx));
                } else if asked == generation && latest {
                    let _ = tx.send(opened.take().flatten());
                } else {
                    let _ = tx.send(None);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;

    fn git_repo(name: &str) -> Option<PathBuf> {
        let dir = crate::test_support::temp_dir(&format!("session-{name}"));
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(&dir)
            .status()
            .ok()?
            .success();
        ok.then(|| dir.canonicalize().unwrap_or(dir))
    }

    #[gpui_kit::test]
    fn quick_opens_leave_core_on_the_last_one(cx: &mut TestAppContext) {
        let (Some(a), Some(b), Some(c)) = (git_repo("a"), git_repo("b"), git_repo("c")) else {
            return;
        };
        let core = cx.update(install_core);
        let answers: Vec<_> =
            cx.update(|cx| [&a, &b, &c].map(|root| open_repo(cx, root.clone())).into());
        cx.run_until_parked();
        let answers: Vec<Option<Res<OpenedRepo>>> = answers
            .into_iter()
            .map(|mut rx| rx.try_recv().unwrap().unwrap())
            .collect();
        assert!(answers[0].is_none() && answers[1].is_none());
        let last = answers[2].as_ref().unwrap().as_ref().unwrap();
        assert_eq!(PathBuf::from(&last.root), c);
        assert_eq!(core.repo().unwrap(), c);
    }

    #[gpui_kit::test]
    fn a_close_after_an_open_wins(cx: &mut TestAppContext) {
        let Some(a) = git_repo("close") else { return };
        let core = cx.update(install_core);
        let mut first = cx.update(|cx| open_repo(cx, a.clone()));
        cx.run_until_parked();
        assert!(first.try_recv().unwrap().unwrap().is_some());
        assert_eq!(core.repo().unwrap(), a);
        let mut second = cx.update(|cx| {
            let rx = open_repo(cx, a.clone());
            close_repo(cx);
            rx
        });
        cx.run_until_parked();
        assert!(second.try_recv().unwrap().unwrap().is_none());
        assert!(core.repo().is_err());
    }
}
