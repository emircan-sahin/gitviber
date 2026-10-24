//! "Open in": the editors, terminals and git clients found here and the user's own apps
//! (Settings → Open In), for the status bar's button, File → Open in… and right-click menus.
//! Opening one makes it the one a click runs (Settings' `open_in_app`).
//!
//! ```ignore
//! // A right-click menu's item: "Open in <app>", or a submenu of every app until one is picked.
//! menu = gitviber_ui::open_in::menu_items(menu, move |_| OpenTarget::file(path.clone()), window, cx);
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use gitviber_core::api;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::{
    App, AppContext as _, Context, Global, Image, ImageFormat, SharedString, Task, Window,
};

use crate::settings::Settings;
use crate::workspace::Toast;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Editor,
    Terminal,
    /// Git clients.
    Other,
    /// The user's own, with a command.
    Custom,
}

impl Group {
    pub const ORDER: [Group; 4] = [Group::Editor, Group::Terminal, Group::Other, Group::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Group::Editor => "Editors",
            Group::Terminal => "Terminals",
            Group::Other => "Git Clients",
            Group::Custom => "Your Apps",
        }
    }

    fn parse(text: &str) -> Group {
        match text {
            "editor" => Group::Editor,
            "terminal" => Group::Terminal,
            _ => Group::Other,
        }
    }
}

/// An app "Open in" offers.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenApp {
    pub id: String,
    pub name: String,
    pub group: Group,
    /// The user's own app's command template; None for one found here.
    pub command: Option<String>,
}

/// What to open: a path in the worktree ("" for all of it), the line an editor should show, and
/// whether the worktree opens as a project with the file in it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenTarget {
    pub path: String,
    pub line: Option<u32>,
    pub project: bool,
}

impl OpenTarget {
    /// The worktree as a project, with `path` in it when one is given.
    pub fn project(path: Option<String>, line: Option<u32>) -> Self {
        OpenTarget {
            path: path.unwrap_or_default(),
            line,
            project: true,
        }
    }

    /// One file or folder of the worktree, by itself.
    pub fn file(path: impl Into<String>) -> Self {
        OpenTarget {
            path: path.into(),
            line: None,
            project: false,
        }
    }
}

/// The apps found here, asked once a run and again when a list opens; their icons (macOS).
/// Observe it (`cx.observe_global::<Found>`) to follow the list.
#[derive(Default)]
pub struct Found {
    apps: Vec<(String, String, Group)>,
    icons: HashMap<String, Option<Arc<Image>>>,
    _ask: Option<Task<()>>,
    _icons: Vec<Task<()>>,
}

impl Global for Found {}

/// Asks again which apps are installed, off the UI thread; the menus redraw with the answer.
pub fn refresh(cx: &mut App) {
    if !crate::has_core(cx) {
        return;
    }
    let core = crate::core(cx);
    let asked = cx.background_spawn(async move {
        let list = api::files::open_in_apps(&core).unwrap_or_default();
        // Installed keeps its fields to itself; its JSON is what the old app read.
        serde_json::to_value(list)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| {
                Some((
                    a.get("id")?.as_str()?.to_string(),
                    a.get("name")?.as_str()?.to_string(),
                    Group::parse(a.get("group")?.as_str()?),
                ))
            })
            .collect::<Vec<_>>()
    });
    let task = cx.spawn(async move |cx| {
        let apps = asked.await;
        cx.update(|cx| {
            let found = cx.default_global::<Found>();
            if found.apps != apps {
                found.apps = apps;
                cx.refresh_windows();
            }
        });
    });
    cx.default_global::<Found>()._ask = Some(task);
}

/// Every app it offers, in menu order: the found ones (unless Settings hides them), then the
/// user's own that have a name and a command.
pub fn apps(cx: &App) -> Vec<OpenApp> {
    let s = Settings::get(cx);
    let found = cx
        .try_global::<Found>()
        .map(|f| f.apps.clone())
        .unwrap_or_default();
    list_for(s, &found)
}

fn list_for(s: &Settings, found: &[(String, String, Group)]) -> Vec<OpenApp> {
    let builtin = found
        .iter()
        .filter(|_| !s.open_in_hide_builtins)
        .map(|(id, name, group)| OpenApp {
            id: id.clone(),
            name: name.clone(),
            group: *group,
            command: None,
        });
    let custom = s
        .open_in_custom
        .iter()
        .filter(|c| !c.name.trim().is_empty() && !c.command.trim().is_empty())
        .map(|c| OpenApp {
            id: c.id.clone(),
            name: c.name.trim().to_string(),
            group: Group::Custom,
            command: Some(c.command.clone()),
        });
    builtin.chain(custom).collect()
}

/// The app a click runs, while it's still offered.
pub fn last(cx: &App) -> Option<OpenApp> {
    let id = &Settings::get(cx).open_in_app;
    (!id.is_empty())
        .then(|| apps(cx).into_iter().find(|a| a.id == *id))
        .flatten()
}

/// Opens `target` in `app`, which becomes the one a click runs.
pub fn open(app: &OpenApp, target: OpenTarget, cx: &mut App) {
    let id = app.id.clone();
    Settings::update(cx, |s| s.open_in_app = id);
    if !crate::has_core(cx) {
        return;
    }
    let (core, app) = (crate::core(cx), app.clone());
    let name = app.name.clone();
    let opened = cx.background_spawn(async move {
        let OpenTarget {
            path,
            line,
            project,
        } = target;
        match &app.command {
            Some(command) => {
                api::files::open_in_custom(&core, command.clone(), path, line, project)
            }
            None => api::files::open_in(&core, app.id.clone(), path, line, project),
        }
    });
    cx.spawn(async move |cx| {
        if let Err(err) = opened.await {
            cx.update(|cx| {
                let Some(window) = cx
                    .active_window()
                    .or_else(|| cx.windows().into_iter().next())
                else {
                    return;
                };
                let _ = window.update(cx, |_, window, cx| {
                    Toast::error(err)
                        .title(format!("Could not open in {name}"))
                        .show(window, cx)
                });
            });
        }
    })
    .detach();
}

/// A found app's icon as macOS draws it, asked for the first time something shows it; None
/// for the user's own apps, off macOS, and while it's drawn.
pub fn icon(app: &OpenApp, cx: &mut App) -> Option<Arc<Image>> {
    if !cfg!(target_os = "macos") || app.command.is_some() || !crate::has_core(cx) {
        return None;
    }
    if let Some(known) = cx.default_global::<Found>().icons.get(&app.id) {
        return known.clone();
    }
    let (core, id) = (crate::core(cx), app.id.clone());
    cx.default_global::<Found>().icons.insert(id.clone(), None);
    let drawn = cx.background_spawn({
        let id = id.clone();
        async move { api::files::open_in_icon(&core, id).unwrap_or_default() }
    });
    let task = cx.spawn(async move |cx| {
        let png = drawn.await;
        cx.update(|cx| {
            let image =
                (!png.is_empty()).then(|| Arc::new(Image::from_bytes(ImageFormat::Png, png)));
            cx.default_global::<Found>().icons.insert(id, image);
            cx.refresh_windows();
        });
    });
    cx.default_global::<Found>()._icons.push(task);
    None
}

/// The groups of `apps` that have any, in menu order.
pub fn grouped(apps: &[OpenApp]) -> Vec<(Group, Vec<OpenApp>)> {
    Group::ORDER
        .into_iter()
        .map(|g| {
            (
                g,
                apps.iter()
                    .filter(|a| a.group == g)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        })
        .filter(|(_, list)| !list.is_empty())
        .collect()
}

/// A right-click menu's "Open in <last app>" for `target`, or a submenu of every app until one
/// is picked; nothing when none is found.
pub fn menu_items(
    menu: PopupMenu,
    target: impl Fn(&App) -> OpenTarget + 'static,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let target = std::rc::Rc::new(target);
    if let Some(app) = last(cx) {
        let label: SharedString = format!("Open in {}", app.name).into();
        return menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
            let t = target(cx);
            open(&app, t, cx)
        }));
    }
    let list = apps(cx);
    if list.is_empty() {
        return menu;
    }
    menu.submenu("Open in", window, cx, move |mut sub, _, _| {
        for (group, apps) in grouped(&list) {
            sub = sub.label(group.label());
            for app in apps {
                let target = target.clone();
                sub = sub.item(
                    PopupMenuItem::new(app.name.clone()).on_click(move |_, _, cx| {
                        let t = target(cx);
                        open(&app, t, cx)
                    }),
                );
            }
        }
        sub
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::CustomApp;

    #[test]
    fn the_list_is_the_found_apps_then_the_users_own() {
        let found = vec![
            ("zed".to_string(), "Zed".to_string(), Group::Editor),
            ("iterm".to_string(), "iTerm2".to_string(), Group::Terminal),
        ];
        let mut s = Settings {
            open_in_custom: vec![
                CustomApp {
                    id: "custom:a".into(),
                    name: " Mine ".into(),
                    command: "mine {path}".into(),
                },
                CustomApp {
                    id: "custom:b".into(),
                    name: "No command".into(),
                    command: " ".into(),
                },
            ],
            ..Settings::default()
        };
        let ids: Vec<String> = list_for(&s, &found).into_iter().map(|a| a.id).collect();
        assert_eq!(ids, ["zed", "iterm", "custom:a"]);
        assert_eq!(list_for(&s, &found)[2].name, "Mine");
        s.open_in_hide_builtins = true;
        let ids: Vec<String> = list_for(&s, &found).into_iter().map(|a| a.id).collect();
        assert_eq!(ids, ["custom:a"]);
        let groups: Vec<Group> = grouped(&list_for(&Settings::default(), &found))
            .into_iter()
            .map(|(g, _)| g)
            .collect();
        assert_eq!(groups, [Group::Editor, Group::Terminal]);
    }
}
