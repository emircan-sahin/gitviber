//! Opening a folder as the main window's repository: File → Open Repository…, Open Recent, a
//! project's row, a drop, `gitviber <path>`. git is asked first, off the UI thread, so a folder
//! that isn't one leaves the open repository as it was: a folder offers Initialize, a file's
//! folder says so, and one git refuses for its owner offers Trust this folder. A file shows once
//! its repository is open, after any unsaved edits the switch would drop were settled.

use std::path::{Path, PathBuf};

use gitviber_core::git;
use gitviber_files::{OpenOptions, Reveal};
use gitviber_refs::ask::{Ask, ask};
use gitviber_refs::errors::GitFix;
use gitviber_refs::toast::{Action, git_failed};
use gitviber_ui::Workspace;
use gitviber_ui::workspace::{Toast, settle_edits};
use gpui_kit::{App, AppContext as _};

use crate::help::toast;
use crate::projects::folder_name;

/// What git says a folder is in.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    /// A repository, by its worktree's top level.
    Repo(PathBuf),
    Gone,
    NotARepo,
    /// git's own words: a refusal for the folder's owner, a broken config.
    Failed(String),
}

/// Asks git about `folder`. Blocks on git (a stalled network volume, seconds): off the UI thread.
pub fn look(folder: &Path) -> Found {
    if !folder.is_dir() {
        return Found::Gone;
    }
    match git::toplevel(folder) {
        Ok(top) => Found::Repo(PathBuf::from(top)),
        Err(e) if e == git::NOT_A_REPO => Found::NotARepo,
        Err(e) => Found::Failed(e),
    }
}

/// A folder to open, and a file in it: its path from the repository's root, and where in it.
#[derive(Clone)]
pub struct Request {
    pub folder: PathBuf,
    pub file: Option<(String, Option<Reveal>)>,
}

impl Request {
    pub fn folder(folder: impl Into<PathBuf>) -> Self {
        Request {
            folder: folder.into(),
            file: None,
        }
    }
}

/// Opens `request` once git has said what its folder is.
pub fn open(request: Request, cx: &mut App) {
    let folder = request.folder.clone();
    let found = cx.background_spawn(async move { look(&folder) });
    cx.spawn(async move |cx| {
        let found = found.await;
        cx.update(|cx| act(found, request, cx));
    })
    .detach();
}

/// What git said, acted on in the main window.
pub fn act(found: Found, request: Request, cx: &mut App) {
    let name = folder_name(&request.folder.to_string_lossy()).to_string();
    match found {
        Found::Repo(root) => show(root, request.file, cx),
        Found::Gone => toast(
            Toast::error(request.folder.to_string_lossy().into_owned())
                .title("No such file or folder"),
            cx,
        ),
        // A file's folder isn't one to make a repository of: one dropped from the desktop would ask.
        Found::NotARepo if request.file.is_some() => {
            toast(Toast::info(format!("{name} isn't in a git repository")), cx)
        }
        Found::NotARepo => offer_init(request.folder, name, cx),
        Found::Failed(err) => failed(&err, request, cx),
    }
}

/// `root` in the main window, brought forward, and `file` in it. Another repository drops the
/// tabs: their unsaved edits are asked about first, and nothing opens until they're settled.
fn show(root: PathBuf, file: Option<(String, Option<Reveal>)>, cx: &mut App) {
    let Some(handle) = crate::workspace_view::main_window(cx) else {
        return;
    };
    let _ = handle.update(cx, |_, window, cx| {
        window.activate_window();
        let Some(ws) = Workspace::for_window(window, cx) else {
            return;
        };
        let switching = ws
            .read(cx)
            .project()
            .is_none_or(|p| p.read(cx).root() != root);
        let dirty = if switching {
            ws.read(cx).unkept_dirty_tabs(cx)
        } else {
            Vec::new()
        };
        if let Some(settle) = settle_edits(cx).filter(|_| !dirty.is_empty()) {
            let weak = ws.downgrade();
            // After this update: the ask opens in this window.
            cx.defer(move |cx| {
                let then = move |go: bool, cx: &mut App| {
                    if go {
                        cx.defer(move |cx| show(root, file, cx));
                    }
                };
                settle(dirty, weak, Box::new(then), cx)
            });
            return;
        }
        ws.update(cx, |ws, cx| ws.open_project(root.clone(), window, cx));
        // Not there: the switch was refused (an action still running), which said why.
        let here = ws
            .read(cx)
            .project()
            .is_some_and(|p| p.read(cx).root() == root);
        if let (true, Some((path, reveal))) = (here, file) {
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

fn offer_init(folder: PathBuf, name: String, cx: &mut App) {
    let Some(handle) = crate::workspace_view::main_window(cx) else {
        return;
    };
    let _ = handle.update(cx, |_, window, cx| {
        window.activate_window();
        let answer = ask(
            Ask::new(
                "Open Repository",
                format!("{name} isn't a git repository yet. Initialize one here?"),
                "Initialize Repository",
            ),
            window,
            cx,
        );
        cx.spawn(async move |cx| {
            if answer.await != Ok(true) {
                return;
            }
            let at = folder.clone();
            let made = cx.background_spawn(async move { git::init(&at) }).await;
            cx.update(|cx| match made {
                Ok(()) => open(Request::folder(folder), cx),
                Err(err) => toast(
                    Toast::error(err).title("Couldn't initialize the repository"),
                    cx,
                ),
            });
        })
        .detach();
    });
}

/// git's words, explained when it knows them; a refusal for the folder's owner offers Trust
/// this folder, which opens it once git takes it.
fn failed(err: &str, request: Request, cx: &mut App) {
    let Some(handle) = crate::workspace_view::main_window(cx) else {
        return;
    };
    let trust = Action::new("Trust this folder", move |_, cx| {
        let (request, at) = (request.clone(), request.folder.clone());
        let trusted = cx.background_spawn(async move { git::trust_folder(&at) });
        cx.spawn(async move |cx| {
            let trusted = trusted.await;
            cx.update(|cx| match trusted {
                Ok(()) => open(request, cx),
                Err(err) => toast(Toast::error(err).title("Couldn't trust the folder"), cx),
            });
        })
        .detach();
    });
    git_failed(
        "Couldn't open the repository",
        err,
        &vec![(GitFix::SafeDirectory, trust)],
    )
    .show_in(handle, cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use gitviber_ui::workspace::{
        Selection, TabHandle, TabItem, register_opener, set_settle_edits,
    };
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::{
        AnyWindowHandle, Context, IntoElement, Render, SharedString, TestAppContext,
        VisualTestContext, Window, div,
    };

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "git {args:?}");
    }

    /// A tab that may hold unsaved edits.
    struct Typed(Rc<Cell<bool>>);
    impl Render for Typed {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    impl TabItem for Typed {
        fn tab_title(&self, _: &App) -> SharedString {
            "typed".into()
        }
        fn is_dirty(&self, _: &App) -> bool {
            self.0.get()
        }
    }

    type Then = Box<dyn FnOnce(bool, &mut App)>;

    /// Outside the window's update, as a task or an event runs `act`.
    fn app(cx: &mut VisualTestContext) -> &mut TestAppContext {
        cx
    }

    /// `gitviber other/x.rs:5` with unsaved edits: nothing opens until they're settled, then the
    /// repository and its file. A folder of the open repository keeps the tabs; one that isn't a
    /// repository leaves the open one as it was.
    #[gpui_kit::test]
    fn a_file_in_another_repo_waits_for_the_unsaved_edits(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("gitviber-open-wait-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (a, b, plain) = (dir.join("a"), dir.join("b"), dir.join("plain"));
        for d in [&a, &b.join("sub"), &plain] {
            std::fs::create_dir_all(d).unwrap();
        }
        git(&a, &["init", "-q"]);
        git(&b, &["init", "-q"]);
        let (a, b) = (a.canonicalize().unwrap(), b.canonicalize().unwrap());
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", dir.join("config")) };
        let (asked, pending) = (Rc::new(Cell::new(0)), Rc::new(RefCell::new(None::<Then>)));
        let (count, park) = (asked.clone(), pending.clone());
        cx.update(|cx| {
            gitviber_ui::init(cx);
            crate::placeholders::register(cx);
            register_opener(cx, "file", |_, _, cx| {
                Some(TabHandle::new(cx.new(|_| Typed(Rc::default()))))
            });
            set_settle_edits(
                cx,
                Rc::new(move |_, _, then, _| {
                    count.set(count.get() + 1);
                    *park.borrow_mut() = Some(then);
                }),
            );
        });
        let window: AnyWindowHandle = cx
            .add_window(|window, cx| {
                let view = crate::workspace_view::main_view(Some(a.clone()), window, cx);
                gpui_kit::base::Root::new(view, window, cx)
            })
            .into();
        let cx = &mut VisualTestContext::from_window(window, cx);
        cx.run_until_parked();
        let ws = cx.update(|window, cx| Workspace::for_window(window, cx).unwrap());
        let dirty = Rc::new(Cell::new(true));
        let typed = |dirty: &Rc<Cell<bool>>, cx: &mut VisualTestContext| {
            let d = dirty.clone();
            cx.update(|window, cx| {
                ws.update(cx, |ws, cx| {
                    ws.open_tab(
                        "typed",
                        false,
                        move |_, cx| cx.new(|_| Typed(d)),
                        window,
                        cx,
                    )
                })
            });
        };
        typed(&dirty, cx);
        let root = |cx: &mut VisualTestContext| {
            ws.read_with(cx, |ws, cx| {
                ws.project().map(|p| p.read(cx).root().to_path_buf())
            })
        };
        let has = |path: &str, cx: &mut VisualTestContext| {
            let file = Selection::File { path: path.into() };
            ws.read_with(cx, |ws, _| ws.tab_for(&file).is_some())
        };
        let file = |folder: &Path, path: &str| Request {
            folder: folder.to_path_buf(),
            file: Some((path.to_string(), None)),
        };

        app(cx).update(|cx| act(look(&b), file(&b, "x.rs"), cx));
        cx.run_until_parked();
        assert_eq!(asked.get(), 1);
        assert_eq!(root(cx), Some(a.clone()), "nothing switched while it asks");
        assert!(!has("x.rs", cx), "nor opened in the old repository");
        dirty.set(false);
        let then = pending.borrow_mut().take().unwrap();
        app(cx).update(|cx| then(true, cx));
        cx.run_until_parked();
        assert_eq!(root(cx), Some(b.clone()));
        assert!(has("x.rs", cx), "the file once its repository is open");

        // A folder of the open repository: its tabs stay, unsaved ones too, and nothing asks.
        dirty.set(true);
        typed(&dirty, cx);
        app(cx).update(|cx| act(look(&b.join("sub")), file(&b.join("sub"), "sub/y.rs"), cx));
        cx.run_until_parked();
        assert_eq!(asked.get(), 1);
        assert!(has("sub/y.rs", cx) && has("x.rs", cx));

        // Not a repository: the open one stays; a file's folder only says so, a folder offers
        // Initialize.
        app(cx).update(|cx| act(look(&plain), file(&plain, "z.txt"), cx));
        cx.run_until_parked();
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        app(cx).update(|cx| act(look(&plain), Request::folder(&plain), cx));
        cx.run_until_parked();
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(root(cx), Some(b.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_is_asked_what_a_folder_is_in() {
        let dir = std::env::temp_dir().join(format!("gitviber-open-repo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (repo, plain) = (dir.join("repo"), dir.join("plain"));
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        std::fs::create_dir_all(&plain).unwrap();
        git(&repo, &["init", "-q"]);
        let repo = repo.canonicalize().unwrap();
        // A subfolder is its repository: the open one stays open, its tabs too.
        assert_eq!(look(&repo.join("sub")), Found::Repo(repo.clone()));
        assert_eq!(look(&plain), Found::NotARepo);
        assert_eq!(look(&dir.join("gone")), Found::Gone);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
