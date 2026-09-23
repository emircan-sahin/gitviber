mod diff;
mod display;
mod fs;
mod git;
mod github;
mod journal;
mod lfs;
mod menu;
mod navigation;
mod network;
mod open_in;
mod pty;
#[cfg(test)]
mod scenario_tests;
mod shell;
mod suggest;
mod titlebar;
mod vibrancy;
mod watch;

use journal::{Action, Mode};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default)]
struct AppState {
    repo: Mutex<Option<PathBuf>>,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    github: github::Session,
    ptys: pty::Ptys,
    network: network::Running,
    /// Held by commands that write the index: two `git add`s at once fail on index.lock.
    index: Arc<Mutex<()>>,
    journal: Arc<journal::Journal>,
    /// `git --version`, checked once; the page asks again after the user installs git.
    git: Mutex<Option<git::GitInfo>>,
    suggest: suggest::Suggester,
}

type Res<T> = Result<T, String>;

fn repo(state: &State<AppState>) -> Res<PathBuf> {
    state
        .repo
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "No repository is open.".to_string())
}

/// Git and disk work runs off the async runtime so a slow command never stalls the UI.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

/// Runs an index-writing git command one at a time. An agent or the terminal can hold
/// index.lock for a moment too, so that failure is retried once.
async fn indexed<T: Send + 'static>(
    state: &State<'_, AppState>,
    f: impl Fn(&Path) -> Res<T> + Send + 'static,
) -> Res<T> {
    let r = repo(state)?;
    let lock = state.index.clone();
    blocking(move || with_index_lock(&lock, &r, f)).await
}

fn with_index_lock<T>(lock: &Mutex<()>, repo: &Path, f: impl Fn(&Path) -> Res<T>) -> Res<T> {
    let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
    match f(repo) {
        Err(e) if e.contains("index.lock") => {
            std::thread::sleep(std::time::Duration::from_millis(400));
            f(repo)
        }
        done => done,
    }
}

/// Runs a command that may move HEAD or local branches, recording what it moved for undo.
async fn journaled<T: Send + 'static>(
    state: &State<'_, AppState>,
    action: Action,
    f: impl FnOnce(&Path) -> Res<T> + Send + 'static,
) -> Res<T> {
    let r = repo(state)?;
    let journal = state.journal.clone();
    blocking(move || journal.record(&r, action, f)).await
}

/// Registers a network command under the page's id, streaming its progress to `progress`.
fn watch_network(
    state: &State<AppState>,
    op: String,
    progress: Channel<network::Progress>,
) -> network::Net {
    state.network.start(op, move |p| {
        let _ = progress.send(p);
    })
}

/// "Commit" and the like read better with the commit's short id.
fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenedRepo {
    root: String,
    /// The main worktree; the projects list is keyed by it, so agent worktrees don't pile up there.
    main: String,
}

#[tauri::command]
async fn open_repo(app: AppHandle, state: State<'_, AppState>, path: String) -> Res<OpenedRepo> {
    let (root, main) = blocking(move || {
        let path = PathBuf::from(path);
        let root = git::toplevel(&path)?;
        let main = git::main_worktree(Path::new(&root)).unwrap_or_else(|| root.clone());
        Ok((root, main))
    })
    .await?;
    let root_path = PathBuf::from(&root);
    let watcher = watch::start(app, root_path.clone())?;
    *state.watcher.lock().unwrap() = Some(watcher);
    *state.repo.lock().unwrap() = Some(root_path);
    Ok(OpenedRepo { root, main })
}

#[tauri::command]
async fn git_info(app: AppHandle, recheck: bool) -> Res<git::GitInfo> {
    blocking(move || {
        let state = app.state::<AppState>();
        if let Some(info) = state.git.lock().unwrap().clone().filter(|_| !recheck) {
            return Ok(info);
        }
        let mut info = git::check_install();
        // A git only the login shell's PATH has (MacPorts, nix) shows up once that's read.
        if matches!(info.state, "missing" | "tools")
            && shell::login_path().is_none()
            && shell::wait_for_login_path().is_some()
        {
            info = git::check_install();
        }
        *state.git.lock().unwrap() = Some(info.clone());
        Ok(info)
    })
    .await
}

#[tauri::command]
async fn install_git() -> Res<()> {
    blocking(git::install_tools).await
}

#[derive(serde::Serialize)]
struct IdentityCheck {
    current: git::Identity,
    /// From the GitHub account, when one is signed in already.
    suggested: Option<git::Identity>,
}

#[tauri::command]
async fn git_identity(app: AppHandle) -> Res<IdentityCheck> {
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
async fn set_git_identity(
    state: State<'_, AppState>,
    name: Option<String>,
    email: Option<String>,
) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::set_global_identity(&r, name.as_deref(), email.as_deref())).await
}

#[tauri::command]
async fn status(state: State<'_, AppState>) -> Res<git::RepoStatus> {
    let r = repo(&state)?;
    blocking(move || git::status(&r)).await
}

#[tauri::command]
async fn log(
    state: State<'_, AppState>,
    rev: Option<String>,
    skip: u32,
    limit: u32,
    filter: Option<git::LogFilter>,
) -> Res<Vec<git::Commit>> {
    let r = repo(&state)?;
    let filter = filter.unwrap_or_default();
    blocking(move || {
        // Only pathspecs, but the rule holds: no path from the frontend reaches outside the repo.
        for p in &filter.paths {
            fs::resolve(&r, p)?;
        }
        git::log_filtered(&r, rev.as_deref(), skip, limit, &filter)
    })
    .await
}

#[tauri::command]
async fn find_commit(state: State<'_, AppState>, sha: String) -> Res<Option<git::Commit>> {
    let r = repo(&state)?;
    blocking(move || git::find_commit(&r, &sha)).await
}

#[tauri::command]
async fn commit_files(state: State<'_, AppState>, sha: String) -> Res<Vec<git::FileChange>> {
    let r = repo(&state)?;
    blocking(move || git::commit_files(&r, &sha)).await
}

#[tauri::command]
async fn diff_pair(
    state: State<'_, AppState>,
    kind: String,
    path: String,
    old_path: Option<String>,
    sha: Option<String>,
    base: Option<String>,
    whitespace: Option<String>,
) -> Res<git::DiffPair> {
    let r = repo(&state)?;
    blocking(move || {
        git::diff_pair(
            &r,
            &kind,
            &path,
            old_path.as_deref(),
            sha.as_deref(),
            base.as_deref(),
            whitespace.as_deref(),
            |p| fs::read_file(&r, p),
        )
    })
    .await
}

/// Raw bytes, not JSON: a video as a number array would be several times its size.
#[tauri::command]
async fn media(
    state: State<'_, AppState>,
    kind: String,
    path: String,
    old_path: Option<String>,
    sha: Option<String>,
    base: Option<String>,
    original: bool,
) -> Res<tauri::ipc::Response> {
    let r = repo(&state)?;
    let bytes = blocking(move || {
        git::media(
            &r,
            &kind,
            &path,
            old_path.as_deref(),
            sha.as_deref(),
            base.as_deref(),
            original,
            |p| fs::read_media(&r, p),
        )
    })
    .await?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
async fn list_dir(state: State<'_, AppState>, path: String) -> Res<Vec<fs::Entry>> {
    let r = repo(&state)?;
    blocking(move || fs::list_dir(&r, &path)).await
}

#[tauri::command]
async fn blame(state: State<'_, AppState>, path: String) -> Res<git::Blame> {
    let r = repo(&state)?;
    blocking(move || {
        fs::resolve(&r, &path)?;
        git::blame(&r, &path)
    })
    .await
}

#[tauri::command]
async fn read_file(state: State<'_, AppState>, path: String) -> Res<git::FileText> {
    let r = repo(&state)?;
    blocking(move || Ok(fs::read_file(&r, &path))).await
}

#[tauri::command]
async fn branches(state: State<'_, AppState>) -> Res<Vec<git::Branch>> {
    let r = repo(&state)?;
    blocking(move || git::branches(&r)).await
}

#[tauri::command]
async fn switch_branch(state: State<'_, AppState>, name: String, create: bool) -> Res<()> {
    let label = if create {
        format!("Create branch {name}")
    } else {
        format!("Switch to {name}")
    };
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::switch_branch(r, &name, create)
    })
    .await
}

#[tauri::command]
async fn delete_branches(state: State<'_, AppState>, names: Vec<String>, force: bool) -> Res<()> {
    let label = match names.as_slice() {
        [one] => format!("Delete branch {one}"),
        all => format!("Delete {} branches", all.len()),
    };
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::delete_branches(r, &names, force)
    })
    .await
}

#[tauri::command]
async fn delete_remote_branch(
    state: State<'_, AppState>,
    name: String,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<()> {
    let r = repo(&state)?;
    let net = watch_network(&state, op, progress);
    blocking(move || git::delete_remote_branch(&r, &name, &net)).await
}

#[tauri::command]
async fn worktrees(state: State<'_, AppState>) -> Res<Vec<git::Worktree>> {
    let r = repo(&state)?;
    blocking(move || git::worktrees(&r).map(git::with_live_locks)).await
}

#[tauri::command]
async fn add_worktree(state: State<'_, AppState>, branch: String) -> Res<String> {
    let r = repo(&state)?;
    blocking(move || git::add_worktree(&r, &branch)).await
}

#[tauri::command]
async fn worktree_state(state: State<'_, AppState>, path: String) -> Res<git::WorktreeState> {
    let r = repo(&state)?;
    blocking(move || git::worktree_state(&r, &path)).await
}

#[tauri::command]
async fn remove_worktree(state: State<'_, AppState>, path: String, force: bool) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::remove_worktree(&r, &path, force)).await
}

#[tauri::command]
async fn stage(state: State<'_, AppState>, paths: Vec<String>, allow_nested: bool) -> Res<()> {
    indexed(&state, move |r| git::stage_with(r, &paths, allow_nested)).await
}

#[tauri::command]
async fn unstage(state: State<'_, AppState>, paths: Vec<String>) -> Res<()> {
    indexed(&state, move |r| git::unstage(r, &paths)).await
}

#[tauri::command]
async fn discard(state: State<'_, AppState>, paths: Vec<String>) -> Res<()> {
    let r = repo(&state)?;
    let (journal, index) = (state.journal.clone(), state.index.clone());
    blocking(move || {
        journal.discard(&r, &paths, || {
            with_index_lock(&index, &r, |r| git::discard(r, &paths))
        })
    })
    .await
}

#[tauri::command]
async fn commit(
    state: State<'_, AppState>,
    message: String,
    options: git::CommitOptions,
) -> Res<()> {
    let subject = message.lines().next().unwrap_or("").trim();
    let label = match (options.amend, subject) {
        (true, "") => "Amend last commit".to_string(),
        (true, s) => format!("Amend \"{s}\""),
        (false, s) => format!("Commit \"{s}\""),
    };
    let lock = state.index.clone();
    journaled(&state, Action::new(label, Mode::Soft), move |r| {
        with_index_lock(&lock, r, |r| git::commit(r, &message, &options))
    })
    .await
}

#[tauri::command]
async fn commit_template(state: State<'_, AppState>) -> Res<Option<String>> {
    let r = repo(&state)?;
    blocking(move || Ok(git::commit_template(&r))).await
}

#[tauri::command]
async fn recent_authors(state: State<'_, AppState>) -> Res<Vec<String>> {
    let r = repo(&state)?;
    blocking(move || git::recent_authors(&r)).await
}

/// Runs the user's own agent CLI for a commit message (off unless they set one up).
#[tauri::command]
async fn suggest_message(
    state: State<'_, AppState>,
    command: String,
    prompt: String,
    scope: suggest::Scope,
) -> Res<String> {
    let r = repo(&state)?;
    let cancel = state.suggest.start();
    let flag = cancel.clone();
    let out = blocking(move || suggest::run(&r, &command, &prompt, scope, &flag)).await;
    state.suggest.finish(&cancel);
    out
}

#[tauri::command]
fn suggest_cancel(state: State<'_, AppState>) {
    state.suggest.cancel()
}

#[tauri::command]
async fn commit_details(state: State<'_, AppState>, sha: String) -> Res<git::CommitDetails> {
    let r = repo(&state)?;
    blocking(move || git::commit_details(&r, &sha)).await
}

#[tauri::command]
async fn push(
    state: State<'_, AppState>,
    force: Option<bool>,
    remote: Option<String>,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<()> {
    let r = repo(&state)?;
    let net = watch_network(&state, op, progress);
    blocking(move || git::push(&r, force.unwrap_or(false), remote.as_deref(), &net)).await
}

/// The bool results below mean "stopped on conflicts".
#[tauri::command]
async fn pull(
    state: State<'_, AppState>,
    mode: String,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<bool> {
    let label = match mode.as_str() {
        "merge" => "Pull (merge)",
        "rebase" => "Pull (rebase)",
        _ => "Pull",
    };
    let net = watch_network(&state, op, progress);
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::pull(r, &mode, &net)
    })
    .await
}

#[tauri::command]
async fn merge(state: State<'_, AppState>, name: String) -> Res<bool> {
    let label = format!("Merge {name}");
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::merge(r, &name)
    })
    .await
}

#[tauri::command]
async fn rebase(state: State<'_, AppState>, onto: String) -> Res<bool> {
    let label = format!("Rebase onto {onto}");
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::rebase(r, &onto)
    })
    .await
}

// These finish (or call off) the action that stopped on conflicts; its entry is recorded then.
#[tauri::command]
async fn op_continue(state: State<'_, AppState>) -> Res<bool> {
    let lock = state.index.clone();
    journaled(&state, Action::new("Continue", Mode::Keep), move |r| {
        with_index_lock(&lock, r, git::op_continue)
    })
    .await
}

#[tauri::command]
async fn op_abort(state: State<'_, AppState>) -> Res<()> {
    journaled(&state, Action::new("Abort", Mode::Keep), git::op_abort).await
}

#[tauri::command]
async fn rebase_skip(state: State<'_, AppState>) -> Res<bool> {
    journaled(&state, Action::new("Skip", Mode::Keep), git::rebase_skip).await
}

#[tauri::command]
async fn resolve_side(state: State<'_, AppState>, path: String, side: String) -> Res<()> {
    indexed(&state, move |r| git::resolve_side(r, &path, &side)).await
}

#[tauri::command]
async fn write_file(state: State<'_, AppState>, path: String, content: String) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || fs::write_file(&r, &path, &content)).await
}

// ---------------------------------------------------------------- Explorer file actions

#[tauri::command]
async fn create_file(state: State<'_, AppState>, path: String) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || fs::create_file(&r, &path)).await
}

#[tauri::command]
async fn create_dir(state: State<'_, AppState>, path: String) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || fs::create_dir(&r, &path)).await
}

#[tauri::command]
async fn rename_path(state: State<'_, AppState>, from: String, to: String) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || fs::rename_entry(&r, &from, &to)).await
}

#[tauri::command]
async fn trash_path(state: State<'_, AppState>, path: String) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || fs::trash(&r, &path)).await
}

#[tauri::command]
async fn reveal_path(state: State<'_, AppState>, path: String) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || fs::reveal(&r, &path)).await
}

/// The apps "Open in…" can use, as installed right now.
#[tauri::command]
async fn open_in_apps() -> Res<Vec<open_in::Installed>> {
    blocking(|| Ok(open_in::installed())).await
}

/// `path` in the open worktree ("" for all of it) in a known app; editors go to `line`.
#[tauri::command]
async fn open_in(
    state: State<'_, AppState>,
    app: String,
    path: String,
    line: Option<u32>,
) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || open_in::open(&r, &path, line, &app)).await
}

/// The same with the user's own command template.
#[tauri::command]
async fn open_in_custom(
    state: State<'_, AppState>,
    command: String,
    path: String,
    line: Option<u32>,
) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || open_in::open_custom(&r, &path, line, &command)).await
}

#[derive(serde::Serialize)]
struct ProjectInfo {
    /// False once the folder was moved or deleted.
    exists: bool,
    /// owner/name of its GitHub origin
    github: Option<String>,
}

/// For the projects list: which saved folders are still there, and where each lives on GitHub.
#[tauri::command]
async fn project_info(paths: Vec<String>) -> Res<Vec<ProjectInfo>> {
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
async fn reveal_project(path: String) -> Res<()> {
    blocking(move || {
        let top = git::toplevel(Path::new(&path))?;
        if Path::new(&top) != Path::new(&path) {
            return Err(format!("not a project folder: {path}"));
        }
        fs::reveal(Path::new(&top), "")
    })
    .await
}

#[tauri::command]
async fn fetch(
    state: State<'_, AppState>,
    op: String,
    progress: Channel<network::Progress>,
) -> Res<()> {
    let r = repo(&state)?;
    let net = watch_network(&state, op, progress);
    blocking(move || git::fetch(&r, &net)).await
}

#[tauri::command]
async fn last_fetch(state: State<'_, AppState>) -> Res<Option<u64>> {
    let r = repo(&state)?;
    blocking(move || Ok(git::last_fetch(&r))).await
}

/// Stops the network command the page started under `op`; it then fails with "git:cancelled".
#[tauri::command]
fn cancel_network(state: State<'_, AppState>, op: String) {
    state.network.cancel(&op)
}

/// Like open_repo, any folder the user picked: the clone lands in `parent/name`.
#[tauri::command]
async fn clone_repo(
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
async fn init_repo(path: String) -> Res<()> {
    blocking(move || git::init(Path::new(&path))).await
}

// ---------------------------------------------------------------- history actions

#[tauri::command]
async fn undo_commit(state: State<'_, AppState>, sha: String) -> Res<()> {
    let label = format!("Undo commit {}", short(&sha));
    journaled(&state, Action::new(label, Mode::Soft), move |r| {
        git::undo_commit(r, &sha)
    })
    .await
}

#[tauri::command]
async fn reset(state: State<'_, AppState>, sha: String, mode: String, head: String) -> Res<()> {
    let label = format!("Reset to {}", short(&sha));
    // Undone the same way, except that a hard reset's lost changes can't come back.
    let back = match mode.as_str() {
        "soft" => Mode::Soft,
        "mixed" => Mode::Mixed,
        _ => Mode::Keep,
    };
    journaled(&state, Action::new(label, back), move |r| {
        git::reset(r, &sha, &mode, &head)
    })
    .await
}

#[tauri::command]
async fn drops_pushed(state: State<'_, AppState>, sha: String) -> Res<bool> {
    let r = repo(&state)?;
    blocking(move || git::drops_pushed(&r, &sha)).await
}

#[tauri::command]
async fn revert(state: State<'_, AppState>, sha: String) -> Res<bool> {
    let label = format!("Revert {}", short(&sha));
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::revert(r, &sha)
    })
    .await
}

#[tauri::command]
async fn checkout_commit(state: State<'_, AppState>, sha: String) -> Res<()> {
    let label = format!("Check out {}", short(&sha));
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::checkout_commit(r, &sha)
    })
    .await
}

#[tauri::command]
async fn create_branch_at(state: State<'_, AppState>, name: String, sha: String) -> Res<()> {
    let label = format!("Create branch {name}");
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::create_branch_at(r, &name, &sha)
    })
    .await
}

#[tauri::command]
async fn create_tag(state: State<'_, AppState>, name: String, sha: String) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::create_tag(&r, &name, &sha)).await
}

// ---------------------------------------------------------------- undo / redo

#[tauri::command]
async fn journal(state: State<'_, AppState>) -> Res<journal::View> {
    let r = repo(&state)?;
    let journal = state.journal.clone();
    blocking(move || Ok(journal.view(&r))).await
}

#[tauri::command]
fn journal_last(state: State<'_, AppState>) -> Res<Option<u64>> {
    Ok(state.journal.last(&repo(&state)?))
}

/// `id`: the entry the user means; refused if it is no longer the next one.
#[tauri::command]
async fn undo(state: State<'_, AppState>, id: Option<u64>) -> Res<journal::EntryView> {
    step(&state, false, id).await
}

#[tauri::command]
async fn redo(state: State<'_, AppState>, id: Option<u64>) -> Res<journal::EntryView> {
    step(&state, true, id).await
}

async fn step(
    state: &State<'_, AppState>,
    forward: bool,
    id: Option<u64>,
) -> Res<journal::EntryView> {
    let r = repo(state)?;
    let (journal, index) = (state.journal.clone(), state.index.clone());
    blocking(move || journal.step(&r, forward, id, &index)).await
}

/// https://github.com/owner/name when origin is on GitHub, for "Open on GitHub" links.
#[tauri::command]
async fn github_web_url(state: State<'_, AppState>) -> Res<Option<String>> {
    let r = repo(&state)?;
    blocking(move || {
        Ok(git::remote_url(&r, "origin")
            .and_then(|u| github::parse_remote(&u))
            .map(|g| format!("https://github.com/{}/{}", g.owner, g.name)))
    })
    .await
}

// ---------------------------------------------------------------- GitHub
// Tauri state is borrowed, so these hop to the blocking pool via the app handle.

#[tauri::command]
async fn gh_account(app: AppHandle) -> Res<github::Account> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::account(&state.github, &repo(&state)?)
    })
    .await
}

#[tauri::command]
async fn gh_original_remote(
    state: State<'_, AppState>,
    original: String,
    fetch: bool,
) -> Res<Option<String>> {
    let r = repo(&state)?;
    blocking(move || github::original_remote(&r, &original, fetch)).await
}

#[tauri::command]
async fn switch_tracking(state: State<'_, AppState>, remote_ref: String) -> Res<()> {
    let local = remote_ref
        .split_once('/')
        .map_or(remote_ref.as_str(), |(_, b)| b);
    let label = format!("Switch to {local}");
    journaled(&state, Action::new(label, Mode::Keep), move |r| {
        git::switch_tracking(r, &remote_ref)
    })
    .await
}

#[tauri::command]
async fn set_push_default(state: State<'_, AppState>, remote: String) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::set_push_default(&r, &remote)).await
}

#[tauri::command]
async fn gh_sync_fork(app: AppHandle, branch: String) -> Res<String> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::sync_fork(&state.github, &repo(&state)?, &branch)
    })
    .await
}

#[tauri::command]
async fn pull_draft(state: State<'_, AppState>, base: String) -> Res<git::PullDraft> {
    let r = repo(&state)?;
    blocking(move || git::pull_draft(&r, &base)).await
}

#[tauri::command]
async fn gh_remotes(state: State<'_, AppState>) -> Res<Vec<github::Remote>> {
    let r = repo(&state)?;
    blocking(move || Ok(github::remotes(&r))).await
}

#[tauri::command]
async fn gh_add_original_remote(app: AppHandle) -> Res<String> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::add_original_remote(&state.github, &repo(&state)?)
    })
    .await
}

#[tauri::command]
async fn gh_protected_branches(app: AppHandle) -> Res<Vec<String>> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::protected_branches(&state.github, &repo(&state)?)
    })
    .await
}

#[tauri::command]
async fn pr_list(
    app: AppHandle,
    target: Option<String>,
    filter: String,
    pages: usize,
) -> Res<Vec<github::Pull>> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::list(
            &state.github,
            &repo(&state)?,
            target.as_deref(),
            &filter,
            pages,
        )
    })
    .await
}

#[tauri::command]
async fn pr_detail(app: AppHandle, target: Option<String>, number: u64) -> Res<github::PullDetail> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::detail(&state.github, &repo(&state)?, target.as_deref(), number)
    })
    .await
}

#[tauri::command]
async fn pr_attachments(
    app: AppHandle,
    target: Option<String>,
    number: u64,
) -> Res<std::collections::HashMap<String, String>> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::attachments(&state.github, &repo(&state)?, target.as_deref(), number)
    })
    .await
}

#[tauri::command]
async fn pr_files(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    base_ref: String,
    base_sha: String,
    head_sha: String,
) -> Res<github::PullFiles> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::files(
            &state.github,
            &repo(&state)?,
            target.as_deref(),
            number,
            &base_ref,
            &base_sha,
            &head_sha,
        )
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn pr_create(
    app: AppHandle,
    target: Option<String>,
    title: String,
    body: String,
    head: String,
    base: String,
    draft: bool,
    maintainer_edits: bool,
) -> Res<github::Pull> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::create(
            &state.github,
            &repo(&state)?,
            target.as_deref(),
            &title,
            &body,
            &head,
            &base,
            draft,
            maintainer_edits,
        )
    })
    .await
}

#[tauri::command]
async fn pr_merge(app: AppHandle, target: Option<String>, number: u64, method: String) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::merge(
            &state.github,
            &repo(&state)?,
            target.as_deref(),
            number,
            &method,
        )
    })
    .await
}

#[tauri::command]
async fn pr_set_open(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    open: bool,
) -> Res<github::Pull> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::set_open(
            &state.github,
            &repo(&state)?,
            target.as_deref(),
            number,
            open,
        )
    })
    .await
}

#[tauri::command]
async fn pr_review(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    event: String,
    body: String,
) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::review(
            &state.github,
            &repo(&state)?,
            target.as_deref(),
            number,
            &event,
            &body,
        )
    })
    .await
}

#[tauri::command]
async fn issue_list(
    app: AppHandle,
    target: Option<String>,
    filter: String,
    labels: Vec<String>,
) -> Res<Vec<github::Issue>> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::issues(
            &state.github,
            &repo(&state)?,
            target.as_deref(),
            &filter,
            &labels,
        )
    })
    .await
}

#[tauri::command]
async fn issue_counts(
    app: AppHandle,
    target: Option<String>,
    labels: Vec<String>,
) -> Res<github::IssueCounts> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::issue_counts(&state.github, &repo(&state)?, target.as_deref(), &labels)
    })
    .await
}

#[tauri::command]
async fn issue_labels(app: AppHandle, target: Option<String>) -> Res<Vec<github::Label>> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::issue_labels(&state.github, &repo(&state)?, target.as_deref())
    })
    .await
}

#[tauri::command]
async fn issue_detail(
    app: AppHandle,
    target: Option<String>,
    number: u64,
) -> Res<github::IssueDetail> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::issue_detail(&state.github, &repo(&state)?, target.as_deref(), number)
    })
    .await
}

#[tauri::command]
async fn issue_create(
    app: AppHandle,
    target: Option<String>,
    title: String,
    body: String,
) -> Res<github::Issue> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::issue_create(
            &state.github,
            &repo(&state)?,
            target.as_deref(),
            &title,
            &body,
        )
    })
    .await
}

#[tauri::command]
async fn issue_edit(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    title: String,
    body: String,
) -> Res<github::Issue> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::issue_edit(
            &state.github,
            &repo(&state)?,
            target.as_deref(),
            number,
            &title,
            &body,
        )
    })
    .await
}

#[tauri::command]
async fn issue_set_open(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    open: bool,
    reason: String,
) -> Res<github::Issue> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::issue_set_open(
            &state.github,
            &repo(&state)?,
            target.as_deref(),
            number,
            open,
            &reason,
        )
    })
    .await
}

#[tauri::command]
async fn issue_set_labels(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    labels: Vec<String>,
) -> Res<Vec<github::Label>> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::issue_set_labels(
            &state.github,
            &repo(&state)?,
            target.as_deref(),
            number,
            &labels,
        )
    })
    .await
}

#[tauri::command]
async fn issue_delete(app: AppHandle, target: Option<String>, number: u64) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::issue_delete(&state.github, &repo(&state)?, target.as_deref(), number)
    })
    .await
}

#[tauri::command]
async fn issue_comment(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    body: String,
) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::issue_comment(
            &state.github,
            &repo(&state)?,
            target.as_deref(),
            number,
            &body,
        )
    })
    .await
}

#[tauri::command]
async fn pr_checkout(
    app: AppHandle,
    target: Option<String>,
    number: u64,
    head_ref: String,
    same_repo: bool,
) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        let r = repo(&state)?;
        let action = Action::new(format!("Check out PR #{number}"), Mode::Keep);
        state.journal.record(&r, action, |r| {
            let remote = github::fetch_remote(&state.github, r, target.as_deref())?;
            // The original's PRs get their own local names; origin's keep pr/<n>.
            let owner = target
                .as_deref()
                .filter(|_| remote != "origin")
                .and_then(|t| t.split('/').next());
            github::checkout(r, &remote, owner, number, &head_ref, same_repo)
        })
    })
    .await
}

/// Any folder, unlike the repo commands: the shell can `cd` anywhere the user can anyway.
#[tauri::command]
fn pty_spawn(
    state: State<'_, AppState>,
    cwd: String,
    cols: u16,
    rows: u16,
    output: tauri::ipc::Channel<tauri::ipc::Response>,
    exit: tauri::ipc::Channel<Option<u32>>,
) -> Res<u32> {
    state.ptys.spawn(Path::new(&cwd), cols, rows, output, exit)
}

#[tauri::command]
fn pty_write(state: State<'_, AppState>, id: u32, data: String) -> Res<()> {
    state.ptys.write(id, &data)
}

#[tauri::command]
fn pty_resize(state: State<'_, AppState>, id: u32, cols: u16, rows: u16) -> Res<()> {
    state.ptys.resize(id, cols, rows)
}

#[tauri::command]
fn pty_kill(state: State<'_, AppState>, id: u32) {
    state.ptys.kill(id)
}

/// What a bug report asks for: app version and commit, OS, and git.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct About {
    version: String,
    commit: String,
    os: String,
    arch: String,
    git: Option<String>,
}

#[tauri::command]
async fn about(app: AppHandle) -> Res<About> {
    let version = app.package_info().version.to_string();
    blocking(move || {
        let text = |cmd: &str, args: &[&str]| {
            std::process::Command::new(cmd)
                .args(args)
                .env("PATH", git::search_path())
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };
        let os = match std::env::consts::OS {
            "macos" => format!(
                "macOS {}",
                text("sw_vers", &["-productVersion"]).unwrap_or_default()
            ),
            other => other.to_string(),
        };
        Ok(About {
            version,
            commit: env!("GITVIBER_COMMIT").to_string(),
            os: os.trim().to_string(),
            arch: std::env::consts::ARCH.to_string(),
            git: text("git", &["--version"])
                .map(|v| v.trim_start_matches("git version ").to_string()),
        })
    })
    .await
}

#[tauri::command]
fn open_url(url: String) -> Res<()> {
    github::open_url(&url)
}

#[tauri::command]
fn set_menu(
    app: AppHandle,
    handles: State<'_, menu::Handles>,
    items: std::collections::HashMap<String, menu::ItemState>,
    recent: Option<Vec<menu::Recent>>,
) -> Res<()> {
    menu::update(&app, &handles, items, recent).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    shell::resolve_in_background();
    let context = tauri::generate_context!();
    // Release builds load the bundled app; only debug builds are served from the dev server.
    let dev_url = if cfg!(debug_assertions) {
        context.config().build.dev_url.clone()
    } else {
        None
    };
    tauri::Builder::default()
        .plugin(navigation::guard(dev_url))
        .plugin(tauri_plugin_dialog::init())
        .menu(menu::build)
        .on_menu_event(|app, event| {
            let _ = app.emit("menu", event.id().as_ref());
        })
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                webview.state::<AppState>().ptys.kill_all();
            }
        })
        .manage(AppState::default())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            menu::keep_typed_key_equivalents();
            if let Some(webview) = app.get_webview_window("main") {
                display::unlock_high_refresh_rate(&webview);
                titlebar::setup(&webview);
                // The page shows the window once its theme is applied (main.tsx); if it
                // never gets that far, a visible window beats an app with none.
                let w = webview.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    if !w.is_visible().unwrap_or(true) {
                        let _ = w.show();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_repo,
            git_info,
            install_git,
            git_identity,
            set_git_identity,
            status,
            log,
            find_commit,
            commit_files,
            diff_pair,
            media,
            list_dir,
            read_file,
            blame,
            branches,
            switch_branch,
            delete_branches,
            delete_remote_branch,
            worktrees,
            worktree_state,
            add_worktree,
            remove_worktree,
            stage,
            unstage,
            discard,
            commit,
            commit_template,
            recent_authors,
            suggest_message,
            suggest_cancel,
            commit_details,
            push,
            pull,
            fetch,
            last_fetch,
            cancel_network,
            clone_repo,
            init_repo,
            merge,
            rebase,
            op_continue,
            op_abort,
            rebase_skip,
            resolve_side,
            write_file,
            create_file,
            create_dir,
            rename_path,
            trash_path,
            reveal_path,
            open_in_apps,
            open_in,
            open_in_custom,
            project_info,
            reveal_project,
            undo_commit,
            reset,
            drops_pushed,
            revert,
            checkout_commit,
            create_branch_at,
            create_tag,
            journal,
            journal_last,
            undo,
            redo,
            github_web_url,
            gh_account,
            gh_protected_branches,
            gh_original_remote,
            gh_remotes,
            pull_draft,
            gh_sync_fork,
            set_push_default,
            switch_tracking,
            gh_add_original_remote,
            pr_list,
            pr_detail,
            pr_attachments,
            pr_files,
            pr_create,
            pr_merge,
            pr_set_open,
            pr_review,
            pr_checkout,
            issue_list,
            issue_counts,
            issue_labels,
            issue_detail,
            issue_create,
            issue_edit,
            issue_set_open,
            issue_set_labels,
            issue_delete,
            issue_comment,
            open_url,
            about,
            set_menu,
            pty_spawn,
            pty_write,
            pty_resize,
            pty_kill,
            vibrancy::set_translucent
        ])
        .run(context)
        .expect("error while running GitViber");
}
