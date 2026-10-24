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
    let main = git::main_worktree(Path::new(&root)).unwrap_or_else(|| root.clone());
    let root_path = PathBuf::from(&root);
    // A new watcher even for the open repo: one deleted and made again at its path left the old
    // one watching nothing (inotify).
    let watcher = watch::start(core.events(), root_path.clone())?;
    *core.watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(watcher);
    // gh's active account until the page sends this project's own (gh_use_account): one
    // project's pick never carries into the next. Before the repo switches, so none goes out as both.
    // Already open (`gitviber .` in its own terminal), it keeps its account.
    let open = core.repo.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if open.as_ref() != Some(&root_path) {
        github::use_account(&core.github, None)?;
    }
    *core.repo.lock().unwrap_or_else(|e| e.into_inner()) = Some(root_path);
    Ok(OpenedRepo { root, main })
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
