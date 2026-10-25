//! A repo whose file watcher didn't start opens without live updates (core's RepoWatchFailed):
//! said once per repo a session, with Retry to start the watcher again.

use std::collections::HashSet;
use std::path::Path;

use gitviber_core::{Event, api};
use gitviber_ui::CoreEvents;
use gitviber_ui::workspace::Toast;
use gpui_kit::{App, AppContext as _, Global, Subscription};

use crate::help::toast;

#[derive(Default)]
struct LiveUpdates {
    _events: Option<Subscription>,
    /// Repos already told about: switching back and forth to one doesn't say it again.
    told: HashSet<String>,
}

impl Global for LiveUpdates {}

pub fn install(cx: &mut App) {
    if !gitviber_ui::has_core(cx) {
        return;
    }
    let events = cx.subscribe(&CoreEvents::global(cx), |_, event: &Event, cx| {
        if let Event::RepoWatchFailed { root, error } = event {
            failed(root, error, cx);
        }
    });
    cx.default_global::<LiveUpdates>()._events = Some(events);
}

fn failed(root: &str, error: &str, cx: &mut App) {
    if !cx
        .default_global::<LiveUpdates>()
        .told
        .insert(root.to_string())
    {
        return;
    }
    let name = Path::new(root)
        .file_name()
        .map_or_else(|| root.to_string(), |n| n.to_string_lossy().into_owned());
    let root = root.to_string();
    toast(
        Toast::warning(format!("Live updates are off for {name}: {error}"))
            .action("Retry", move |_, cx| retry(root.clone(), cx)),
        cx,
    );
}

/// Off the UI thread, as the watcher's start waits on fseventsd. A retry that fails says so again.
fn retry(root: String, cx: &mut App) {
    cx.default_global::<LiveUpdates>().told.remove(&root);
    let core = gitviber_ui::core(cx);
    cx.background_spawn(async move { api::repo::watch_again(&core, root) })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::{
        AnyWindowHandle, Context, IntoElement, Render, TestAppContext, Window, WindowOptions, div,
    };

    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn toasts(window: AnyWindowHandle, cx: &mut TestAppContext) -> usize {
        window
            .update(cx, |_, window, cx| window.notifications(cx).len())
            .unwrap()
    }

    fn fail(root: &str, cx: &mut TestAppContext) {
        cx.update(|cx| {
            CoreEvents::global(cx).update(cx, |_, cx| {
                cx.emit(Event::RepoWatchFailed {
                    root: root.into(),
                    error: "No path was found.".into(),
                })
            })
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn a_repo_without_live_updates_is_said_once(cx: &mut TestAppContext) {
        gitviber_ui::test_support::config_dir("live-updates");
        cx.update(gitviber_ui::init);
        let window = cx.update(|cx| {
            gitviber_ui::install_core(cx);
            install(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Host))
                .unwrap()
                .0
        });
        fail("/repos/a", cx);
        assert_eq!(toasts(window, cx), 1);
        // Switched away and back: the watcher fails again, and nothing new is said.
        fail("/repos/a", cx);
        assert_eq!(toasts(window, cx), 1);
        fail("/repos/b", cx);
        assert_eq!(toasts(window, cx), 2);
    }
}
