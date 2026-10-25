//! What shows where a feature crate hasn't registered yet: the panels by their ids, and the top
//! bar's chips. Each gives way to the real one registered under its id.

use std::path::{Path, PathBuf};

use gitviber_ui::ActiveAppTheme;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{
    BarItem, BarSlot, Panel, PanelContext, PanelSpec, Slot, register_bar_item_default,
    register_panel_default,
};
use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Global, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Task, Window, div, prelude::*, px, rems,
};

pub struct Placeholder {
    _live: Live,
    icon: IconName,
    title: SharedString,
    note: SharedString,
}

impl Panel for Placeholder {}

impl Render for Placeholder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .p_4()
            .child(Icon::new(self.icon).large().text_color(t.subtle))
            .child(
                div()
                    .text_color(t.muted_foreground)
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .text_color(t.subtle)
                    .text_size(rems(0.6875))
                    .text_center()
                    .child(self.note.clone()),
            )
    }
}

fn spec(
    id: &'static str,
    slot: Slot,
    title: &'static str,
    order: i32,
    icon: IconName,
    note: &'static str,
) -> PanelSpec {
    let spec = PanelSpec::new(id, slot, title, move |_, _, cx| {
        cx.new(|_| Placeholder {
            _live: Live::new("Placeholder"),
            icon,
            title: title.into(),
            note: note.into(),
        })
    })
    .order(order);
    if slot == Slot::Bottom {
        spec.per_window()
    } else {
        spec
    }
}

/// The branch HEAD names, read straight from the repo until gitviber-core reports it.
fn head_branch(root: &Path) -> Option<String> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_file() {
        let text = std::fs::read_to_string(&dot_git).ok()?;
        root.join(text.strip_prefix("gitdir:")?.trim())
    } else {
        dot_git
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    match head.trim().strip_prefix("ref: refs/heads/") {
        Some(branch) => Some(branch.to_string()),
        None => Some(head.trim().chars().take(7).collect()),
    }
}

/// A top bar chip: icon, text, the switcher's chevrons.
pub fn chip(
    id: &'static str,
    icon: Option<AnyElement>,
    label: SharedString,
    extra: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    let t = cx.app_theme();
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(26.))
        .px(px(8.))
        .rounded(px(4.))
        .hover(|s| s.bg(t.hover))
        .cursor_pointer()
        .children(icon)
        .child(
            div()
                .font_weight(t.semibold())
                .text_size(rems(0.8125))
                .child(label),
        )
        .children(extra)
        .child(
            Icon::new(IconName::ChevronsUpDown)
                .xsmall()
                .text_color(t.subtle),
        )
        .into_any_element()
}

fn project_chip(ctx: &PanelContext, _: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let project = ctx.project.as_ref()?;
    let name = project.read(cx).name().clone();
    let t = cx.app_theme();
    let letter = name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .collect::<String>();
    let tile = div()
        .size(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .bg(t.renamed_fill)
        .text_color(t.on_status)
        .text_size(rems(0.6875))
        .font_weight(gpui_kit::FontWeight::BOLD)
        .child(letter)
        .into_any_element();
    Some(chip("chip-project", Some(tile), name, None, cx))
}

/// The branch the chip shows, read off the UI thread once per repo change.
#[derive(Default)]
struct BranchCache {
    /// The root and revision it was read at, and what it said.
    read: Option<(PathBuf, u64, Option<String>)>,
    reading: Option<(PathBuf, u64)>,
    _task: Option<Task<()>>,
}

impl Global for BranchCache {}

fn cached_branch(root: &Path, revision: u64, cx: &mut App) -> Option<String> {
    let cache = cx.default_global::<BranchCache>();
    let fresh = cache
        .read
        .as_ref()
        .is_some_and(|(r, rev, _)| r == root && *rev == revision);
    let asked = cache
        .reading
        .as_ref()
        .is_some_and(|(r, rev)| r == root && *rev == revision);
    if !fresh && !asked {
        let path = root.to_path_buf();
        cache.reading = Some((path.clone(), revision));
        let task = cx.spawn(async move |cx| {
            let read_at = path.clone();
            let branch = cx
                .background_spawn(async move { head_branch(&read_at) })
                .await;
            cx.update(|cx| {
                let cache = cx.global_mut::<BranchCache>();
                cache.read = Some((path, revision, branch));
                cache.reading = None;
                cx.refresh_windows();
            });
        });
        cx.global_mut::<BranchCache>()._task = Some(task);
    }
    // While a newer read runs, the last one of this root stands in.
    cx.global::<BranchCache>()
        .read
        .as_ref()
        .filter(|(r, _, _)| r == root)
        .and_then(|(_, _, b)| b.clone())
}

fn branch_chip(ctx: &PanelContext, _: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let (root, revision) = ctx.project.as_ref().map(|p| {
        let p = p.read(cx);
        (p.root().to_path_buf(), p.revision())
    })?;
    let branch = cached_branch(&root, revision, cx)?;
    let t = cx.app_theme();
    let icon = Icon::new(IconName::GitBranch)
        .small()
        .text_color(t.muted_foreground)
        .into_any_element();
    let slash = div().text_color(t.subtle).child("/").into_any_element();
    Some(
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(slash)
            .child(chip("chip-branch", Some(icon), branch.into(), None, cx))
            .into_any_element(),
    )
}

pub fn register(cx: &mut App) {
    for s in [
        spec(
            "changes",
            Slot::Left,
            "Changes",
            0,
            IconName::GitCompare,
            "The working tree's changes come with gitviber-changes.",
        ),
        spec(
            "history",
            Slot::Left,
            "History",
            1,
            IconName::GitCommitHorizontal,
            "Commits and branches come with gitviber-history.",
        ),
        spec(
            "pulls",
            Slot::Left,
            "PRs",
            2,
            IconName::GitPullRequest,
            "Pull requests come with gitviber-github.",
        ),
        spec(
            "issues",
            Slot::Left,
            "Issues",
            3,
            IconName::CircleDot,
            "Issues come with gitviber-github.",
        ),
        spec(
            "explorer",
            Slot::Right,
            "Explorer",
            0,
            IconName::FolderTree,
            "The file tree comes with gitviber-files.",
        ),
        spec(
            "search",
            Slot::Right,
            "Search",
            1,
            IconName::Search,
            "Find in files comes with gitviber-files.",
        ),
        spec(
            "terminal",
            Slot::Bottom,
            "Terminal",
            0,
            IconName::SquareTerminal,
            "Terminals come with gitviber-terminal.",
        ),
    ] {
        register_panel_default(cx, s);
    }
    register_bar_item_default(
        cx,
        BarItem::new("project", BarSlot::TopChips, project_chip).order(0),
    );
    register_bar_item_default(
        cx,
        BarItem::new("branch", BarSlot::TopChips, branch_chip).order(1),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_branch_of_this_repo_or_nothing() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        // A detached HEAD reads as its short hash; either way there's something.
        assert!(head_branch(&here).is_some_and(|b| !b.is_empty()));
        assert!(head_branch(Path::new("/nonexistent")).is_none());
    }
}
