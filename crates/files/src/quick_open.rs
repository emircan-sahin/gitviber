//! Quick open's files (⌘P): every file git lists in the project, read in the background when the
//! explorer first shows and again, lazily, after the repo changed. The palette asks synchronously,
//! so it gets what was read last; a project never read is read there and then.

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
    /// The project's revision the list was read at.
    revision: u64,
    used: u64,
    _task: Option<Task<()>>,
}

#[derive(Default)]
struct Index {
    lists: HashMap<PathBuf, Listing>,
    clock: u64,
}

impl Global for Index {}

fn read(cx: &App) -> Vec<SharedString> {
    let core = backend::core(cx);
    api::files::list_files(&core)
        .unwrap_or_default()
        .into_iter()
        .map(SharedString::from)
        .collect()
}

/// Reads `root`'s files in the background unless they're there.
pub fn warm(root: &Path, cx: &mut App) {
    if root.as_os_str().is_empty() || cx.default_global::<Index>().lists.contains_key(root) {
        return;
    }
    refresh(root.to_path_buf(), 0, cx);
}

fn refresh(root: PathBuf, revision: u64, cx: &mut App) {
    let core = backend::core(cx);
    let key = root.clone();
    let task = cx.spawn(async move |cx| {
        let files = cx
            .background_spawn(async move { api::files::list_files(&core) })
            .await;
        let Ok(files) = files else { return };
        let files: Vec<SharedString> = files.into_iter().map(SharedString::from).collect();
        cx.update(|cx| {
            if let Some(l) = cx.default_global::<Index>().lists.get_mut(&key) {
                l.files = Arc::new(files);
                l.revision = revision;
                l._task = None;
            }
        });
    });
    let index = cx.default_global::<Index>();
    index.clock += 1;
    let used = index.clock;
    let entry = index.lists.entry(root).or_insert_with(|| Listing {
        files: Arc::default(),
        revision,
        used,
        _task: None,
    });
    entry._task = Some(task);
    entry.used = used;
    prune(index);
}

fn prune(index: &mut Index) {
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

/// The project's files now: as last read, or read here the first time.
fn files(ctx: &PanelContext, cx: &mut App) -> Arc<Vec<SharedString>> {
    let Some(project) = &ctx.project else {
        return Arc::default();
    };
    let (root, revision) = {
        let p = project.read(cx);
        (p.root().to_path_buf(), p.revision())
    };
    let index = cx.default_global::<Index>();
    index.clock += 1;
    let clock = index.clock;
    match index.lists.get_mut(&root) {
        Some(l) if !l.files.is_empty() || l._task.is_some() => {
            l.used = clock;
            let files = l.files.clone();
            if l.revision != revision && l._task.is_none() {
                refresh(root, revision, cx);
            }
            if files.is_empty() {
                return Arc::new(read(cx));
            }
            files
        }
        _ => {
            let files = Arc::new(read(cx));
            let index = cx.default_global::<Index>();
            index.lists.insert(
                root,
                Listing {
                    files: files.clone(),
                    revision,
                    used: clock,
                    _task: None,
                },
            );
            prune(index);
            files
        }
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
