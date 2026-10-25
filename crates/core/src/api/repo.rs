use crate::state::{Core, Res};
use crate::{fs, git, github, network, shell, watch};
use std::path::{Path, PathBuf};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedRepo {
    pub root: String,
    /// The main worktree; the projects list is keyed by it, so agent worktrees don't pile up there.
    pub main: String,
}

pub fn open_repo(core: &Core, path: String) -> Res<OpenedRepo> {
    let root = git::toplevel(&PathBuf::from(path))?;
    let root_path = PathBuf::from(&root);
    // A new watcher even for the open repo: one deleted and made again at its path left the old
    // one watching nothing (inotify). It comes up on a thread of its own, while this goes on.
    let watcher = watch::start(core.events(), root_path.clone());
    let main = git::main_worktree(Path::new(&root)).unwrap_or_else(|| root.clone());
    *core.watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(watcher);
    // gh's active account until the page sends this project's own (gh_use_account): one
    // project's pick never carries into the next. Before the repo switches, so none goes out as both.
    // Already open (`gitviber .` in its own terminal), it keeps its account.
    let open = core.repo.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if open.as_ref() != Some(&root_path) {
        github::use_account(&core.github, None)?;
    }
    *core.repo.lock().unwrap_or_else(|e| e.into_inner()) = Some(root_path);
    *core.dirs.lock().unwrap_or_else(|e| e.into_inner()) = None;
    // The UI reads from here on: a watcher still coming up catches up once it's up.
    if let Some(w) = &*core.watcher.lock().unwrap_or_else(|e| e.into_inner()) {
        w.opened();
    }
    Ok(OpenedRepo { root, main })
}

/// Starts the watcher of the repo at `root` again, after Event::RepoWatchFailed. Once it's up,
/// one refresh catches up what changed meanwhile. A repo opened or closed since keeps its own.
pub fn watch_again(core: &Core, root: String) {
    let root = PathBuf::from(root);
    let mut watcher = core.watcher.lock().unwrap_or_else(|e| e.into_inner());
    if watcher.as_ref().is_some_and(|w| w.root() == root) {
        let again = watch::start(core.events(), root);
        again.opened();
        *watcher = Some(again);
    }
}

/// Closes the open repo: its watcher stops, and commands find no repo open until the next
/// `open_repo`.
pub fn close_repo(core: &Core) -> Res<()> {
    *core.watcher.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *core.repo.lock().unwrap_or_else(|e| e.into_inner()) = None;
    *core.dirs.lock().unwrap_or_else(|e| e.into_inner()) = None;
    Ok(())
}

pub fn git_info(core: &Core, recheck: bool) -> Res<git::GitInfo> {
    if let Some(info) = core
        .git
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .filter(|_| !recheck)
    {
        return Ok(info);
    }
    // A git only the login shell's PATH has (MacPorts, nix, one just installed there)
    // shows up once that's read: at launch by waiting for it, on "Check again" by
    // asking the shell afresh.
    if recheck {
        shell::reprobe();
    }
    let before = shell::generation();
    let mut info = git::check_install();
    if !recheck && matches!(info.state, "missing" | "tools") {
        shell::wait_for_first_answer();
        if shell::generation() != before {
            info = git::check_install();
        }
    }
    *core.git.lock().unwrap_or_else(|e| e.into_inner()) = Some(info.clone());
    Ok(info)
}

pub fn install_git(_core: &Core) -> Res<()> {
    git::install_tools()
}

#[derive(serde::Serialize)]
pub struct IdentityCheck {
    pub current: git::Identity,
    /// From the GitHub account, when one is signed in already.
    pub suggested: Option<git::Identity>,
}

pub fn git_identity(core: &Core) -> Res<IdentityCheck> {
    let r = core.repo()?;
    let current = git::identity(&r);
    let complete = current.name.is_some() && current.email.is_some();
    let suggested = (!complete)
        .then(|| github::profile(&core.github, &r))
        .flatten();
    Ok(IdentityCheck { current, suggested })
}

/// Only the parts given are written, to the global config: the repo's own stays untouched.
pub fn set_git_identity(core: &Core, name: Option<String>, email: Option<String>) -> Res<()> {
    core.in_repo(move |r| git::set_global_identity(r, name.as_deref(), email.as_deref()))
}

pub fn remote_list(core: &Core) -> Res<Vec<git::Remote>> {
    core.in_repo(git::remote_list)
}

/// `action`: "add" (name, url), "remove" (name), "rename" (name, to) or "set-url" (name, url).
pub fn remote_edit(core: &Core, action: String, name: String, value: Option<String>) -> Res<()> {
    core.in_repo(move |r| {
        let value = || value.as_deref().ok_or("missing value");
        match action.as_str() {
            "add" => git::remote_add(r, &name, value()?),
            "remove" => git::remote_remove(r, &name),
            "rename" => git::remote_rename(r, &name, value()?),
            "set-url" => git::remote_set_url(r, &name, value()?),
            other => Err(format!("unknown remote action: {other}")),
        }
    })
}

/// This repository's own identity, and the global one it would use without it.
#[derive(serde::Serialize)]
pub struct RepoIdentity {
    pub own: git::Identity,
    pub global: git::Identity,
}

pub fn repo_identity(core: &Core) -> Res<RepoIdentity> {
    core.in_repo(move |r| {
        Ok(RepoIdentity {
            own: git::repo_identity(r),
            global: git::global_identity(r),
        })
    })
}

/// Both parts, or neither: the repository's own identity then goes and the global one applies.
pub fn set_repo_identity(core: &Core, name: Option<String>, email: Option<String>) -> Res<()> {
    core.in_repo(move |r| match (&name, &email) {
        (Some(n), Some(e)) => git::set_repo_identity(r, Some((n, e))),
        _ => git::set_repo_identity(r, None),
    })
}

#[derive(serde::Serialize)]
pub struct ProjectInfo {
    /// False once the folder was moved or deleted.
    pub exists: bool,
    /// owner/name of its GitHub origin
    pub github: Option<String>,
}

/// For the projects list: which saved folders are still there, and where each lives on GitHub.
pub fn project_info(_core: &Core, paths: Vec<String>) -> Res<Vec<ProjectInfo>> {
    Ok(paths
        .iter()
        .map(|p| {
            let path = Path::new(p);
            let exists = path.is_dir();
            ProjectInfo {
                exists,
                github: exists.then(|| github::origin_repo(path)).flatten(),
            }
        })
        .collect())
}

/// The branch of a folder outside the open repo, for a terminal tab there.
pub fn folder_branch(_core: &Core, path: String) -> Res<Option<String>> {
    Ok(git::current_branch(Path::new(&path)))
}

/// Reveals a saved project, open or not. reveal_path is confined to the open repo; this only
/// takes the top folder of a git worktree, so the page can't point Finder anywhere else.
pub fn reveal_project(_core: &Core, path: String) -> Res<()> {
    let top = git::toplevel(Path::new(&path))?;
    if Path::new(&top) != Path::new(&path) {
        return Err(format!("not a project folder: {path}"));
    }
    fs::reveal(Path::new(&top), "")
}

/// Like open_repo, any folder the user picked: the clone lands in `parent/name`.
pub fn clone_repo(
    core: &Core,
    url: String,
    parent: String,
    name: String,
    op: String,
    progress: impl Fn(network::Progress) + Send + Sync + 'static,
) -> Res<String> {
    let net = core.watch_network(op, progress);
    git::clone(Path::new(&parent), &url, &name, &net)
}

/// After the user said to trust a repository git refused for its owner; see git::trust_folder.
pub fn trust_folder(_core: &Core, path: String) -> Res<()> {
    git::trust_folder(Path::new(&path))
}

pub fn init_repo(_core: &Core, path: String) -> Res<()> {
    git::init(Path::new(&path))
}

#[cfg(test)]
mod close_tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn closing_leaves_no_repo_open() {
        let dir = std::env::temp_dir().join(format!("gitviber-close-repo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let init = std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&dir)
            .status();
        let core = Core::new(Arc::new(|_| {}));
        if init.is_ok_and(|s| s.success()) {
            open_repo(&core, dir.to_string_lossy().into_owned()).unwrap();
            assert!(core.repo().is_ok());
            assert!(core.watcher.lock().unwrap().is_some());
        }
        close_repo(&core).unwrap();
        assert!(core.repo().is_err());
        assert!(core.watcher.lock().unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod open_tests {
    use super::*;
    use crate::events::Event;
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;

    fn repo(name: &str) -> Option<PathBuf> {
        let dir = std::env::temp_dir().join(format!("gitviber-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        let init = std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(&dir)
            .status();
        init.is_ok_and(|s| s.success())
            .then(|| dir.canonicalize().unwrap())
    }

    /// A core whose RepoChanged events come out of the receiver.
    fn core() -> (Arc<Core>, mpsc::Receiver<watch::RepoChanged>) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let core = Core::new(Arc::new(move |e| {
            if let Event::RepoChanged(change) = e {
                let _ = tx.lock().unwrap().send(change);
            }
        }));
        (Arc::new(core), rx)
    }

    /// open_repo on a thread of its own: one that waited for a held watcher would never return.
    fn open(core: &Arc<Core>, dir: &Path) -> OpenedRepo {
        let (tx, rx) = mpsc::channel();
        let (core, dir) = (core.clone(), dir.to_string_lossy().into_owned());
        std::thread::spawn(move || tx.send(open_repo(&core, dir)));
        rx.recv_timeout(Duration::from_secs(20))
            .expect("open_repo waited for the watcher")
            .unwrap()
    }

    /// fseventsd under load took 1–10 s to start a watcher. The repo opens without it, and what
    /// changed in the meantime comes in with one refresh once it's up.
    #[test]
    fn a_repo_opens_before_its_watcher_is_up() {
        let Some(dir) = repo("slow-watcher") else {
            return;
        };
        let (core, rx) = core();
        let held = watch::held::hold(&dir);
        assert_eq!(Path::new(&open(&core, &dir).root), dir);
        // No watcher hears this one.
        std::fs::write(dir.join("early.txt"), "a").unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        drop(held);
        let caught = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("a catch-up");
        assert!(caught.git && caught.worktree, "{caught:?}");
        assert_eq!(Path::new(&caught.root), dir);
        let status = crate::api::changes::status(&core).unwrap();
        assert!(status.unstaged.iter().any(|f| f.path == "early.txt"));
        // From then on the watcher hears writes, and that was the only catch-up.
        std::fs::write(dir.join("late.txt"), "b").unwrap();
        let heard = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("the write heard");
        assert!(heard.worktree && !heard.git, "{heard:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A watcher that can't start (its folder went away before it was up): the repo stays open,
    /// the app hears why, and watch_again starts it once there's something to watch.
    #[test]
    fn a_watcher_that_fails_says_so_and_starts_again() {
        let Some(dir) = repo("failed-watcher") else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let core = Arc::new(Core::new(Arc::new(move |e| {
            if matches!(e, Event::RepoChanged(_) | Event::RepoWatchFailed { .. }) {
                let _ = tx.lock().unwrap().send(e);
            }
        })));
        let held = watch::held::hold(&dir);
        open(&core, &dir);
        let away = dir.with_extension("away");
        std::fs::rename(&dir, &away).unwrap();
        drop(held);
        match rx
            .recv_timeout(Duration::from_secs(20))
            .expect("the failure")
        {
            Event::RepoWatchFailed { root, error } => {
                assert_eq!(Path::new(&root), dir);
                assert!(!error.is_empty() && !error.contains(&root), "{error}");
            }
            other => panic!("{other:?}"),
        }
        assert!(core.repo().is_ok());
        std::fs::rename(&away, &dir).unwrap();
        // Not the open repo: nothing starts. One that did would fail at once, and say so.
        watch_again(&core, "/nowhere".into());
        assert!(rx.recv_timeout(Duration::from_secs(1)).is_err());
        watch_again(&core, dir.to_string_lossy().into_owned());
        // Sent once the watcher is up.
        match rx
            .recv_timeout(Duration::from_secs(20))
            .expect("a catch-up")
        {
            Event::RepoChanged(c) => assert!(c.git && c.worktree && Path::new(&c.root) == dir),
            other => panic!("{other:?}"),
        }
        close_repo(&core).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Switched away from (or closed) before its watcher was up: that repo tells nothing, ever.
    #[test]
    fn a_watcher_left_before_it_is_up_tells_nothing() {
        let (Some(a), Some(b)) = (repo("left-a"), repo("left-b")) else {
            return;
        };
        let (core, rx) = core();
        let held = watch::held::hold(&a);
        open(&core, &a);
        open(&core, &b);
        drop(held);
        std::fs::write(a.join("after.txt"), "a").unwrap();
        std::fs::write(b.join("after.txt"), "b").unwrap();
        // b's catch-up or its write, however long fseventsd takes; then whatever else comes.
        let mut heard = vec![];
        while !heard.contains(&b) {
            let change = rx
                .recv_timeout(Duration::from_secs(20))
                .expect("b's watcher");
            heard.push(PathBuf::from(change.root));
        }
        heard.extend(
            std::iter::from_fn(|| rx.recv_timeout(Duration::from_secs(1)).ok())
                .map(|c| PathBuf::from(c.root)),
        );
        assert!(!heard.contains(&a), "a told: {heard:?}");
        close_repo(&core).unwrap();
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }
}
