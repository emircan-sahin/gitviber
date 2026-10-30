use crate::state::{Core, Res};
use crate::{fs, git, github, network, shell, watch};
use std::path::{Path, PathBuf};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedRepo {
    pub root: String,
    /// The main worktree; the projects list is keyed by it, so agent worktrees don't pile up there.
    pub main: String,
    /// False for a folder outside any repository: it opens as a plain project (files, terminals,
    /// agents), and git's views offer `git init`.
    pub git: bool,
}

/// Opens the repository `path` is in, or `path` itself as a plain folder when it's in none.
pub fn open_repo(core: &Core, path: String) -> Res<OpenedRepo> {
    let path = PathBuf::from(path);
    let (root, git) = match git::toplevel(&path) {
        Ok(top) => (top, true),
        Err(e) if e == git::NOT_A_REPO => {
            let folder = path.canonicalize().map_err(|e| e.to_string())?;
            (folder.to_string_lossy().into_owned(), false)
        }
        Err(e) => return Err(e),
    };
    let root_path = PathBuf::from(&root);
    git::opened_as(&root_path, !git);
    // A new watcher even for the open repo: one deleted and made again at its path left the old
    // one watching nothing (inotify). It comes up on a thread of its own, while this goes on.
    let watcher = watch::start(core.events(), root_path.clone());
    let main = git
        .then(|| git::main_worktree(Path::new(&root)))
        .flatten()
        .unwrap_or_else(|| root.clone());
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
    Ok(OpenedRepo { root, main, git })
}

/// The path the projects list keys `path` by once it's opened, as open_repo finds it: its
/// repository's main worktree, else the folder itself, canonical. None when it's gone or git
/// refuses it. Blocks on git.
pub fn project_key(path: &Path) -> Option<String> {
    match git::toplevel(path) {
        Ok(top) => Some(git::main_worktree(Path::new(&top)).unwrap_or(top)),
        Err(e) if e == git::NOT_A_REPO => {
            Some(path.canonicalize().ok()?.to_string_lossy().into_owned())
        }
        Err(_) => None,
    }
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
/// takes the top folder of a git worktree, or a folder outside any repository (a plain
/// project), so the page can't point Finder into one's subfolders.
pub fn reveal_project(_core: &Core, path: String) -> Res<()> {
    let top = match git::toplevel(Path::new(&path)) {
        Err(e) if e == git::NOT_A_REPO => path.clone(),
        top => top?,
    };
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

/// `git init` in a plain folder; opening it again makes it the repository it is now.
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
        // Held until it's closed, so it never starts: fseventsd starts streams one at a time, and
        // the watcher tests wait behind each one.
        let _held = watch::held::hold(&dir.canonicalize().unwrap());
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
mod folder_tests {
    use super::*;
    use std::sync::Arc;

    /// A folder outside any repository opens as itself, without git; once `git init` ran there,
    /// opening it again finds the repository.
    #[test]
    fn a_plain_folder_opens_and_becomes_a_repo_after_init() {
        let dir = std::env::temp_dir().join(format!("gitviber-plain-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let dir = dir.canonicalize().unwrap();
        let core = Core::new(Arc::new(|_| {}));
        let _held = watch::held::hold(&dir);
        if git::toplevel(&dir).is_ok() {
            // The temp dir sits in a repository here: nothing to check.
            return;
        }
        let opened = open_repo(&core, dir.to_string_lossy().into_owned()).unwrap();
        assert!(!opened.git);
        assert_eq!(Path::new(&opened.root), dir);
        assert_eq!(opened.main, opened.root);
        assert_eq!(
            core.repo().unwrap(),
            dir,
            "files and terminals have their folder"
        );

        // Its files list, search and diff as a repository's would, .gitignore kept.
        std::fs::write(dir.join(".gitignore"), "build/\n").unwrap();
        std::fs::write(dir.join("sub/notes.txt"), "a plain idea\n").unwrap();
        std::fs::create_dir_all(dir.join("build")).unwrap();
        std::fs::write(dir.join("build/out.txt"), "a plain idea\n").unwrap();
        let mut files = crate::api::files::list_files(&core).unwrap();
        files.sort();
        assert_eq!(files, [".gitignore", "sub/notes.txt"]);
        let ignored = crate::api::files::list_ignored(&core).unwrap();
        assert_eq!(
            ignored.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            ["build"]
        );
        let query = crate::grep::Query {
            text: "plain idea".into(),
            ..Default::default()
        };
        let found = crate::api::files::search_files(&core, query).unwrap();
        let hits: Vec<&str> = found.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(hits, ["sub/notes.txt"]);
        let pair = crate::api::files::diff_pair(
            &core,
            "worktree".into(),
            "sub/notes.txt".into(),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(pair.modified.text.contains("plain idea"));
        assert!(pair.rows.iter().all(|r| r.k == 0), "no line shows as added");

        init_repo(&core, dir.to_string_lossy().into_owned()).unwrap();
        let _held_again = watch::held::hold(&dir);
        let opened = open_repo(&core, dir.to_string_lossy().into_owned()).unwrap();
        assert!(opened.git);
        assert_eq!(Path::new(&opened.root), dir);
        close_repo(&core).unwrap();
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

    /// Locate… picking a subfolder, a symlinked path or a linked worktree keys the project as
    /// opening it would, so it doesn't turn up twice.
    #[cfg(unix)]
    #[test]
    fn a_located_folder_is_keyed_as_its_open_would_be() {
        let Some(dir) = repo("project-key") else {
            return;
        };
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-C"])
                .arg(&dir)
                .args(args)
                .status()
                .is_ok_and(|s| s.success())
        };
        let key = Some(dir.to_string_lossy().into_owned());
        std::fs::create_dir_all(dir.join("src")).unwrap();
        assert_eq!(project_key(&dir.join("src")), key);
        let link = dir.with_extension("link");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        assert_eq!(project_key(&link), key);
        let linked = dir.with_extension("wt");
        let _ = std::fs::remove_dir_all(&linked);
        assert!(git(&["commit", "-q", "--allow-empty", "-m", "first"]));
        assert!(git(&[
            "worktree",
            "add",
            "-q",
            &linked.to_string_lossy(),
            "-b",
            "side"
        ]));
        assert_eq!(project_key(&linked), key);
        // Outside any repository: the folder itself, canonical.
        let plain = dir.with_extension("plain");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(
            project_key(&plain),
            Some(plain.canonicalize().unwrap().to_string_lossy().into_owned())
        );
        assert_eq!(project_key(&dir.join("nowhere")), None);
        for d in [&linked, &plain, &dir] {
            let _ = std::fs::remove_dir_all(d);
        }
        let _ = std::fs::remove_file(&link);
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
        let caught = rx.recv_timeout(watch::UP).expect("a catch-up");
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
        match rx.recv_timeout(watch::UP).expect("a catch-up") {
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
            let change = rx.recv_timeout(watch::UP).expect("b's watcher");
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
