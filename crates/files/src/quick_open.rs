//! Quick open's files (⌘P): every file git lists in the project, read in the background when the
//! repo opens and again, lazily, after it changed. The palette gets what was read last and never
//! waits on git: before the first read lands it gets none, [`status`] says why, and
//! [`QuickOpenFiles`] changes (`observe_global`) when a read lands. A failed read is kept until
//! the repo changes, so ⌘P doesn't run git again each time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gitviber_core::api;
use gitviber_ui::workspace::{PanelContext, QuickItem, register_quick_open};
use gpui_kit::{App, AppContext as _, Global, SharedString, Task};

use crate::{OpenOptions, backend, open_file};

/// Projects whose lists are kept; a window rarely switches between more.
const KEEP: usize = 4;

struct Listing {
    files: Arc<Vec<SharedString>>,
    /// git's words when the last read failed; `files` are then the last ones read, if any.
    error: Option<SharedString>,
    /// The project's revision the list was read at.
    revision: u64,
    used: u64,
    _task: Option<Task<()>>,
}

/// The projects' file lists; observe it to hear when one is read.
#[derive(Default)]
pub struct QuickOpenFiles {
    lists: HashMap<PathBuf, Listing>,
    clock: u64,
}

impl Global for QuickOpenFiles {}

/// Where a project's list stands, for the palette when it has no files to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuickOpenStatus {
    /// Being read; there's no list yet.
    Listing,
    Listed,
    /// git failed; its words.
    Failed(SharedString),
}

/// Reads `root`'s files in the background unless they're there.
pub(crate) fn warm(root: &Path, revision: u64, cx: &mut App) {
    if root.as_os_str().is_empty()
        || cx
            .default_global::<QuickOpenFiles>()
            .lists
            .contains_key(root)
    {
        return;
    }
    refresh(root.to_path_buf(), revision, cx);
}

fn refresh(root: PathBuf, revision: u64, cx: &mut App) {
    let core = backend::core(cx);
    let key = root.clone();
    let task = cx.spawn(async move |cx| {
        let files = cx
            .background_spawn(async move { api::files::list_files(&core) })
            .await;
        cx.update_global::<QuickOpenFiles, _>(|index, _| {
            let Some(l) = index.lists.get_mut(&key) else {
                return;
            };
            match files {
                Ok(files) => {
                    l.files = Arc::new(files.into_iter().map(SharedString::from).collect());
                    l.error = None;
                }
                Err(e) => l.error = Some(e.into()),
            }
            l.revision = revision;
            l._task = None;
        });
    });
    let index = cx.default_global::<QuickOpenFiles>();
    index.clock += 1;
    let used = index.clock;
    let entry = index.lists.entry(root).or_insert_with(|| Listing {
        files: Arc::default(),
        error: None,
        revision,
        used,
        _task: None,
    });
    entry._task = Some(task);
    entry.used = used;
    prune(index);
}

fn prune(index: &mut QuickOpenFiles) {
    while index.lists.len() > KEEP {
        let Some(oldest) = index
            .lists
            .iter()
            .min_by_key(|(_, l)| l.used)
            .map(|(k, _)| k.clone())
        else {
            break;
        };
        index.lists.remove(&oldest);
    }
}

/// The project's files as last read; a first read, or one after the repo changed, starts in the
/// background.
fn files(ctx: &PanelContext, cx: &mut App) -> Arc<Vec<SharedString>> {
    let Some(project) = &ctx.project else {
        return Arc::default();
    };
    let (root, revision, open) = {
        let p = project.read(cx);
        (p.root().to_path_buf(), p.revision(), p.is_open())
    };
    if !open {
        return Arc::default();
    }
    let index = cx.default_global::<QuickOpenFiles>();
    index.clock += 1;
    let clock = index.clock;
    let Some(l) = index.lists.get_mut(&root) else {
        refresh(root, revision, cx);
        return Arc::default();
    };
    l.used = clock;
    let files = l.files.clone();
    if l.revision != revision && l._task.is_none() {
        refresh(root, revision, cx);
    }
    files
}

/// Where `ctx`'s project's list stands.
pub fn status(ctx: &PanelContext, cx: &App) -> QuickOpenStatus {
    let root = ctx.project.as_ref().map(|p| p.read(cx).root());
    let listing = root.and_then(|root| {
        cx.try_global::<QuickOpenFiles>()
            .and_then(|index| index.lists.get(root))
    });
    match listing {
        Some(Listing {
            error: Some(e),
            _task: None,
            ..
        }) => QuickOpenStatus::Failed(e.clone()),
        Some(l) if l._task.is_none() || !l.files.is_empty() => QuickOpenStatus::Listed,
        _ => QuickOpenStatus::Listing,
    }
}

pub(crate) fn register(cx: &mut App) {
    register_quick_open(
        cx,
        Rc::new(|ctx: &PanelContext, cx: &mut App| {
            let files = files(ctx, cx);
            files
                .iter()
                .map(|path| {
                    let workspace = ctx.workspace.clone();
                    let open = path.clone();
                    QuickItem {
                        label: path.clone(),
                        detail: None,
                        is_path: true,
                        run: Rc::new(move |window, cx| {
                            open_file(
                                &workspace,
                                &open,
                                OpenOptions {
                                    preview: false,
                                    ..Default::default()
                                },
                                window,
                                cx,
                            )
                        }),
                    }
                })
                .collect()
        }),
    );
}
