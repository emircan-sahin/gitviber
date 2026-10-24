//! What the views say about the workspace: tabs an agent opened with `gitviber browser` (taken
//! into the workspace of the worktree its pane opened in, never made the active tab unless opened
//! to show the user), tabs their agents closed, the devices and schemes they chose, popups of the
//! user's tabs, and the app's keys typed in a page. Plus what the app tells the browser: its
//! settings, a pane that closed, an agent done in a worktree, worktrees removed.

use super::devices::DeviceChoice;
use super::hub::Hub;
use super::tab::{BrowserTab, Saved, Scheme};
use crate::Event;
use gitviber_core::Event as CoreEvent;
use gitviber_ui::workspace::{selection_key, Selection, Toast, Workspace};
use gitviber_ui::{CoreEvents, Settings};
use gpui::{
    AnyWindowHandle, App, AppContext as _, Entity, Global, Keystroke, Subscription, Task, Window,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};

fn key_of(id: &str) -> String {
    format!("browser::{id}")
}

/// Every window's workspace, with its window.
fn workspaces(cx: &mut App) -> Vec<(AnyWindowHandle, Entity<Workspace>)> {
    cx.windows()
        .into_iter()
        .filter_map(|w| {
            let ws = w
                .update(cx, |_, window, cx| Workspace::for_window(window, cx))
                .ok()??;
            Some((w, ws))
        })
        .collect()
}

/// The window whose workspace has `root` open: an agent's tab goes to its pane's worktree.
fn workspace_of(root: &str, cx: &mut App) -> Option<(AnyWindowHandle, Entity<Workspace>)> {
    let want = crate::root_key(root);
    workspaces(cx).into_iter().find(|(_, ws)| {
        ws.read(cx).project().is_some_and(|p| {
            let open = crate::root_key(&p.read(cx).root().to_string_lossy());
            want == open || std::path::Path::new(&want).starts_with(&open)
        })
    })
}

/// The window and the tab view showing page `id`, when one is built.
fn tab_of(
    id: &str,
    cx: &mut App,
) -> Option<(
    AnyWindowHandle,
    Entity<Workspace>,
    Option<Entity<BrowserTab>>,
)> {
    let key = key_of(id);
    workspaces(cx).into_iter().find_map(|(w, ws)| {
        let entry = ws.read(cx).tabs().get(&key)?;
        let view = entry
            .item
            .handle()
            .and_then(|h| h.view().downcast::<BrowserTab>().ok());
        Some((w, ws.clone(), view))
    })
}

#[derive(Default)]
struct Told {
    downloads: Option<Instant>,
    popups: Option<Instant>,
}

impl Global for Told {}

/// One toast for a burst of refusals.
fn once_in_a_while(at: fn(&mut Told) -> &mut Option<Instant>, cx: &mut App) -> bool {
    let told = at(cx.default_global::<Told>());
    if told.is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
        return false;
    }
    *told = Some(Instant::now());
    true
}

fn toast_in_front(toast: Toast, cx: &mut App) {
    let Some(w) = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next())
    else {
        return;
    };
    let _ = w.update(cx, |_, window, cx| {
        match Workspace::for_window(window, cx) {
            Some(ws) => ws.update(cx, |ws, cx| ws.toast(toast, cx)),
            None => toast.show(window, cx),
        }
    });
}

/// A key the page handed back (keys.rs), as GPUI names it.
fn keystroke(k: &PageKey) -> Option<Keystroke> {
    let key = match k.key.as_str() {
        "ArrowUp" => "up".to_string(),
        "ArrowDown" => "down".into(),
        "ArrowLeft" => "left".into(),
        "ArrowRight" => "right".into(),
        "Enter" => "enter".into(),
        "Escape" => "escape".into(),
        "Tab" => "tab".into(),
        " " => "space".into(),
        "Backspace" => "backspace".into(),
        "Delete" => "delete".into(),
        "Home" => "home".into(),
        "End" => "end".into(),
        "PageUp" => "pageup".into(),
        "PageDown" => "pagedown".into(),
        "+" => "=".into(),
        "{" => "[".into(),
        "}" => "]".into(),
        k => k.to_lowercase(),
    };
    let mut text = String::new();
    for (held, m) in [
        (k.ctrl_key, "ctrl-"),
        (k.alt_key, "alt-"),
        (k.shift_key, "shift-"),
        (k.meta_key, "cmd-"),
    ] {
        if held {
            text.push_str(m);
        }
    }
    text.push_str(&key);
    Keystroke::parse(&text).ok()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageKey {
    id: String,
    key: String,
    meta_key: bool,
    ctrl_key: bool,
    alt_key: bool,
    shift_key: bool,
}

/// An event of the views' that the workspace acts on (hub.rs passes them on).
pub(super) fn workspace_event(event: Event, cx: &mut App) {
    match event.name {
        "browser-open" => {
            #[derive(Deserialize)]
            struct Opened {
                id: String,
                url: String,
                root: String,
                show: bool,
            }
            if let Ok(o) = serde_json::from_value::<Opened>(event.payload) {
                open_agent_tab(o.id, o.url, o.root, o.show, cx);
            }
        }
        "browser-closed" => {
            #[derive(Deserialize)]
            struct Id {
                id: String,
            }
            let Ok(Id { id }) = serde_json::from_value(event.payload) else {
                return;
            };
            if let Some((_, ws, _)) = tab_of(&id, cx) {
                // Closed for good: its page can't come back as it was.
                ws.update(cx, |ws, cx| ws.force_close_tabs(&[key_of(&id).into()], cx));
            }
            if let Some(hub) = Hub::global(cx) {
                hub.update(cx, |hub, _| hub.forget(&id));
            }
        }
        "browser-device" => {
            #[derive(Deserialize)]
            struct Device {
                id: String,
                device: Option<DeviceChoice>,
            }
            let Ok(d) = serde_json::from_value::<Device>(event.payload) else {
                return;
            };
            if let Some((w, _, Some(tab))) = tab_of(&d.id, cx) {
                let _ = w.update(cx, |_, window, cx| {
                    tab.update(cx, |tab, cx| tab.set_device(d.device, window, cx))
                });
            }
        }
        "browser-appearance" => {
            #[derive(Deserialize)]
            struct Shown {
                id: String,
                dark: Option<bool>,
            }
            let Ok(s) = serde_json::from_value::<Shown>(event.payload) else {
                return;
            };
            if let Some((w, _, Some(tab))) = tab_of(&s.id, cx) {
                let _ = w.update(cx, |_, window, cx| {
                    tab.update(cx, |tab, cx| tab.set_scheme(s.dark, window, cx))
                });
            }
        }
        "browser-popup" => {
            #[derive(Deserialize)]
            struct Popped {
                id: Option<String>,
                opener: String,
                url: String,
            }
            let Ok(p) = serde_json::from_value::<Popped>(event.payload) else {
                return;
            };
            let Some(id) = p.id else {
                return;
            };
            open_popup(id, p.opener, p.url, cx);
        }
        "browser-popup-blocked" => {
            if once_in_a_while(|t| &mut t.popups, cx) {
                toast_in_front(
                    Toast::info("This page tried to open several windows at once.")
                        .title("Popup blocked"),
                    cx,
                );
            }
        }
        "browser-download-blocked" => {
            if once_in_a_while(|t| &mut t.downloads, cx) {
                toast_in_front(
                    Toast::info("A page started several downloads at once, or one out of sight.")
                        .title("Download blocked"),
                    cx,
                );
            }
        }
        "browser-key" => {
            let Ok(k) = serde_json::from_value::<PageKey>(event.payload) else {
                return;
            };
            let (Some((w, _, _)), Some(stroke)) = (tab_of(&k.id, cx), keystroke(&k)) else {
                return;
            };
            let _ = w.update(cx, |_, window, cx| window.dispatch_keystroke(stroke, cx));
        }
        _ => {}
    }
}

fn open_agent_tab(id: String, url: String, root: String, show: bool, cx: &mut App) {
    let Some((w, ws)) = workspace_of(&root, cx) else {
        // No window has its worktree open: it runs out of sight until one does.
        return;
    };
    let selection = Selection::Browser {
        id: id.clone(),
        url: url.clone(),
        title: None,
        device: None,
        zoom: None,
        scheme: None,
    };
    if !show && !Settings::get(cx).browser_show_agent_tabs {
        return;
    }
    let _ = w.update(cx, |_, window, cx| {
        if show {
            ws.update(cx, |ws, cx| ws.open(selection, false, window, cx));
            return;
        }
        let saved = Saved::of(&selection).unwrap_or_else(|| unreachable!());
        let weak = ws.downgrade();
        let tab = cx.new(|cx| BrowserTab::new(saved, root, weak, window, cx));
        // No selection: an agent's tab isn't saved with the workspace nor let go for memory, as
        // its page can't come back as it was.
        let key = selection_key(&selection);
        ws.update(cx, |ws, cx| ws.add_tab(key, tab, None, cx));
    });
}

/// A popup of a user's tab joins its opener's workspace and comes forward, as a browser does;
/// light or dark as the opener shows (a device's user agent would load it again).
fn open_popup(id: String, opener: String, url: String, cx: &mut App) {
    let Some((w, ws, _)) = tab_of(&opener, cx) else {
        // Its opener went before the tab could be made: nothing would ever show or close it.
        crate::close(&id);
        return;
    };
    let scheme = ws
        .read(cx)
        .tabs()
        .get(&key_of(&opener))
        .and_then(|t| t.item.selection())
        .and_then(Saved::of)
        .and_then(|s| s.scheme);
    let selection = Selection::Browser {
        id,
        url,
        title: None,
        device: None,
        zoom: None,
        scheme: scheme.map(|s| if s == Scheme::Dark { "dark" } else { "light" }.to_string()),
    };
    let _ = w.update(cx, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open(selection, false, window, cx))
    });
}

/// What the browser keeps of the app while it runs: its settings sent as they change, the panes
/// with agents' tabs watched for closing, agents finishing, worktrees removed.
pub(super) struct Watch {
    _settings: Subscription,
    _core: Option<Subscription>,
    _panes: Task<()>,
}

impl Global for Watch {}

/// Settings → Browser, as the views and the socket take it; the skill on its own thread.
fn send_settings(was: &mut Option<(u32, u32, u32, bool, bool, bool, bool)>, cx: &mut App) {
    let s = Settings::get(cx);
    let now = (
        s.browser_live_hidden,
        s.browser_park_after_min,
        s.browser_agent_drawn,
        s.browser_console,
        s.browser_agent_control,
        s.browser_mute_agent_tabs,
        s.browser_agent_control && s.browser_agent_skill,
    );
    if *was == Some(now) {
        return;
    }
    let skill_was = was.map(|w| w.6);
    *was = Some(now);
    let (live, park, drawn, console, control, muted, skill) = now;
    crate::configure(live, park, drawn, console, muted);
    if let Some(handle) = crate::installed() {
        crate::server::set_enabled(handle, control);
    }
    // Written at launch too: a new version's HELP replaces the one an older one wrote.
    if skill_was != Some(skill) {
        std::thread::spawn(move || drop(crate::skill::sync(Some(skill))));
    }
    send_app_keys(cx);
}

/// Only the chords bound to the app's commands, and the tab's own (⌘L, ⌘R), leave a page.
fn send_app_keys(cx: &App) {
    use gitviber_ui::keymap::{all_commands, bindings_for, IS_MAC};
    let overrides = &Settings::get(cx).keybindings;
    let chords: Vec<String> = all_commands()
        .filter(|c| c.local.is_none_or(|l| l == super::tab::CONTEXT))
        .flat_map(|c| bindings_for(c, overrides, IS_MAC))
        .collect();
    crate::set_app_keys(chords);
}

pub(super) fn watch(cx: &mut App) {
    let mut sent = None;
    send_settings(&mut sent, cx);
    let settings = cx.observe_global::<Settings>(move |cx| send_settings(&mut sent, cx));
    let core = gitviber_ui::has_core(cx).then(|| {
        let mut states: HashMap<u32, Option<&'static str>> = HashMap::new();
        let mut finishing: HashMap<String, Task<()>> = HashMap::new();
        cx.subscribe(
            &CoreEvents::global(cx),
            move |_, event: &CoreEvent, cx| match event {
                CoreEvent::WorktreesRemoved(roots) => {
                    for root in roots {
                        crate::close_root(root);
                    }
                }
                CoreEvent::AgentState(a) => {
                    let was = states.insert(a.id, a.state);
                    let done = was.flatten() == Some("working") && a.state != Some("working");
                    if !done || !Settings::get(cx).browser_reload_on_agent_done {
                        return;
                    }
                    let Some(dir) = gitviber_ui::core(cx).ptys.pane(a.id) else {
                        return;
                    };
                    let dir = dir.to_string_lossy().into_owned();
                    // Agents in one worktree often finish together: one reload for them.
                    let key = dir.clone();
                    finishing.retain(|_, t| !t.is_ready());
                    let task = cx.spawn(async move |cx| {
                        cx.background_executor().timer(Duration::from_secs(1)).await;
                        cx.update(|_| crate::agent_done(&dir));
                    });
                    finishing.insert(key, task);
                }
                _ => {}
            },
        )
    });
    // A closed pane's tabs are the user's from then on (agent.rs `agent_pane_closed`): looked
    // for while any pane has agents' tabs.
    let panes = cx.spawn(async move |cx| loop {
        cx.background_executor().timer(Duration::from_secs(2)).await;
        cx.update(|cx| {
            if !gitviber_ui::has_core(cx) {
                return;
            }
            let core = gitviber_ui::core(cx);
            for pty in crate::agent_ptys() {
                if core.ptys.pane(pty).is_none() {
                    crate::agent_pane_closed(pty);
                }
            }
        });
    });
    cx.set_global(Watch {
        _settings: settings,
        _core: core,
        _panes: panes,
    });
}

/// Every page's cookies, cache and storage, and the sessions agents saved, once the user says so.
pub(super) fn clear_data(window: &mut Window, cx: &mut App) {
    let answer = window.prompt(
        gpui::PromptLevel::Warning,
        "Clear the browser's data?",
        Some("Every tab's cookies, cache and storage go, and the sessions agents saved. Your own browser's data isn't touched."),
        &["Clear", "Cancel"],
        cx,
    );
    window
        .spawn(cx, async move |cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            let Some(handle) = crate::installed() else {
                return;
            };
            let done = cx
                .background_spawn(async move {
                    crate::clear_data(handle)?;
                    let data = handle.path().app_data_dir()?;
                    crate::Shelf::clear_all(&data)
                        .map_err(|e| format!("Couldn't delete saved sessions: {e}"))
                })
                .await;
            let toast = match done {
                Ok(()) => Toast::success("The browser's data is cleared."),
                Err(e) => Toast::error(e).title("Couldn't clear the browser's data"),
            };
            let _ = cx.update(|window, cx| match Workspace::for_window(window, cx) {
                Some(ws) => ws.update(cx, |ws, cx| ws.toast(toast, cx)),
                None => toast.show(window, cx),
            });
        })
        .detach();
}

/// A new user tab in `window`'s workspace, beside the code (⇧⌘B).
pub fn open_new_tab(window: &mut Window, cx: &mut App) {
    let Some(ws) = Workspace::for_window(window, cx) else {
        return;
    };
    let selection = Selection::Browser {
        id: super::new_id(),
        url: super::url::BLANK.into(),
        title: None,
        device: None,
        zoom: None,
        scheme: None,
    };
    ws.update(cx, |ws, cx| ws.open(selection, false, window, cx));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: &str, cmd: bool, shift: bool) -> PageKey {
        PageKey {
            id: "t".into(),
            key: key.into(),
            meta_key: cmd,
            ctrl_key: false,
            alt_key: false,
            shift_key: shift,
        }
    }

    #[test]
    fn a_page_key_reads_as_gpui_names_it() {
        assert_eq!(
            keystroke(&key("l", true, false)).unwrap().unparse(),
            "cmd-l"
        );
        assert_eq!(
            keystroke(&key("{", true, true)).unwrap().unparse(),
            "cmd-shift-["
        );
        assert_eq!(
            keystroke(&key("ArrowLeft", true, false)).unwrap().unparse(),
            "cmd-left"
        );
        assert_eq!(
            keystroke(&key("B", true, true)).unwrap().unparse(),
            "cmd-shift-b"
        );
    }
}
