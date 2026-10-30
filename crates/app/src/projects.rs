//! The saved projects, as the welcome screen and the left panel list them (Settings → Appearance
//! → Projects in the sidebar), in the top bar picker's arrangement with its folders open: a click
//! or ↵ opens one, a drag reorders, hover actions and a right-click menu (also ⇧F10) do the
//! rest, ⌫ removes one. A folder that was moved sinks below
//! the others and offers Locate….

use std::collections::HashMap;
use std::path::PathBuf;

use gitviber_core::api;
use gitviber_ui::actions::{project as project_cmds, terminal as term_cmds};
use gitviber_ui::projects::{DropOn, Place, home_dir, home_relative, project_name};
use gitviber_ui::widgets::{DraggedProject, Ends, list_step, project_tile};
use gitviber_ui::workspace::{Slot, Toast};
use gitviber_ui::{ActiveAppTheme, UiState, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Global,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, PathPromptOptions,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Task, Window, div,
    prelude::FluentBuilder as _, px, rems,
};

use crate::help::{open_url, toast};

#[derive(Clone, Debug, Default, PartialEq)]
struct Info {
    exists: bool,
    github: Option<String>,
}

/// What each saved folder is now, asked when a list of them is drawn: the first time, after the
/// list changed, and after a window came forward, so a moved folder shows up soon. Never while
/// no list shows: each folder is a git call.
#[derive(Default)]
struct Infos {
    by_path: HashMap<String, Info>,
    asked: Vec<String>,
    /// A window came forward since the last ask.
    stale: bool,
    _task: Option<Task<()>>,
}

impl Global for Infos {}

/// A window came forward: a folder moved meanwhile is asked about when a list is drawn next.
pub fn mark_stale(cx: &mut App) {
    cx.default_global::<Infos>().stale = true;
}

/// Asks again about the saved folders when what's known is old, or at once (`force`).
fn refresh(force: bool, cx: &mut App) {
    if !gitviber_ui::has_core(cx) {
        return;
    }
    let list = UiState::get(cx).recent_projects.clone();
    let infos = cx.default_global::<Infos>();
    if !force && !infos.stale && infos.asked == list {
        return;
    }
    infos.stale = false;
    infos.asked = list.clone();
    let core = gitviber_ui::core(cx);
    let asked = cx.background_spawn(async move {
        let info = api::repo::project_info(&core, list.clone()).unwrap_or_default();
        list.into_iter()
            .zip(info)
            .map(|(p, i)| {
                (
                    p,
                    Info {
                        exists: i.exists,
                        github: i.github,
                    },
                )
            })
            .collect::<HashMap<_, _>>()
    });
    let task = cx.spawn(async move |cx| {
        let found = asked.await;
        cx.update(|cx| {
            let infos = cx.default_global::<Infos>();
            if infos.by_path != found {
                infos.by_path = found;
                cx.refresh_windows();
            }
        });
    });
    cx.default_global::<Infos>()._task = Some(task);
}

fn info(path: &str, cx: &App) -> Option<Info> {
    cx.try_global::<Infos>()?.by_path.get(path).cloned()
}

/// The order shown: folders that are gone sink below the rest, each group in the user's order.
pub fn shown_order(list: &[String], gone: impl Fn(&str) -> bool) -> Vec<String> {
    let (here, away): (Vec<&String>, Vec<&String>) = list.iter().partition(|p| !gone(p));
    here.into_iter().chain(away).cloned().collect()
}

fn forget(path: &str, cx: &mut App) {
    UiState::update(cx, |s| s.forget_project(path));
}

fn toggle_pin(path: &str, cx: &mut App) {
    UiState::update(cx, |s| s.project_org.toggle_pin(path));
}

/// In the main window, once git says it's a repository (open_repo).
fn open_project(path: &str, cx: &mut App) {
    crate::open_repo::open(crate::open_repo::Request::folder(path), cx);
}

/// A terminal in a project, the window staying where it is.
fn terminal_in(path: &str, window: &mut Window, cx: &mut App) {
    let Some(ws) = Workspace::for_window(window, cx) else {
        return;
    };
    let Some(panel) = Workspace::ensure_panel(&ws, "terminal", window, cx)
        .and_then(|p| p.view().downcast::<gitviber_terminal::TerminalPanel>().ok())
    else {
        return window.dispatch_action(Box::new(term_cmds::New), cx);
    };
    ws.update(cx, |ws, cx| ws.set_open(Slot::Bottom, true, cx));
    let dir = PathBuf::from(path);
    panel.update(cx, |p, cx| p.open_terminal(dir, None, window, cx));
}

fn reveal(path: &str, cx: &mut App) {
    let (core, path) = (gitviber_ui::core(cx), path.to_string());
    let revealed = cx.background_spawn(async move { api::repo::reveal_project(&core, path) });
    cx.spawn(async move |cx| {
        if let Err(err) = revealed.await {
            cx.update(|cx| toast(Toast::error(err).title("Couldn't show the folder"), cx));
        }
    })
    .detach();
}

fn copy(text: &str, what: &'static str, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
    toast(Toast::success(text.to_string()).title(what), cx);
}

/// Pick the new place of a folder that was moved: it takes the old one's place in the list.
fn locate(path: &str, cx: &mut App) {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Locate".into()),
    });
    let old = path.to_string();
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = picked.await else {
            return;
        };
        let Some(new) = paths.into_iter().next() else {
            return;
        };
        // Keyed as opening it keys it, so a subfolder or a linked worktree doesn't leave the
        // old entry beside a new one for the real root.
        let new = cx
            .background_spawn(async move {
                api::repo::project_key(&new).unwrap_or_else(|| new.to_string_lossy().into_owned())
            })
            .await;
        cx.update(|cx| {
            let (old, put) = (old.clone(), new.clone());
            UiState::update(cx, move |s| s.relocate_project(&old, &put));
            refresh(true, cx);
            open_project(&new, cx);
        });
    })
    .detach();
}

/// The window is in a linked worktree of the open project: its row then opens the main folder,
/// as a row of another project would.
#[derive(Clone, Debug, PartialEq)]
pub struct Away {
    /// The worktree the window is in: the row's New Terminal starts there, as + does.
    root: String,
    folder: String,
    main_branch: Option<String>,
}

/// Set while the window is in `root` and not in the project's `main` folder.
pub fn away_of(root: &str, main: &str, worktrees: &[gitviber_core::git::Worktree]) -> Option<Away> {
    (root != main).then(|| Away {
        root: root.to_string(),
        folder: project_name(root).to_string(),
        main_branch: worktrees
            .iter()
            .find(|w| w.main)
            .and_then(|w| w.branch.clone()),
    })
}

/// Where a row's New Terminal starts: in the worktree the window is in, for the open project's
/// row (`away` is set only on that one), as the terminal's + does.
fn terminal_dir(path: &str, away: Option<&Away>) -> String {
    away.map_or_else(|| path.to_string(), |a| a.root.clone())
}

impl Away {
    fn open_label(&self) -> String {
        match &self.main_branch {
            Some(branch) => format!("Open the main folder, on {branch}"),
            None => "Open the main folder".into(),
        }
    }
}

/// A list's keyboard state, kept while it shows: one focus for the list, the row the keys act
/// on (by path, so a reorder keeps it), and the scroll that brings that row into view.
struct Keys {
    focus: FocusHandle,
    row: Option<String>,
    /// Moved there by a key: the row shows a ring, as a click's doesn't.
    by_key: bool,
    scroll: ScrollHandle,
}

/// The row the keys act on: the one moved to, else the open project's, else the first.
fn key_row(shown: &[String], row: Option<&str>, current: Option<&str>) -> usize {
    let at = |p: Option<&str>| p.and_then(|p| shown.iter().position(|s| s == p));
    at(row).or_else(|| at(current)).unwrap_or(0)
}

impl Keys {
    fn go(&mut self, row: Option<&String>, cx: &mut Context<Self>) {
        self.row = row.cloned();
        self.by_key = true;
        cx.notify();
    }
}

/// ↑/↓, ↵/Space and ⇧F10 on the row the keys act on.
fn key_down(
    keys: &Entity<Keys>,
    shown: &[String],
    current: Option<&str>,
    away: bool,
    e: &KeyDownEvent,
    window: &mut Window,
    cx: &mut App,
) {
    let k = &e.keystroke;
    let at = key_row(shown, keys.read(cx).row.as_deref(), current);
    let Some(path) = shown.get(at) else { return };
    if gitviber_refs::ui::is_menu_key(k) {
        let scroll = keys.read(cx).scroll.clone();
        if let Some(mut row) = scroll.bounds_for_item(at) {
            // Bounds as laid out; the list draws them scrolled.
            row.origin += scroll.offset();
            gitviber_refs::ui::open_row_menu(row, window, cx);
        }
        return cx.stop_propagation();
    }
    let m = k.modifiers;
    if m.platform || m.control || m.alt || m.shift {
        return;
    }
    if let Some(to) = list_step(k, Some(at), shown.len(), Ends::Stop, false) {
        keys.update(cx, |k, cx| k.go(shown.get(to), cx));
        keys.read(cx).scroll.scroll_to_item(to);
        return cx.stop_propagation();
    }
    match k.key.as_str() {
        "enter" | "space" => {
            if info(path, cx).is_some_and(|i| !i.exists) {
                locate(path, cx)
            } else if away || current != Some(path.as_str()) {
                open_project(path, cx)
            }
        }
        _ => return,
    }
    cx.stop_propagation();
}

/// ⌫: removes the keys' row (not the open project), the keys going on to its neighbour.
fn forget_key_row(keys: &Entity<Keys>, shown: &[String], current: Option<&str>, cx: &mut App) {
    let at = key_row(shown, keys.read(cx).row.as_deref(), current);
    let Some(path) = shown.get(at).filter(|p| current != Some(p.as_str())) else {
        return;
    };
    let next = shown
        .get(at + 1)
        .or_else(|| at.checked_sub(1).and_then(|i| shown.get(i)));
    keys.update(cx, |k, cx| k.go(next, cx));
    forget(path, cx);
}

/// The list. `current`: the open project's main folder, which can't be removed and doesn't open
/// again (unless the window is `away` in another worktree of it); `compact`: a line a row without
/// hover actions, for the narrow sidebar; `max_h`: taller than that, it scrolls.
pub fn list(
    id: &'static str,
    current: Option<String>,
    away: Option<Away>,
    compact: bool,
    max_h: Option<f32>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    refresh(false, cx);
    let keys = window.use_keyed_state(SharedString::from(format!("{id}-keys")), cx, |_, cx| Keys {
        focus: cx.focus_handle().tab_stop(true),
        row: None,
        by_key: false,
        scroll: ScrollHandle::new(),
    });
    let cx: &App = cx;
    let (focus, scroll) = {
        let k = keys.read(cx);
        (k.focus.clone(), k.scroll.clone())
    };
    let focused = focus.is_focused(window);
    let state = UiState::get(cx);
    let recent = state.recent_projects.clone();
    let org = state.project_org.clone();
    let gone = |p: &str| info(p, cx).is_some_and(|i| !i.exists);
    let shown = shown_order(&org.flat(&recent), gone);
    let key_at = focused.then(|| key_row(&shown, keys.read(cx).row.as_deref(), current.as_deref()));
    let ring = keys.read(cx).by_key;
    let home = home_dir();
    let t = cx.app_theme().clone();
    let rows = shown.clone().into_iter().enumerate().map(|(i, path)| {
        let keyed = key_at == Some(i);
        let name = project_name(&path).to_string();
        let i_info = info(&path, cx);
        // Unknown until project_info answers: taken as there rather than flash every row dimmed.
        let missing = i_info.as_ref().is_some_and(|i| !i.exists);
        let github = i_info.and_then(|i| i.github);
        let is_current = current.as_deref() == Some(path.as_str());
        let away = away.clone().filter(|_| is_current);
        // Opening the open project does nothing, unless the window is in one of its worktrees.
        let here = is_current && away.is_none();
        let detail = match &away {
            _ if missing => format!("Not found · {}", home_relative(&path, home.as_deref())),
            Some(a) => format!("In worktree {}", a.folder),
            None => home_relative(&path, home.as_deref()),
        };
        let open_label: SharedString = away
            .as_ref()
            .map_or_else(|| "Open".into(), |a| a.open_label().into());
        let (open, drop_to, clicked) = (path.clone(), path.clone(), path.clone());
        let keys_c = keys.downgrade();
        let pinned = org.place_of(&path) == Place::Pinned;
        let actions = (!compact).then(|| {
            let mut buttons: Vec<AnyElement> = Vec::new();
            let action = |id: &str, icon: IconName, tip: &'static str| {
                Button::new(SharedString::from(format!("{id}-{i}")))
                    .ghost()
                    .xsmall()
                    .icon(icon)
                    .tooltip(tip)
            };
            if missing {
                let p = path.clone();
                buttons.push(
                    action("locate", IconName::FolderSearch, "Locate…")
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            locate(&p, cx)
                        })
                        .into_any_element(),
                );
            } else {
                let (r, c) = (path.clone(), path.clone());
                buttons.push(
                    action(
                        "reveal",
                        IconName::FolderSearch,
                        crate::menus::reveal_label(),
                    )
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        reveal(&r, cx)
                    })
                    .into_any_element(),
                );
                buttons.push(
                    action("copy", IconName::Copy, "Copy path")
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            copy(&c, "Path copied", cx)
                        })
                        .into_any_element(),
                );
            }
            if !is_current {
                let f = path.clone();
                buttons.push(
                    action("forget", IconName::X, "Remove from list")
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            forget(&f, cx)
                        })
                        .into_any_element(),
                );
            }
            div()
                .flex()
                .flex_none()
                .items_center()
                .invisible()
                .group_hover("project-row", |s| s.visible())
                .children(buttons)
        });
        div()
            .id(SharedString::from(format!("{id}-{i}")))
            .group("project-row")
            .flex()
            .items_center()
            .gap(px(if compact { 8. } else { 10. }))
            .h(px(if compact { 28. } else { 36. }))
            .px_2()
            .rounded(px(3.))
            .border_1()
            .border_color(if keyed && ring {
                t.ring
            } else {
                gpui_kit::transparent_black()
            })
            .when(is_current, |d| d.bg(t.active))
            .when(!here, |d| d.cursor_pointer())
            .when(!is_current, |d| d.hover(|s| s.bg(t.hover)))
            .when(keyed && !here, |d| d.bg(t.hover))
            .child(div().when(missing, |d| d.opacity(0.45)).child(project_tile(
                &path,
                if compact { 16. } else { 24. },
                cx,
            )))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(rems(0.78125))
                            .font_weight(t.medium())
                            .when(missing, |d| d.text_color(t.muted_foreground))
                            .child(name.clone()),
                    )
                    .when(!compact, |d| {
                        // The middle goes: a path's start says where, its end which one.
                        d.child(
                            div()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis_middle()
                                .text_size(rems(0.65625))
                                .text_color(if missing { t.conflict } else { t.subtle })
                                .child(detail.clone()),
                        )
                    }),
            )
            .when(is_current, |d| {
                d.child(Icon::new(IconName::Check).small().text_color(t.primary))
            })
            .children(actions)
            .when(compact, |d| {
                let detail = detail.clone();
                d.tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(detail.clone()).build(window, cx)
                })
            })
            .on_click(move |_, _, cx| {
                let _ = keys_c.update(cx, |k, cx| {
                    k.row = Some(clicked.clone());
                    k.by_key = false;
                    cx.notify();
                });
                if missing {
                    locate(&open, cx)
                } else if !here {
                    open_project(&open, cx)
                }
            })
            .on_drag(DraggedProject { path: path.clone() }, |d, _, _, cx| {
                cx.new(|_| d.clone())
            })
            .drag_over::<DraggedProject>(move |s, _, _, cx| s.bg(cx.app_theme().active))
            .on_drop(move |dragged: &DraggedProject, _, cx| {
                let to = drop_to.clone();
                let from = dragged.path.clone();
                UiState::update(cx, move |s| {
                    let recent = s.recent_projects.clone();
                    s.project_org.drop_on(&from, DropOn::Before(to), &recent)
                });
            })
            .context_menu(move |menu, _, _| {
                let p = path.clone();
                let mut m = menu;
                if missing {
                    let l = p.clone();
                    m = m.item(
                        PopupMenuItem::new("Locate…")
                            .icon(IconName::FolderSearch)
                            .on_click(move |_, _, cx| locate(&l, cx)),
                    );
                } else {
                    let tm = terminal_dir(&p, away.as_ref());
                    let (o, r) = (p.clone(), p.clone());
                    m = m
                        .item(
                            PopupMenuItem::new(open_label.clone())
                                .icon(IconName::FolderOpen)
                                .disabled(here)
                                .on_click(move |_, _, cx| open_project(&o, cx)),
                        )
                        .item(
                            PopupMenuItem::new("New Terminal")
                                .icon(IconName::SquareTerminal)
                                .on_click(move |_, window, cx| terminal_in(&tm, window, cx)),
                        )
                        .item(
                            PopupMenuItem::new(crate::menus::reveal_label())
                                .icon(IconName::FolderSearch)
                                .on_click(move |_, _, cx| reveal(&r, cx)),
                        );
                    if let Some(repo) = github.clone() {
                        m = m.item(
                            PopupMenuItem::new("Open on GitHub")
                                .icon(IconName::ExternalLink)
                                .on_click(move |_, _, cx| {
                                    open_url(format!("https://github.com/{repo}"), cx)
                                }),
                        );
                    }
                }
                let (cp, cn, top, f) = (p.clone(), p.clone(), p.clone(), p.clone());
                m.separator()
                    .item(
                        PopupMenuItem::new("Copy Path")
                            .icon(IconName::Copy)
                            .on_click(move |_, _, cx| copy(&cp, "Path copied", cx)),
                    )
                    .item(
                        PopupMenuItem::new("Copy Name")
                            .icon(IconName::Copy)
                            .on_click(move |_, _, cx| copy(project_name(&cn), "Name copied", cx)),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new(if pinned { "Unpin" } else { "Pin" })
                            .icon(if pinned {
                                IconName::PinOff
                            } else {
                                IconName::Pin
                            })
                            .on_click(move |_, _, cx| toggle_pin(&top, cx)),
                    )
                    .item(
                        PopupMenuItem::new("Remove from List")
                            .icon(IconName::X)
                            .disabled(is_current)
                            .on_click(move |_, _, cx| forget(&f, cx)),
                    )
            })
            .into_any_element()
    });
    let is_away = away.is_some();
    let (keys_k, keys_f) = (keys.downgrade(), keys.downgrade());
    let (shown_f, current_f, current_k) = (shown.clone(), current.clone(), current.clone());
    div()
        .id(id)
        .key_context("ProjectList")
        .track_focus(&focus)
        .track_scroll(&scroll)
        .overflow_y_scroll()
        .when_some(max_h, |d, h| d.max_h(px(h)))
        .flex()
        .flex_col()
        .gap_px()
        .w_full()
        .on_key_down(move |e: &KeyDownEvent, window, cx| {
            if let Some(keys) = keys_k.upgrade() {
                key_down(&keys, &shown, current_k.as_deref(), is_away, e, window, cx);
            }
        })
        .on_action(move |_: &project_cmds::Forget, _, cx| {
            if let Some(keys) = keys_f.upgrade() {
                forget_key_row(&keys, &shown_f, current_f.as_deref(), cx);
            }
        })
        .children(rows)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gone_folders_sink() {
        let list: Vec<String> = ["/a", "/b", "/c"].map(String::from).into();
        assert_eq!(shown_order(&list, |p| p == "/a"), ["/b", "/c", "/a"]);
    }

    #[test]
    fn away_names_the_worktree_and_the_main_folders_branch() {
        let main = gitviber_core::git::Worktree {
            path: "/r/app".into(),
            head: None,
            branch: Some("main".into()),
            detached: false,
            bare: false,
            locked: false,
            lock_reason: None,
            in_use: false,
            prunable: false,
            current: false,
            main: true,
        };
        assert_eq!(
            away_of("/r/app", "/r/app", std::slice::from_ref(&main)),
            None
        );
        let away = away_of("/r/app-fix", "/r/app", &[main]).unwrap();
        assert_eq!(away.folder, "app-fix");
        assert_eq!(away.open_label(), "Open the main folder, on main");
        // #110: the open project's New Terminal starts in the linked worktree, others' in theirs.
        assert_eq!(terminal_dir("/r/app", Some(&away)), "/r/app-fix");
        assert_eq!(terminal_dir("/r/other", None), "/r/other");
        assert_eq!(
            away_of("/r/x", "/r/app", &[]).unwrap().open_label(),
            "Open the main folder"
        );
    }

    #[test]
    fn keys_act_on_the_row_moved_to_else_the_open_project() {
        let shown: Vec<String> = ["/a", "/b", "/c"].map(String::from).into();
        assert_eq!(key_row(&shown, Some("/c"), Some("/b")), 2);
        assert_eq!(key_row(&shown, None, Some("/b")), 1);
        // A row removed meanwhile: the open project's, else the first.
        assert_eq!(key_row(&shown, Some("/gone"), None), 0);
        assert_eq!(key_row(&[], None, None), 0);
    }
}
