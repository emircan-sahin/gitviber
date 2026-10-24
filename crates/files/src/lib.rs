//! GitViber's files: the Explorer and Search panels (right sidebar, ids "explorer" and
//! "search"), quick open's files (⌘P), and the tabs for the "file", "external" and "files"
//! selections: a file in the code editor (markdown and SVG rendered too, media in the media
//! view), a file outside the repo read-only, and two files compared in the diff view. Call
//! [`init`] once at startup.

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

mod backend;
pub mod code;
mod code_map;
mod compare;
pub mod explorer;
pub mod file_view;
pub mod language;
pub mod media;
#[cfg(target_os = "macos")]
pub mod native_media;
mod quick_open;
pub mod search;
mod svg;
pub mod text;

use std::sync::Arc;

use gitviber_core::git::FileChange;
use gitviber_core::grep::Query;
use gitviber_ui::workspace::{
    BarItem, BarSlot, PanelContext, PanelSpec, Selection, Slot, TabHandle, Workspace,
    register_bar_item, register_opener, register_panel,
};
use gpui_kit::{App, AppContext as _, SharedString, WeakEntity, Window};

pub use code::{CodeBlock, CodeEvent, CodeView};
pub use compare::CompareTab;
pub use explorer::ExplorerPanel;
pub use file_view::{FileSource, FileView, set_code_attacher};
pub use media::MediaView;
pub use search::SearchPanel;

/// A line to show once a file is open: a link's at its column (0-based, UTF-16), or a search
/// result's with its match selected.
#[derive(Clone)]
pub enum Reveal {
    Line { line: u32, column: u32 },
    Match { line: u32, query: Query },
}

#[derive(Clone, Default)]
pub struct OpenOptions {
    /// A single click's tab, which the next preview open replaces.
    pub preview: bool,
    pub reveal: Option<Reveal>,
    /// Move the keyboard into the file.
    pub focus: bool,
}

/// Registers the panels, the openers of "file", "external" and "files", quick open's files, the
/// code view's keys and the code fence aliases.
pub fn init(cx: &mut App) {
    language::register_aliases();
    code::bind_keys(cx);
    register_panel(
        cx,
        PanelSpec::new(
            explorer::PANEL_ID,
            Slot::Right,
            "Explorer",
            |ctx, window, cx| cx.new(|cx| ExplorerPanel::new(ctx, window, cx)),
        )
        .order(0),
    );
    register_panel(
        cx,
        PanelSpec::new(
            search::PANEL_ID,
            Slot::Right,
            "Search",
            |ctx, window, cx| cx.new(|cx| SearchPanel::new(ctx, window, cx)),
        )
        .order(1),
    );
    register_opener(cx, "file", open_selection);
    register_opener(cx, "external", open_selection);
    register_opener(cx, "files", open_selection);
    quick_open::register(cx);
    global_commands(cx);
    register_bar_item(
        cx,
        BarItem::new("language", BarSlot::StatusRight, language_item),
    );
    // A diff's Go to Definition into another file, and its preview's file links, land on the line.
    gitviber_diff::set_file_opener(cx, |path, at, window, cx| {
        let Some(ws) = Workspace::for_window(window, cx) else {
            return;
        };
        let options = OpenOptions {
            preview: false,
            reveal: at.map(|(line, column)| Reveal::Line { line, column }),
            focus: true,
        };
        open_file(&ws.downgrade(), path, options, window, cx);
    });
}

/// The status bar's name of the active file's language, while its code shows.
fn language_item(ctx: &PanelContext, _: &mut Window, cx: &mut App) -> Option<gpui_kit::AnyElement> {
    use gpui_kit::{IntoElement as _, ParentElement as _, Styled as _};
    let ws = ctx.workspace.upgrade()?;
    let view = ws.read(cx).tabs().active()?.item.handle()?.view();
    let lang = view
        .downcast::<FileView>()
        .ok()?
        .read(cx)
        .shown_language(cx)?;
    Some(
        gpui_kit::div()
            .px(gpui_kit::px(6.))
            .child(language::language_label(lang).to_string())
            .into_any_element(),
    )
}

/// `f` in the active window with its workspace, after the action that asked for it is done
/// dispatching (the window can't be updated during it).
fn in_active_workspace(
    cx: &mut App,
    f: impl FnOnce(&gpui_kit::Entity<Workspace>, &mut Window, &mut App) + 'static,
) {
    let Some(handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        _ = handle.update(cx, |_, window, cx| {
            if let Some(ws) = Workspace::for_window(window, cx) {
                f(&ws, window, cx);
            }
        });
    });
}

/// The old app's global commands, whatever has focus; a view that handles one itself (the code
/// view's Find in Files, with its selection) runs first.
fn global_commands(cx: &mut App) {
    use gitviber_ui::keymap::{file, search};
    cx.on_action(|_: &search::FindInFiles, cx| {
        in_active_workspace(cx, |ws, window, cx| {
            find_in_files(&ws.downgrade(), None, window, cx)
        });
    });
    // From any tab that shows a file: a diff's, a commit's, the file's own.
    cx.on_action(|_: &file::RevealInExplorer, cx| {
        in_active_workspace(cx, |ws, window, cx| {
            let Some(selection) = active_selection(ws, cx) else {
                return;
            };
            // A file outside the repo isn't in its tree.
            if !matches!(selection, Selection::External { .. }) {
                let path = gitviber_ui::workspace::selection_path(&selection);
                reveal_in_explorer(&ws.downgrade(), &path, window, cx);
            }
        });
    });
    cx.on_action(|_: &file::Reveal, cx| {
        in_active_workspace(cx, |ws, _, cx| {
            let Some(selection) = active_selection(ws, cx) else {
                return;
            };
            let core = backend::core(cx);
            let path = gitviber_ui::workspace::selection_path(&selection);
            let external = matches!(selection, Selection::External { .. });
            cx.background_spawn(async move {
                _ = if external {
                    gitviber_core::api::files::external_reveal(&core, path)
                } else {
                    gitviber_core::api::files::reveal_path(&core, path)
                };
            })
            .detach();
        });
    });
}

/// What the active tab shows, when it's a selection (a file, a diff, a commit's file).
fn active_selection(ws: &gpui_kit::Entity<Workspace>, cx: &App) -> Option<Selection> {
    ws.read(cx).tabs().active()?.item.selection().cloned()
}

fn open_selection(selection: &Selection, window: &mut Window, cx: &mut App) -> Option<TabHandle> {
    // The workspace is being updated (it called this): only its handle is taken here.
    let ws = Workspace::for_window(window, cx)?.downgrade();
    let source = match selection {
        Selection::File { path } => FileSource::Repo(path.clone()),
        Selection::External { path } => FileSource::External(path.clone()),
        Selection::Files { file } => {
            let left = file.old_path.clone()?;
            return Some(TabHandle::new(
                cx.new(|cx| CompareTab::new(left, file.path.clone(), cx)),
            ));
        }
        _ => return None,
    };
    Some(TabHandle::new(
        cx.new(|cx| FileView::new(ws, source, window, cx)),
    ))
}

fn open(
    workspace: &WeakEntity<Workspace>,
    selection: Selection,
    opts: OpenOptions,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(ws) = workspace.upgrade() else {
        return;
    };
    ws.update(cx, |ws, cx| {
        if !ws.open(selection.clone(), opts.preview, window, cx) {
            return;
        }
        let view = ws
            .tab_for(&selection)
            .and_then(|t| t.item.handle())
            .and_then(|h| h.view().downcast::<FileView>().ok());
        if let (Some(view), Some(reveal)) = (view, opts.reveal) {
            view.update(cx, |v, cx| v.reveal(reveal, window, cx));
        }
        if opts.focus {
            ws.focus_center(window, cx);
        }
    });
}

/// Opens (or shows) the tab of a file in the project.
pub fn open_file(
    workspace: &WeakEntity<Workspace>,
    path: &str,
    opts: OpenOptions,
    window: &mut Window,
    cx: &mut App,
) {
    open(
        workspace,
        Selection::File {
            path: path.to_string(),
        },
        opts,
        window,
        cx,
    );
}

/// Opens a file outside the project, read-only.
pub fn open_external(
    workspace: &WeakEntity<Workspace>,
    path: &str,
    opts: OpenOptions,
    window: &mut Window,
    cx: &mut App,
) {
    open(
        workspace,
        Selection::External {
            path: path.to_string(),
        },
        opts,
        window,
        cx,
    );
}

/// The selection comparing two working-tree files: `left` against `right`.
pub fn files_selection(left: &str, right: &str) -> Selection {
    Selection::Files {
        file: Arc::new(FileChange {
            path: right.to_string(),
            old_path: Some(left.to_string()),
            status: "M".into(),
            additions: None,
            deletions: None,
            oid: None,
            index_oid: None,
            conflict: None,
            mode: None,
            submodule: None,
            nested: None,
        }),
    }
}

/// Opens the diff of two working-tree files.
pub fn compare_files(
    workspace: &WeakEntity<Workspace>,
    left: &str,
    right: &str,
    window: &mut Window,
    cx: &mut App,
) {
    open(
        workspace,
        files_selection(left, right),
        OpenOptions::default(),
        window,
        cx,
    );
}

/// The repo-relative path of a file tab's key.
pub fn path_of_key(key: &str) -> Option<&str> {
    key.strip_prefix("file::")
}

/// Shows the Explorer with `path` selected in it.
pub fn reveal_in_explorer(
    workspace: &WeakEntity<Workspace>,
    path: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(ws) = workspace.upgrade() else {
        return;
    };
    let panel = ws.update(cx, |ws, cx| {
        ws.show_panel(explorer::PANEL_ID, cx);
        ws.panel(explorer::PANEL_ID, window, cx)
    });
    if let Some(panel) = panel.and_then(|p| p.view().downcast::<ExplorerPanel>().ok()) {
        panel.update(cx, |p, cx| p.reveal(path, true, window, cx));
    }
}

/// Find in Files: the Search panel shown, its box focused with `seed` in it.
pub fn find_in_files(
    workspace: &WeakEntity<Workspace>,
    seed: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(ws) = workspace.upgrade() else {
        return;
    };
    let panel = ws.update(cx, |ws, cx| {
        ws.show_panel(search::PANEL_ID, cx);
        ws.panel(search::PANEL_ID, window, cx)
    });
    if let Some(panel) = panel.and_then(|p| p.view().downcast::<SearchPanel>().ok()) {
        panel.update(cx, |p, cx| p.ask(seed, window, cx));
    }
}

/// An entry was renamed (`to`) or trashed (None): the tabs of the files in it close; a renamed
/// one that was open opens again under its new name.
pub(crate) fn path_moved(
    workspace: &WeakEntity<Workspace>,
    from: &str,
    to: Option<&str>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(ws) = workspace.upgrade() else {
        return;
    };
    let inside = |p: &str| p == from || p.starts_with(&format!("{from}/"));
    let keys: Vec<SharedString> = ws
        .read(cx)
        .tabs()
        .tabs()
        .iter()
        .filter(|t| path_of_key(&t.key).is_some_and(inside))
        .map(|t| t.key.clone())
        .collect();
    if keys.is_empty() {
        return;
    }
    let moved: Vec<String> = match to {
        Some(to) => keys
            .iter()
            .filter_map(|k| path_of_key(k))
            .map(|p| format!("{to}{}", &p[from.len()..]))
            .collect(),
        None => vec![],
    };
    ws.update(cx, |ws, cx| ws.close_tabs(&keys, cx));
    for path in moved {
        open_file(workspace, &path, OpenOptions::default(), window, cx);
    }
}

#[cfg(test)]
mod tests;
