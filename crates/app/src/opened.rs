//! Paths handed to the app from outside its window: `gitviber <path>` and `<file>:<line>`, a folder
//! dropped on the Dock icon or opened with Finder's Open With, a second launch on Linux. Core
//! queues them (its opened.rs) and says so with Event::Opened; they're taken off the UI thread,
//! as a stalled network volume can take seconds to answer, and the last one's project opens in the
//! main window, with its file at its line.

use std::path::PathBuf;

use gitviber_core::opened::Target;
use gitviber_core::{Event, api};
use gitviber_files::{OpenOptions, Reveal};
use gitviber_ui::workspace::Toast;
use gitviber_ui::{CoreEvents, UiState, Workspace};
use gpui_kit::{App, AppContext as _, Global, Subscription, Task};

use crate::help::toast;

#[derive(Default)]
struct Opened {
    _events: Option<Subscription>,
    /// The launch's own take, which a later one waits for: it must not open the last project
    /// over what was asked meanwhile.
    taking: Option<Task<()>>,
}

impl Global for Opened {}

/// Opens what the launch named, else the project used last while it's still a folder; from then
/// on, whatever is handed over.
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
    take(true, cx);
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
    let task = cx.spawn(async move |cx| {
        if let Some(before) = before {
            before.await;
        }
        let (taken, recent) = cx
            .background_spawn(async move {
                let taken = api::app::take_opened(&core).unwrap_or_default();
                // A gone network mount can take seconds to say so: not on the UI thread.
                let recent = recent
                    .map(PathBuf::from)
                    .filter(|p| p.is_dir())
                    .and_then(|p| p.canonicalize().ok());
                (taken, recent)
            })
            .await;
        cx.update(|cx| {
            let (last, toasts) = plan(&taken.open, &taken.missing);
            for t in toasts {
                toast(t, cx);
            }
            match last.map(|i| &taken.open[i]) {
                Some(target) => open_target(target, cx),
                None => {
                    if let Some(root) = recent {
                        open_folder(root, None, cx);
                    }
                }
            }
        });
    });
    cx.default_global::<Opened>().taking = Some(task);
}

fn open_target(t: &Target, cx: &mut App) {
    let file = t.file.clone().map(|path| {
        let line = t.line.unwrap_or(1);
        let column = t.column.unwrap_or(1).saturating_sub(1);
        (path, t.line.map(|_| Reveal::Line { line, column }))
    });
    open_folder(PathBuf::from(&t.folder), file, cx);
}

/// Opens `root` in the main window (brought forward), then `file` in it: a tab opened while the
/// repo opens stays.
fn open_folder(root: PathBuf, file: Option<(String, Option<Reveal>)>, cx: &mut App) {
    let Some(handle) = crate::workspace_view::main_window(cx) else {
        return;
    };
    let _ = handle.update(cx, |_, window, cx| {
        window.activate_window();
        let Some(ws) = Workspace::for_window(window, cx) else {
            return;
        };
        ws.update(cx, |ws, cx| ws.open_project(root, window, cx));
        if let Some((path, reveal)) = file {
            gitviber_files::open_file(
                &ws.downgrade(),
                &path,
                OpenOptions {
                    preview: false,
                    reveal,
                    focus: true,
                },
                window,
                cx,
            );
        }
    });
    if !gitviber_ui::windows::background_launch() {
        cx.activate(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(folder: &str, file: Option<&str>) -> Target {
        Target {
            folder: folder.into(),
            file: file.map(Into::into),
            line: None,
            column: None,
        }
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
