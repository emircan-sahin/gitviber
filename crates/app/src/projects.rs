//! The saved projects, as the welcome screen and the left panel list them (Settings → Appearance
//! → Projects in the sidebar): a click opens one, a drag reorders, hover actions and a right-click
//! menu do the rest. A folder that was moved sinks below the others and offers Locate….

use std::collections::HashMap;
use std::path::PathBuf;

use gitviber_core::api;
use gitviber_ui::actions::terminal as term_cmds;
use gitviber_ui::workspace::{Slot, Toast};
use gitviber_ui::{ActiveAppTheme, UiState, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Global, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, PathPromptOptions, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Task, Window, div, hsla,
    prelude::FluentBuilder as _, px, rems,
};

use crate::help::{open_url, toast};

#[derive(Clone, Debug, Default, PartialEq)]
struct Info {
    exists: bool,
    github: Option<String>,
}

/// What each saved folder is now, asked when the list changes and when a window comes forward,
/// so a moved folder shows up soon.
#[derive(Default)]
struct Infos {
    by_path: HashMap<String, Info>,
    asked: Vec<String>,
    _task: Option<Task<()>>,
}

impl Global for Infos {}

/// Asks again about the saved folders (on a window's activation, or when the list changed).
pub fn refresh(force: bool, cx: &mut App) {
    if !gitviber_ui::has_core(cx) {
        return;
    }
    let list = UiState::get(cx).recent_projects.clone();
    let infos = cx.default_global::<Infos>();
    if !force && infos.asked == list {
        return;
    }
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

pub fn folder_name(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}

/// A stable hue per name, so projects are told apart at a glance.
pub fn hue(name: &str) -> f32 {
    let mut h: u32 = 0;
    for c in name.encode_utf16() {
        h = (h * 31 + u32::from(c)) % 360;
    }
    h as f32
}

/// Square letter tile in its project's hue.
pub fn tile(name: &str, large: bool) -> AnyElement {
    let letter: String = name.chars().next().unwrap_or('?').to_uppercase().collect();
    let bg: Hsla = hsla(hue(name) / 360., 0.55, 0.62, 1.);
    div()
        .size(px(if large { 24. } else { 16. }))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .bg(bg)
        .text_color(hsla(0., 0., 0., 0.8))
        .text_size(rems(if large { 0.6875 } else { 0.59375 }))
        .font_weight(gpui_kit::FontWeight::BOLD)
        .child(letter)
        .into_any_element()
}

/// The order shown: folders that are gone sink below the rest, each group in the user's order.
pub fn shown_order(list: &[String], gone: impl Fn(&str) -> bool) -> Vec<String> {
    let (here, away): (Vec<&String>, Vec<&String>) = list.iter().partition(|p| !gone(p));
    here.into_iter().chain(away).cloned().collect()
}

/// `list` with `from` moved to where `to` is.
pub fn moved(list: &[String], from: &str, to: &str) -> Vec<String> {
    let mut out: Vec<String> = list.to_vec();
    let (Some(a), Some(b)) = (
        out.iter().position(|p| p == from),
        out.iter().position(|p| p == to),
    ) else {
        return out;
    };
    let item = out.remove(a);
    out.insert(b, item);
    out
}

fn forget(path: &str, cx: &mut App) {
    let path = path.to_string();
    UiState::update(cx, move |s| s.recent_projects.retain(|p| *p != path));
}

fn move_to_top(path: &str, cx: &mut App) {
    let path = path.to_string();
    UiState::update(cx, move |s| {
        gitviber_ui::state::bump(&mut s.recent_projects, &path, 20)
    });
}

fn open_project(path: &str, window: &mut Window, cx: &mut App) {
    if let Some(ws) = Workspace::for_window(window, cx) {
        ws.update(cx, |ws, cx| {
            ws.open_project(PathBuf::from(path), window, cx)
        });
    }
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
fn locate(path: &str, window: &mut Window, cx: &mut App) {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Locate".into()),
    });
    let old = path.to_string();
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = picked.await else {
            return;
        };
        let Some(new) = paths.into_iter().next() else {
            return;
        };
        let new = new.to_string_lossy().into_owned();
        cx.update(|cx| {
            let (old, put) = (old.clone(), new.clone());
            UiState::update(cx, move |s| {
                s.recent_projects.retain(|p| *p != put);
                if let Some(p) = s.recent_projects.iter_mut().find(|p| **p == old) {
                    *p = put;
                }
            });
            refresh(true, cx);
            let _ = handle.update(cx, |_, window, cx| open_project(&new, window, cx));
        });
    })
    .detach();
}

#[derive(Clone)]
struct DraggedProject {
    path: String,
}

impl Render for DraggedProject {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .bg(t.elevated)
            .border_1()
            .border_color(t.border_strong)
            .child(tile(folder_name(&self.path), false))
            .child(folder_name(&self.path).to_string())
    }
}

/// The list. `current`: the open project's main folder, which can't be removed and doesn't open
/// again; `compact`: a line a row without hover actions, for the narrow sidebar.
pub fn list(id: &'static str, current: Option<String>, compact: bool, cx: &App) -> AnyElement {
    let recent = UiState::get(cx).recent_projects.clone();
    let gone = |p: &str| info(p, cx).is_some_and(|i| !i.exists);
    let shown = shown_order(&recent, gone);
    let first = recent.first().cloned();
    let t = cx.app_theme().clone();
    let rows = shown.into_iter().enumerate().map(|(i, path)| {
        let name = folder_name(&path).to_string();
        let i_info = info(&path, cx);
        // Unknown until project_info answers: taken as there rather than flash every row dimmed.
        let missing = i_info.as_ref().is_some_and(|i| !i.exists);
        let github = i_info.and_then(|i| i.github);
        let here = current.as_deref() == Some(path.as_str());
        let detail = if missing {
            "Folder not found".to_string()
        } else {
            path.clone()
        };
        let (open, drop_to) = (path.clone(), path.clone());
        let is_first = first.as_deref() == Some(path.as_str());
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
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            locate(&p, window, cx)
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
            if !here {
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
            .when(here, |d| d.bg(t.active))
            .when(!here, |d| d.cursor_pointer().hover(|s| s.bg(t.hover)))
            .child(
                div()
                    .when(missing, |d| d.opacity(0.45))
                    .child(tile(&name, !compact)),
            )
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
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .when(missing, |d| d.text_color(t.muted_foreground))
                            .child(name.clone()),
                    )
                    .when(!compact, |d| {
                        d.child(
                            div()
                                .truncate()
                                .text_size(rems(0.65625))
                                .text_color(if missing { t.conflict } else { t.subtle })
                                .child(detail.clone()),
                        )
                    }),
            )
            .when(here, |d| {
                d.child(Icon::new(IconName::Check).small().text_color(t.primary))
            })
            .children(actions)
            .when(compact, |d| {
                let detail = detail.clone();
                d.tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(detail.clone()).build(window, cx)
                })
            })
            .on_click(move |_, window, cx| {
                if missing {
                    locate(&open, window, cx)
                } else if !here {
                    open_project(&open, window, cx)
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
                    s.recent_projects = moved(&s.recent_projects, &from, &to)
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
                            .on_click(move |_, window, cx| locate(&l, window, cx)),
                    );
                } else {
                    let (o, tm, r) = (p.clone(), p.clone(), p.clone());
                    m = m
                        .item(
                            PopupMenuItem::new("Open")
                                .icon(IconName::FolderOpen)
                                .disabled(here)
                                .on_click(move |_, window, cx| open_project(&o, window, cx)),
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
                            .on_click(move |_, _, cx| copy(folder_name(&cn), "Name copied", cx)),
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("Move to Top")
                            .icon(IconName::ArrowUpToLine)
                            .disabled(is_first)
                            .on_click(move |_, _, cx| move_to_top(&top, cx)),
                    )
                    .item(
                        PopupMenuItem::new("Remove from List")
                            .icon(IconName::X)
                            .disabled(here)
                            .on_click(move |_, _, cx| forget(&f, cx)),
                    )
            })
            .into_any_element()
    });
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap_px()
        .w_full()
        .children(rows)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hues_are_stable_and_spread() {
        assert_eq!(hue("git_viber"), hue("git_viber"));
        assert_ne!(hue("alpha"), hue("beta"));
        assert!(hue("x") < 360.);
        // As the old app's tile computed it (UTF-16 units, mod 360 each step).
        let mut h = 0u32;
        for c in "ab".encode_utf16() {
            h = (h * 31 + c as u32) % 360;
        }
        assert_eq!(hue("ab"), h as f32);
    }

    #[test]
    fn gone_folders_sink_and_drags_reorder() {
        let list: Vec<String> = ["/a", "/b", "/c"].map(String::from).into();
        assert_eq!(shown_order(&list, |p| p == "/a"), ["/b", "/c", "/a"]);
        assert_eq!(moved(&list, "/c", "/a"), ["/c", "/a", "/b"]);
        assert_eq!(moved(&list, "/a", "/c"), ["/b", "/c", "/a"]);
        assert_eq!(moved(&list, "/x", "/a"), list);
        assert_eq!(folder_name("/Users/me/repo/"), "repo");
    }
}
