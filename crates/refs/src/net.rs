//! The network commands running (fetch, pull, push, a remote tag's push), app-wide: the top bar
//! shows the newest with git's progress and Cancel; when it ends, the one still running before it
//! comes back. Progress comes from core's thread over a channel and lands here on the UI thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gitviber_core::network::{LOCAL_PHASE, Progress};
use gpui_kit::{App, AppContext as _, Context, Entity, Global, SharedString, Task};

/// Why a pinned action stopped: core had another worktree open by then.
pub const STOPPED: &str =
    "Another worktree was opened while this ran, so it stopped before doing anything there.";

static NEXT_OP: AtomicU64 = AtomicU64::new(1);

/// A network command's id for core (`watch_network`, `cancel_network`), and where its progress goes.
#[derive(Clone)]
pub struct NetOp {
    pub id: String,
    sink: Arc<dyn Fn(Progress) + Send + Sync>,
    /// The worktree the action started in, when it was pinned to one.
    root: Option<std::path::PathBuf>,
    /// The kinds of progress reported so far: TRANSFER, LOCAL.
    seen: Arc<AtomicU8>,
}

const TRANSFER: u8 = 1;
const LOCAL: u8 = 2;

impl NetOp {
    /// One that reports nowhere: for a command nothing shows.
    pub fn quiet() -> Self {
        NetOp {
            id: format!("net-{}", NEXT_OP.fetch_add(1, Ordering::Relaxed)),
            sink: Arc::new(|_| {}),
            root: None,
            seen: Arc::default(),
        }
    }

    /// Pinned to the worktree `root`: `still_in` refuses once core has another open.
    pub fn pinned(mut self, root: std::path::PathBuf) -> Self {
        self.root = Some(root);
        self
    }

    /// Core still has the worktree open that the action started in: each step of one that runs
    /// several (Sync's pull, then push) asks before it goes on.
    pub fn still_in(&self, core: &gitviber_core::Core) -> gitviber_core::Res<()> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        match core.repo() {
            Ok(open) if &open == root => Ok(()),
            _ => Err(STOPPED.into()),
        }
    }

    /// What core's network commands take as their progress callback.
    pub fn progress(&self) -> impl Fn(Progress) + Send + Sync + 'static {
        let (sink, seen) = (self.sink.clone(), self.seen.clone());
        move |p| {
            let kind = if p.cancellable {
                TRANSFER
            } else if p.phase == LOCAL_PHASE {
                LOCAL
            } else {
                0
            };
            seen.fetch_or(kind, Ordering::Relaxed);
            sink(p)
        }
    }

    /// It only changed local files: a switch or checkout in a repository without Git LFS, or
    /// with every file already downloaded.
    pub fn local_only(&self) -> bool {
        self.seen.load(Ordering::Relaxed) == LOCAL
    }
}

pub struct Running {
    pub id: String,
    pub label: SharedString,
    pub progress: Option<Progress>,
    _pump: Task<()>,
}

/// Every network command running, oldest first.
#[derive(Default)]
pub struct NetActivity {
    running: Vec<Running>,
}

struct Activity(Entity<NetActivity>);
impl Global for Activity {}

impl NetActivity {
    pub fn global(cx: &mut App) -> Entity<NetActivity> {
        if let Some(a) = cx.try_global::<Activity>() {
            return a.0.clone();
        }
        let entity = cx.new(|_| NetActivity::default());
        cx.set_global(Activity(entity.clone()));
        entity
    }

    /// The newest command, which the top bar shows.
    pub fn current(&self) -> Option<&Running> {
        self.running.last()
    }

    pub fn is_running(&self) -> bool {
        !self.running.is_empty()
    }

    /// Starts showing a command; its progress arrives through the returned op until `finish`.
    pub fn start(&mut self, label: SharedString, cx: &mut Context<Self>) -> NetOp {
        let id = format!("net-{}", NEXT_OP.fetch_add(1, Ordering::Relaxed));
        let (tx, mut rx) = mpsc::unbounded::<Progress>();
        let sink: Arc<dyn Fn(Progress) + Send + Sync> = Arc::new(move |p| {
            let _ = tx.unbounded_send(p);
        });
        let op_id = id.clone();
        let pump = cx.spawn(async move |this, cx| {
            while let Some(p) = rx.next().await {
                let ok = this.update(cx, |this, cx| {
                    if let Some(r) = this.running.iter_mut().find(|r| r.id == op_id) {
                        r.progress = Some(p);
                        cx.notify();
                    }
                });
                if ok.is_err() {
                    break;
                }
            }
        });
        self.running.push(Running {
            id: id.clone(),
            label,
            progress: None,
            _pump: pump,
        });
        cx.notify();
        NetOp {
            id,
            sink,
            root: None,
            seen: Arc::default(),
        }
    }

    pub fn finish(&mut self, id: &str, cx: &mut Context<Self>) {
        self.running.retain(|r| r.id != id);
        cx.notify();
    }

    /// Stops the newest command; it then fails with core's CANCELLED, which reads as neutral.
    pub fn cancel_current(&self, cx: &App) {
        if let Some(r) = self.current() {
            let (core, id) = (gitviber_ui::core(cx), r.id.clone());
            cx.background_executor()
                .spawn(async move { gitviber_core::api::sync::cancel_network(&core, id) })
                .detach();
        }
    }
}

/// "Receiving objects 45%", what the top bar shows of a command's progress.
pub fn progress_text(p: &Progress) -> String {
    match p.percent {
        Some(n) => format!("{} {n}%", p.phase),
        None => p.phase.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitviber_core::network::parse_progress;

    #[test]
    fn a_checkout_without_a_download_is_local_only() {
        let local = || parse_progress("Updating files: 40% (4/10)").unwrap();
        let op = NetOp::quiet();
        let report = op.progress();
        assert!(!op.local_only());
        report(local());
        assert!(op.local_only());

        let op = NetOp::quiet();
        let report = op.progress();
        report(parse_progress("Downloading LFS objects: 5% (1/20), 2 MB").unwrap());
        report(local());
        assert!(!op.local_only());
    }
}
