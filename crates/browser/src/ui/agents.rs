//! What the views say about the workspace: tabs an agent opened with `gitviber browser` (taken
//! into the workspace of the worktree its pane opened in, never made the active tab unless opened
//! to show the user), tabs their agents closed, the devices and schemes they chose, and popups of
//! the user's tabs. Plus what the app tells the browser: its settings, a pane that closed, an
//! agent done in a worktree, worktrees removed.

use super::devices::DeviceChoice;
use super::hub::Hub;
use super::tab::{BrowserTab, Saved, Scheme};
use crate::Event;
use gitviber_core::Event as CoreEvent;
use gitviber_ui::workspace::{selection_key, Selection, Toast, Workspace};
use gitviber_ui::{CoreEvents, Settings};
use gpui::{AnyWindowHandle, App, AppContext as _, Entity, Global, Subscription, Task, Window};
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
        "browser-popup-blocked" if once_in_a_while(|t| &mut t.popups, cx) => {
            toast_in_front(
                Toast::info("This page tried to open several windows at once.")
                    .title("Popup blocked"),
                cx,
            );
        }
        "browser-download-blocked" if once_in_a_while(|t| &mut t.downloads, cx) => {
            toast_in_front(
                Toast::info("A page started several downloads at once, or one out of sight.")
                    .title("Download blocked"),
                cx,
            );
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

/// A popup of a user's tab joins its opener's workspace beside it and comes forward, as a browser
/// does; light or dark as the opener shows (a device's user agent would load it again).
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
    let beside = key_of(&opener).into();
    let _ = w.update(cx, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_beside(selection, beside, window, cx))
    });
}

/// What the browser keeps of the app while it runs: its settings sent as they change, agents
/// finishing, worktrees removed.
pub(super) struct Watch {
    _settings: Subscription,
    _core: Option<Subscription>,
}

impl Global for Watch {}

/// Each agent's skill as the latest sync left it. Only this side writes the skill: Settings shows
/// this and hears it change, so what it shows is never read before a write it asked for has
/// finished.
pub struct SkillStatus {
    pub skill: Option<crate::skill::Skill>,
    /// The sync going: a newer one replaces it.
    sync: Task<()>,
    syncing: bool,
}

impl Global for SkillStatus {}

impl Default for SkillStatus {
    fn default() -> Self {
        Self {
            skill: None,
            sync: Task::ready(()),
            syncing: false,
        }
    }
}

impl SkillStatus {
    /// None before the first sync is done and while one is going: what it had may be rewritten.
    pub fn get(cx: &App) -> Option<&crate::skill::Skill> {
        let status = cx.try_global::<Self>()?;
        status.skill.as_ref().filter(|_| !status.syncing)
    }
}

/// Settings → Browser, as the views and the socket take it; the skill off the UI thread.
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
        let sync = cx.spawn(async move |cx| {
            let done = cx
                .background_spawn(async move { crate::skill::sync(Some(skill)) })
                .await;
            cx.update(|cx| {
                let status = cx.global_mut::<SkillStatus>();
                status.skill = Some(done);
                status.syncing = false;
            });
        });
        let status = cx.default_global::<SkillStatus>();
        status.sync = sync;
        status.syncing = true;
    }
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
    cx.set_global(Watch {
        _settings: settings,
        _core: core,
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

    /// Settings reads the skill's status from here, written once each sync has: it showed one
    /// read before the main window's write was done, and none changed as the switch flipped.
    #[gpui::test]
    fn the_skill_status_is_the_latest_syncs(cx: &mut gpui::TestAppContext) {
        let home = gitviber_ui::test_support::temp_dir("browser-skill-home");
        gitviber_ui::test_support::config_dir("browser-skill");
        // SAFETY: nextest gives each test its own process; nothing reads HOME meanwhile.
        unsafe { std::env::set_var("HOME", &home) };
        cx.update(gitviber_ui::init);
        let mut sent = None;
        cx.update(|cx| send_settings(&mut sent, cx));
        assert!(
            cx.update(|cx| SkillStatus::get(cx).is_none()),
            "not before it's done"
        );
        cx.run_until_parked();
        let first = cx.update(|cx| SkillStatus::get(cx).cloned()).unwrap();
        assert!(!first.targets.is_empty());
        let heard = std::rc::Rc::new(std::cell::Cell::new(0));
        let count = heard.clone();
        let _watch =
            cx.update(|cx| cx.observe_global::<SkillStatus>(move |_| count.set(count.get() + 1)));
        cx.update(|cx| {
            Settings::update(cx, |s| {
                s.browser_agent_control = !s.browser_agent_control;
            });
            send_settings(&mut sent, cx);
        });
        assert!(
            cx.update(|cx| SkillStatus::get(cx).is_none()),
            "not while the next one is going"
        );
        cx.run_until_parked();
        assert!(heard.get() > 0, "Settings hears the new sync");
        assert!(cx.update(|cx| SkillStatus::get(cx).is_some()));
    }
}
