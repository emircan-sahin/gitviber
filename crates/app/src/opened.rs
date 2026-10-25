//! Paths handed to the app from outside its window: `gitviber <path>` and `<file>:<line>`, a folder
//! dropped on the Dock icon or opened with Finder's Open With, a second launch on Linux. Core
//! queues them (its opened.rs) and says so with Event::Opened; they're taken off the UI thread,
//! as a stalled network volume can take seconds to answer, and the last one's project opens in the
//! main window, with its file at its line.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gitviber_core::opened::{Taken, Target};
use gitviber_core::{Core, Event, api};
use gitviber_files::Reveal;
use gitviber_ui::workspace::Toast;
use gitviber_ui::{CoreEvents, UiState};
use gpui_kit::{App, AppContext as _, Global, Subscription, Task};

use crate::help::toast;
use crate::open_repo::{self, Found, Request};

/// Past this, a launch still looking for its project (a stalled network volume) shows the
/// welcome screen rather than an empty window.
const LAUNCH_WAIT: Duration = Duration::from_millis(400);

#[derive(Default)]
struct Opened {
    _events: Option<Subscription>,
    /// The launch's own take, which a later one waits for: it must not open the last project
    /// over what was asked meanwhile.
    taking: Option<Task<()>>,
    /// The launch is still finding what to open: the window shows no welcome screen, which
    /// would flash before the last project.
    launching: bool,
    _launch_wait: Option<Task<()>>,
}

impl Global for Opened {}

/// Opens what the launch named, else the project used last while it's still a repository; from
/// then on, whatever is handed over.
pub fn install(cx: &mut App) {
    if !gitviber_ui::has_core(cx) {
        return;
    }
    let events = cx.subscribe(&CoreEvents::global(cx), |_, event: &Event, cx| {
        if let Event::Opened = event {
            take(false, cx);
        }
    });
    cx.default_global::<Opened>()._events = Some(events);
    wait_for_launch(cx);
    take(true, cx);
}

/// No welcome screen until the launch's take says whether a project opens, LAUNCH_WAIT at most.
fn wait_for_launch(cx: &mut App) {
    let wait = cx.spawn(async move |cx| {
        cx.background_executor().timer(LAUNCH_WAIT).await;
        cx.update(launched);
    });
    let opened = cx.default_global::<Opened>();
    opened.launching = true;
    opened._launch_wait = Some(wait);
}

/// The launch is still finding what to open (briefly): no welcome screen yet.
pub fn launching(cx: &App) -> bool {
    cx.try_global::<Opened>().is_some_and(|o| o.launching)
}

fn launched(cx: &mut App) {
    let opened = cx.default_global::<Opened>();
    opened._launch_wait = None;
    if std::mem::take(&mut opened.launching) {
        cx.refresh_windows();
    }
}

/// macOS's open-documents event (Dock, Finder, `open -a`), as file URLs.
pub fn open_urls(urls: Vec<String>, cx: &mut App) {
    let paths: Vec<String> = urls.iter().filter_map(|u| file_path(u)).collect();
    if paths.is_empty() || !gitviber_ui::has_core(cx) {
        return;
    }
    api::app::open_dropped(&gitviber_ui::core(cx), paths);
}

/// `file:///a%20b/c` → `/a b/c`; other schemes aren't paths.
pub fn file_path(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = rest
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok().filter(|p| p.starts_with('/'))
}

/// What to open of a take: the last target (one opens at a time), and what to say about the rest.
pub fn plan(open: &[Target], missing: &[String]) -> (Option<usize>, Vec<Toast>) {
    let mut toasts: Vec<Toast> = missing
        .iter()
        .map(|p| Toast::error(p.clone()).title("No such file or folder"))
        .collect();
    let last = open.len().checked_sub(1);
    if let Some(last) = last
        && open.len() > 1
    {
        let t = &open[last];
        let name = t
            .file
            .as_deref()
            .unwrap_or(&t.folder)
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();
        toasts.push(
            Toast::info(format!(
                "The last of the {} given: one opens at a time.",
                open.len()
            ))
            .title(format!("Opened {name}")),
        );
    }
    (last, toasts)
}

fn take(launch: bool, cx: &mut App) {
    let core = gitviber_ui::core(cx);
    let before = cx.default_global::<Opened>().taking.take();
    let recent = launch
        .then(|| UiState::get(cx).recent_projects.first().cloned())
        .flatten();
    let found = match before {
        // After the take before it, so that what this one opens lands last.
        Some(before) => cx.spawn(async move |cx| {
            before.await;
            cx.background_spawn(async move { resolve(&core, recent) })
                .await
        }),
        // The launch's own starts now, and runs while the window is made.
        None => cx.background_spawn(async move { resolve(&core, recent) }),
    };
    let task = cx.spawn(async move |cx| {
        let resolved = found.await;
        cx.update(|cx| {
            let (last, toasts) = plan(&resolved.taken.open, &resolved.taken.missing);
            for t in toasts {
                toast(t, cx);
            }
            match (last.map(|i| &resolved.taken.open[i]), resolved.asked) {
                (Some(target), Some(found)) => open_repo::act(found, request(target), cx),
                _ => {
                    if let Some(root) = resolved.recent {
                        open_repo::act(Found::Repo(root.clone()), Request::folder(root), cx);
                    }
                }
            }
            if launch {
                launched(cx);
            }
        });
    });
    cx.default_global::<Opened>().taking = Some(task);
}

struct Resolved {
    taken: Taken,
    /// What git says the last target's folder is in.
    asked: Option<Found>,
    /// The project used last, by its top level, while it's still a repository; looked for only
    /// when nothing was handed over.
    recent: Option<PathBuf>,
}

/// What was handed over and where it is. Reads the disk and asks git: a gone network mount can
/// take seconds to say so.
fn resolve(core: &Core, recent: Option<String>) -> Resolved {
    let taken = api::app::take_opened(core).unwrap_or_default();
    let asked = taken
        .open
        .last()
        .map(|t| open_repo::look(Path::new(&t.folder)));
    let recent = recent.filter(|_| taken.open.is_empty()).and_then(|p| {
        match open_repo::look(Path::new(&p)) {
            Found::Repo(root) => Some(root),
            _ => None,
        }
    });
    Resolved {
        taken,
        asked,
        recent,
    }
}

/// The folder a target names, with its file at its line.
fn request(t: &Target) -> Request {
    let file = t.file.clone().map(|path| {
        let line = t.line.unwrap_or(1);
        let column = t.column.unwrap_or(1).saturating_sub(1);
        (path, t.line.map(|_| Reveal::Line { line, column }))
    });
    Request {
        folder: PathBuf::from(&t.folder),
        file,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{AnyWindowHandle, TestAppContext, VisualTestContext};

    fn target(folder: &str, file: Option<&str>) -> Target {
        Target {
            folder: folder.into(),
            file: file.map(Into::into),
            line: None,
            column: None,
        }
    }

    /// The window's first frames show no welcome screen while the launch looks for the last
    /// project: it would flash before it. Past LAUNCH_WAIT (a stalled volume), the welcome shows.
    #[gpui_kit::test]
    fn the_welcome_screen_waits_for_the_launch_to_say_what_opens(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("gitviber-launch-{}", std::process::id()));
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(|cx| {
            gitviber_ui::init(cx);
            crate::placeholders::register(cx);
        });
        let welcome = |cx: &mut TestAppContext| {
            let window: AnyWindowHandle = cx
                .add_window(|window, cx| {
                    let view = crate::workspace_view::main_view(None, window, cx);
                    gpui_kit::base::Root::new(view, window, cx)
                })
                .into();
            VisualTestContext::from_window(window, cx)
        };
        cx.update(wait_for_launch);
        let mut vcx = welcome(cx);
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("welcome").is_none(),
            "nothing while it looks"
        );
        vcx.update(|_, cx| launched(cx));
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("welcome").is_some(),
            "found nothing: the welcome"
        );
        // A launch that takes too long shows it anyway.
        cx.update(wait_for_launch);
        let mut vcx = welcome(cx);
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("welcome").is_none());
        vcx.executor().advance_clock(LAUNCH_WAIT);
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("welcome").is_some());
    }

    #[test]
    fn file_urls_become_paths() {
        assert_eq!(
            file_path("file:///Users/me/a%20b").as_deref(),
            Some("/Users/me/a b")
        );
        assert_eq!(
            file_path("file://localhost/tmp/x").as_deref(),
            Some("/tmp/x")
        );
        assert_eq!(file_path("https://example.com/x"), None);
        assert_eq!(file_path("file://relative"), None);
    }

    #[test]
    fn the_last_target_opens_and_the_rest_are_named() {
        let (last, toasts) = plan(&[], &["/gone".into()]);
        assert_eq!(last, None);
        assert_eq!(toasts.len(), 1);
        assert_eq!(toasts[0].message.as_ref(), "/gone");
        let open = [target("/a", None), target("/b", Some("src/c.rs"))];
        let (last, toasts) = plan(&open, &[]);
        assert_eq!(last, Some(1));
        assert_eq!(toasts[0].title.as_deref(), Some("Opened c.rs"));
        let (last, toasts) = plan(&open[..1], &[]);
        assert_eq!(last, Some(0));
        assert!(toasts.is_empty());
    }
}
