//! Paths handed to the app from outside its window: `gitviber <path>` and `<file>:<line>`, a folder
//! dropped on the Dock icon or opened with Finder's Open With, a second launch on Linux. Core
//! queues them (its opened.rs) and says so with Event::Opened; they're taken off the UI thread,
//! as a stalled network volume can take seconds to answer, and the last one's project opens in the
//! main window, with its file at its line.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gitviber_core::opened::{Taken, Target};
use gitviber_core::{Core, Event, api, git};
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

/// Opens what the launch named, else the worktree used last while it's still a repository (see
/// Reopen); from then on, whatever is handed over.
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
    let reopen = launch.then(|| Reopen::from(UiState::get(cx)));
    let found = match before {
        // After the take before it, so that what this one opens lands last.
        Some(before) => cx.spawn(async move |cx| {
            before.await;
            cx.background_spawn(async move { resolve(&core, reopen) })
                .await
        }),
        // The launch's own starts now, and runs while the window is made.
        None => cx.background_spawn(async move { resolve(&core, reopen) }),
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
    /// What a launch reopens, by its top level; looked for only when nothing was handed over.
    recent: Option<PathBuf>,
}

/// What was handed over and where it is. Reads the disk and asks git: a gone network mount can
/// take seconds to say so.
fn resolve(core: &Core, reopen: Option<Reopen>) -> Resolved {
    let taken = api::app::take_opened(core).unwrap_or_default();
    let asked = taken
        .open
        .last()
        .map(|t| open_repo::look(Path::new(&t.folder)));
    let recent = reopen
        .filter(|_| taken.open.is_empty())
        .and_then(Reopen::find);
    Resolved {
        taken,
        asked,
        recent,
    }
}

/// What a launch with nothing handed over reopens: the worktree open at quit, a linked one too,
/// else the projects in the list's order. A project's open saves its worktree's tabs first and
/// puts the project first in the list, so the two name the same project, unless the saved tabs
/// came from the old app's import, whose order is the alphabet's.
#[derive(Debug, PartialEq)]
struct Reopen {
    last: Option<PathBuf>,
    projects: Vec<PathBuf>,
}

impl From<&UiState> for Reopen {
    fn from(state: &UiState) -> Self {
        Reopen {
            last: state.workspaces.first().map(|w| PathBuf::from(&w.root)),
            projects: state.recent_projects.iter().map(PathBuf::from).collect(),
        }
    }
}

impl Reopen {
    /// The first that's still a repository: the last worktree while it's one of the first
    /// project's, else a project, the first project when it's there. Asks git: off the UI thread.
    fn find(self) -> Option<PathBuf> {
        let project = self.projects.first()?;
        let repo = |p: &Path| match open_repo::look(p) {
            Found::Repo(top) => Some(top),
            _ => None,
        };
        if let Some(top) = self.last.as_deref().filter(|l| l != project).and_then(repo) {
            let main = git::main_worktree(&top).map_or_else(|| top.clone(), PathBuf::from);
            if main == *project {
                return Some(top);
            }
        }
        self.projects.iter().find_map(|p| repo(p))
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
        gitviber_ui::test_support::config_dir("launch");
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

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "git {args:?}");
    }

    fn repo(at: &Path) -> PathBuf {
        std::fs::create_dir_all(at).unwrap();
        git(at, &["init", "-q", "-b", "main"]);
        git(
            at,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "x",
            ],
        );
        at.canonicalize().unwrap()
    }

    /// The worktree quit in comes back, a linked one too. Once it's gone, its project; with that
    /// gone too, the next project in the list, not the welcome screen.
    #[test]
    fn a_launch_reopens_the_worktree_quit_in_else_the_next_project_there() {
        let dir = gitviber_ui::test_support::temp_dir("reopen");
        let app = repo(&dir.join("app"));
        let site = repo(&dir.join("site"));
        let fix = app.join(".claude/worktrees/fix");
        git(
            &app,
            &["worktree", "add", "-q", "-b", "fix", &fix.to_string_lossy()],
        );
        let fix = fix.canonicalize().unwrap();
        let reopen = |last: &Path, projects: &[&Path]| {
            Reopen {
                last: Some(last.to_path_buf()),
                projects: projects.iter().map(|p| p.to_path_buf()).collect(),
            }
            .find()
        };
        assert_eq!(reopen(&fix, &[&app, &site]), Some(fix.clone()));
        assert_eq!(reopen(&app, &[&app, &site]), Some(app.clone()));
        // Tabs saved last under another project's worktree (the old app's import): the project.
        assert_eq!(reopen(&site, &[&app, &site]), Some(app.clone()));
        assert_eq!(reopen(&fix, &[&site, &app]), Some(site.clone()));
        std::fs::remove_dir_all(&fix).unwrap();
        assert_eq!(reopen(&fix, &[&app, &site]), Some(app.clone()));
        std::fs::remove_dir_all(&app).unwrap();
        assert_eq!(reopen(&app, &[&app, &site]), Some(site.clone()));
        assert_eq!(reopen(&fix, &[&app]), None);
        assert_eq!(reopen(&fix, &[]), None);
    }

    #[test]
    fn the_launch_reads_the_last_worktree_and_the_list_from_the_state() {
        let state = UiState {
            recent_projects: vec!["/r/app".into(), "/r/site".into()],
            workspaces: vec![
                gitviber_ui::state::SavedWorkspace {
                    root: "/r/app/.claude/worktrees/fix".into(),
                    ..Default::default()
                },
                gitviber_ui::state::SavedWorkspace {
                    root: "/r/app".into(),
                    ..Default::default()
                },
            ],
            ..UiState::default()
        };
        assert_eq!(
            Reopen::from(&state),
            Reopen {
                last: Some("/r/app/.claude/worktrees/fix".into()),
                projects: vec!["/r/app".into(), "/r/site".into()],
            }
        );
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
