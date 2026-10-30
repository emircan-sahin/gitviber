//! History's first page, read in the background when a repository opens, so the panel's first
//! show draws commits rather than "Loading…" while git runs. Only HEAD's history (the panel's
//! default); the panel reads it again as it always does, and a git change in between drops the
//! page rather than showing a stale one.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gitviber_core::git;
use gitviber_ui::workspace::{Project, ProjectEvent};
use gpui_kit::{App, AppContext as _, Global, Task};

use crate::list::{PAGE, Source};

struct Page {
    log: Option<Vec<git::Commit>>,
    _read: Task<()>,
}

#[derive(Default)]
struct FirstPages {
    pages: HashMap<PathBuf, Page>,
    /// Roots that had a History panel: theirs reads its own (kept for the app's life; a root
    /// reopened later only misses the head start).
    shown: HashSet<PathBuf>,
}

impl Global for FirstPages {}

pub(crate) fn init(cx: &mut App) {
    cx.observe_new(|_: &mut Project, _, cx| {
        cx.subscribe_self(|project, event: &ProjectEvent, cx| match event {
            ProjectEvent::Opened | ProjectEvent::Switched { .. } if project.is_repo() => {
                warm(project.root().to_path_buf(), cx)
            }
            ProjectEvent::RepoChanged(change) if change.git && cx.has_global::<FirstPages>() => {
                cx.global_mut::<FirstPages>().pages.remove(project.root());
            }
            _ => {}
        })
        .detach();
    })
    .detach();
}

fn warm(root: PathBuf, cx: &mut App) {
    if crate::store::get(cx).all_branches || cx.default_global::<FirstPages>().shown.contains(&root)
    {
        return;
    }
    let core = gitviber_ui::core(cx);
    let key = root.clone();
    let read = cx.spawn(async move |cx| {
        let log = cx
            .background_spawn(async move { Source::Head.read(&core, 0, PAGE) })
            .await;
        cx.update_global::<FirstPages, _>(|pages, _| {
            if let Some(page) = pages.pages.get_mut(&key) {
                page.log = log.ok();
            }
        });
    });
    cx.default_global::<FirstPages>().pages.insert(
        root,
        Page {
            log: None,
            _read: read,
        },
    );
}

/// The page read ahead for `root`, if it's in; from now on its panel reads its own.
pub(crate) fn take(root: &Path, cx: &mut App) -> Option<Vec<git::Commit>> {
    let pages = cx.default_global::<FirstPages>();
    pages.shown.insert(root.to_path_buf());
    pages.pages.remove(root)?.log
}
