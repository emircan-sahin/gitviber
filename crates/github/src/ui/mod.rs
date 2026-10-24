//! The views: the PRs and Issues panels, a PR's, a PR file's and an issue's tab, their dialogs.

mod common;
mod connect;
mod dialogs;
mod issue_view;
mod issues_panel;
mod labels;
mod list;
mod pull_file;
pub(crate) mod pull_view;
mod pulls_panel;

use std::sync::Arc;

use gitviber_core::git::FileChange;
use gitviber_core::github::{Issue, Label, Pull};
use gitviber_ui::workspace::{
    PanelSpec, PullRange, Selection, Slot, TabHandle, Workspace, register_opener, register_panel,
};
use gpui_kit::{App, AppContext as _, WeakEntity, Window};

pub use issue_view::IssueView;
pub use issues_panel::IssuesPanel;
pub use pull_file::PullFileView;
pub use pull_view::PullView;
pub use pulls_panel::PullsPanel;

use crate::model::{IssueItem, PullItem, PullState};

pub(crate) fn init(cx: &mut App) {
    register_panel(
        cx,
        PanelSpec::new("pulls", Slot::Left, "PRs", |ctx, window, cx| {
            cx.new(|cx| PullsPanel::new(ctx, window, cx))
        })
        .order(2),
    );
    register_panel(
        cx,
        PanelSpec::new("issues", Slot::Left, "Issues", |ctx, window, cx| {
            cx.new(|cx| IssuesPanel::new(ctx, window, cx))
        })
        .order(3),
    );
    register_opener(cx, "pull", |sel, window, cx| {
        let Selection::Pull { pull } = sel else {
            return None;
        };
        let ws = Workspace::for_window(window, cx)?.downgrade();
        let item = PullItem::from(&**pull);
        Some(TabHandle::new(cx.new(|cx| {
            PullView::new(ws, item.url.clone(), Some(item), window, cx)
        })))
    });
    register_opener(cx, "issue", |sel, window, cx| {
        let Selection::Issue { issue } = sel else {
            return None;
        };
        let ws = Workspace::for_window(window, cx)?.downgrade();
        let item = IssueItem::from(&**issue);
        Some(TabHandle::new(cx.new(|cx| {
            IssueView::new(ws, item.url.clone(), Some(item), window, cx)
        })))
    });
    register_opener(cx, "pr-file", |sel, window, cx| {
        let Selection::PrFile { range, file } = sel else {
            return None;
        };
        let ws = Workspace::for_window(window, cx)?.downgrade();
        let (range, file) = (range.clone(), file.clone());
        Some(TabHandle::new(
            cx.new(|cx| PullFileView::new(ws, range, file, window, cx)),
        ))
    });
}

/// core's shape of a list's item, which a selection holds.
pub(crate) fn core_pull(p: &PullItem) -> Pull {
    Pull {
        number: p.number,
        title: p.title.clone(),
        state: match p.state {
            PullState::Open => "open",
            PullState::Closed => "closed",
            PullState::Merged => "merged",
        }
        .into(),
        draft: p.draft,
        author: p.author.clone(),
        head_ref: p.head_ref.clone(),
        head_sha: p.head_sha.clone(),
        head_repo: p.head_repo.clone(),
        base_ref: p.base_ref.clone(),
        base_sha: p.base_sha.clone(),
        created_at: p.created_at.clone(),
        updated_at: p.updated_at.clone(),
        url: p.url.clone(),
    }
}

pub(crate) fn core_issue(i: &IssueItem) -> Issue {
    Issue {
        number: i.number,
        title: i.title.clone(),
        state: if i.open { "open" } else { "closed" }.into(),
        state_reason: i.state_reason.clone(),
        author: i.author.clone(),
        labels: i
            .labels
            .iter()
            .map(|l| Label {
                name: l.name.clone(),
                color: l.color.clone(),
                description: l.description.clone(),
            })
            .collect(),
        assignees: i.assignees.clone(),
        comments: i.comments,
        created_at: i.created_at.clone(),
        updated_at: i.updated_at.clone(),
        url: i.url.clone(),
    }
}

/// The tab key a PR's page has, as `selection_key` names it.
pub(crate) fn pull_key(url: &str) -> String {
    format!("pull:{url}")
}

fn open(
    ws: &WeakEntity<Workspace>,
    selection: Selection,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(ws) = ws.upgrade() {
        ws.update(cx, |ws, cx| ws.open(selection, preview, window, cx));
    }
}

/// Opens a PR's tab, or shows it. `preview`: a single click's, which the next one replaces.
pub fn open_pull(
    ws: &WeakEntity<Workspace>,
    item: PullItem,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) {
    open(
        ws,
        Selection::Pull {
            pull: Arc::new(core_pull(&item)),
        },
        preview,
        window,
        cx,
    );
}

pub fn open_issue(
    ws: &WeakEntity<Workspace>,
    item: IssueItem,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) {
    open(
        ws,
        Selection::Issue {
            issue: Arc::new(core_issue(&item)),
        },
        preview,
        window,
        cx,
    );
}

/// A PR file's diff over `range`; with the PR's url in it, the file takes line comments.
pub fn open_pull_file(
    ws: &WeakEntity<Workspace>,
    range: PullRange,
    file: FileChange,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) {
    open(
        ws,
        Selection::PrFile {
            range,
            file: Arc::new(file),
        },
        preview,
        window,
        cx,
    );
}
