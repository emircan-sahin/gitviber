use crate::state::{blocking, in_repo, repo, watch_network, AppState, Res};
use crate::{fs, git, github, network, shell, watch};
use std::path::{Path, PathBuf};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedRepo {
    root: String,
    /// The main worktree; the projects list is keyed by it, so agent worktrees don't pile up there.
    main: String,
}

#[tauri::command]
pub async fn open_repo(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Res<OpenedRepo> {
    let (root, main) = blocking(move || {
        let path = PathBuf::from(path);
        let root = git::toplevel(&path)?;
        let main = git::main_worktree(Path::new(&root)).unwrap_or_else(|| root.clone());
        Ok((root, main))
    })
    .await?;
    let root_path = PathBuf::from(&root);
    let watcher = watch::start(app, root_path.clone())?;
    *state.watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(watcher);
    *state.repo.lock().unwrap_or_else(|e| e.into_inner()) = Some(root_path);
    Ok(OpenedRepo { root, main })
}

#[tauri::command]
pub async fn git_info(app: AppHandle, recheck: bool) -> Res<git::GitInfo> {
    blocking(move || {
        let state = app.state::<AppState>();
        if let Some(info) = state
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
        *state.git.lock().unwrap_or_else(|e| e.into_inner()) = Some(info.clone());
        Ok(info)
    })
    .await
}

#[tauri::command]
pub async fn install_git() -> Res<()> {
    blocking(git::install_tools).await
}

#[derive(serde::Serialize)]
pub struct IdentityCheck {
    current: git::Identity,
    /// From the GitHub account, when one is signed in already.
    suggested: Option<git::Identity>,
}

#[tauri::command]
pub async fn git_identity(app: AppHandle) -> Res<IdentityCheck> {
    blocking(move || {
        let state = app.state::<AppState>();
        let r = repo(&state)?;
        let current = git::identity(&r);
        let complete = current.name.is_some() && current.email.is_some();
        let suggested = (!complete)
            .then(|| github::profile(&state.github, &r))
            .flatten();
        Ok(IdentityCheck { current, suggested })
    })
    .await
}

/// Only the parts given are written, to the global config: the repo's own stays untouched.
#[tauri::command]
pub async fn set_git_identity(
    state: State<'_, AppState>,
    name: Option<String>,
    email: Option<String>,
) -> Res<()> {
    in_repo(&state, move |r| {
        git::set_global_identity(r, name.as_deref(), email.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn remote_list(state: State<'_, AppState>) -> Res<Vec<git::Remote>> {
    in_repo(&state, git::remote_list).await
}

/// `action`: "add" (name, url), "remove" (name), "rename" (name, to) or "set-url" (name, url).
#[tauri::command]
pub async fn remote_edit(
    state: State<'_, AppState>,
    action: String,
    name: String,
    value: Option<String>,
) -> Res<()> {
    in_repo(&state, move |r| {
        let value = || value.as_deref().ok_or("missing value");
        match action.as_str() {
            "add" => git::remote_add(r, &name, value()?),
            "remove" => git::remote_remove(r, &name),
            "rename" => git::remote_rename(r, &name, value()?),
            "set-url" => git::remote_set_url(r, &name, value()?),
            other => Err(format!("unknown remote action: {other}")),
        }
    })
    .await
}

/// This repository's own identity, and the global one it would use without it.
#[derive(serde::Serialize)]
pub struct RepoIdentity {
    own: git::Identity,
    global: git::Identity,
}

#[tauri::command]
pub async fn repo_identity(state: State<'_, AppState>) -> Res<RepoIdentity> {
    in_repo(&state, move |r| {
        Ok(RepoIdentity {
            own: git::repo_identity(r),
            global: git::global_identity(r),
        })
    })
    .await
}

/// Both parts, or neither: the repository's own identity then goes and the global one applies.
#[tauri::command]
pub async fn set_repo_identity(
    state: State<'_, AppState>,
    name: Option<String>,
    email: Option<String>,
) -> Res<()> {
    in_repo(&state, move |r| match (&name, &email) {
        (Some(n), Some(e)) => git::set_repo_identity(r, Some((n, e))),
        _ => git::set_repo_identity(r, None),
    })
    .await
}

#[derive(serde::Serialize)]
pub struct ProjectInfo {
    /// False once the folder was moved or deleted.
    exists: bool,
    /// owner/name of its GitHub origin
    github: Option<String>,
}

/// For the projects list: which saved folders are still there, and where each lives on GitHub.
#[tauri::command]
pub async fn project_info(paths: Vec<String>) -> Res<Vec<ProjectInfo>> {
    blocking(move || {
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
    })
    .await
}

/// Reveals a saved project, open or not. reveal_path is confined to the open repo; this only
/// takes the top folder of a git worktree, so the page can't point Finder anywhere else.
#[tauri::command]
pub async fn reveal_project(path: String) -> Res<()> {
    blocking(move || {
        let top = git::toplevel(Path::new(&path))?;
        if Path::new(&top) != Path::new(&path) {
            return Err(format!("not a project folder: {path}"));
        }
        fs::reveal(Path::new(&top), "")
    })
    .await
}

/// Like open_repo, any folder the user picked: the clone lands in `parent/name`.
#[tauri::command]
pub async fn clone_repo(
    state: State<'_, AppState>,
    url: String,
    parent: String,
    name: String,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<String> {
    let net = watch_network(&state, op, progress);
    blocking(move || git::clone(Path::new(&parent), &url, &name, &net)).await
}

#[tauri::command]
pub async fn init_repo(path: String) -> Res<()> {
    blocking(move || git::init(Path::new(&path))).await
}
