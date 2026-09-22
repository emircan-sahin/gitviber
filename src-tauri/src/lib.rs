mod diff;
mod display;
mod fs;
mod git;
mod github;
mod navigation;
mod pty;
#[cfg(test)]
mod scenario_tests;
mod titlebar;
mod watch;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

#[derive(Default)]
struct AppState {
    repo: Mutex<Option<PathBuf>>,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    github: github::Session,
    ptys: pty::Ptys,
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
        let root = git::toplevel(PathBuf::from(path).as_path())?;
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
async fn status(state: State<'_, AppState>) -> Res<git::RepoStatus> {
    let r = repo(&state)?;
    blocking(move || git::status(&r)).await
}

#[tauri::command]
async fn log(state: State<'_, AppState>, skip: u32, limit: u32) -> Res<Vec<git::Commit>> {
    let r = repo(&state)?;
    blocking(move || git::log(&r, skip, limit)).await
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
    let r = repo(&state)?;
    blocking(move || git::switch_branch(&r, &name, create)).await
}

#[tauri::command]
async fn worktrees(state: State<'_, AppState>) -> Res<Vec<git::Worktree>> {
    let r = repo(&state)?;
    blocking(move || git::worktrees(&r)).await
}

#[tauri::command]
async fn add_worktree(state: State<'_, AppState>, branch: String) -> Res<String> {
    let r = repo(&state)?;
    blocking(move || git::add_worktree(&r, &branch)).await
}

#[tauri::command]
async fn worktree_changes(state: State<'_, AppState>, path: String) -> Res<u32> {
    let r = repo(&state)?;
    blocking(move || git::worktree_changes(&r, &path)).await
}

#[tauri::command]
async fn stage(state: State<'_, AppState>, paths: Vec<String>, allow_nested: bool) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::stage_with(&r, &paths, allow_nested)).await
}

#[tauri::command]
async fn unstage(state: State<'_, AppState>, paths: Vec<String>) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::unstage(&r, &paths)).await
}

#[tauri::command]
async fn discard(state: State<'_, AppState>, paths: Vec<String>) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::discard(&r, &paths)).await
}

#[tauri::command]
async fn commit(state: State<'_, AppState>, message: String, amend: bool) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::commit(&r, &message, amend)).await
}

#[tauri::command]
async fn push(state: State<'_, AppState>) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::push(&r)).await
}

/// The bool results below mean "stopped on conflicts".
#[tauri::command]
async fn pull(state: State<'_, AppState>, mode: String) -> Res<bool> {
    let r = repo(&state)?;
    blocking(move || git::pull(&r, &mode)).await
}

#[tauri::command]
async fn merge(state: State<'_, AppState>, name: String) -> Res<bool> {
    let r = repo(&state)?;
    blocking(move || git::merge(&r, &name)).await
}

#[tauri::command]
async fn rebase(state: State<'_, AppState>, onto: String) -> Res<bool> {
    let r = repo(&state)?;
    blocking(move || git::rebase(&r, &onto)).await
}

#[tauri::command]
async fn op_continue(state: State<'_, AppState>) -> Res<bool> {
    let r = repo(&state)?;
    blocking(move || git::op_continue(&r)).await
}

#[tauri::command]
async fn op_abort(state: State<'_, AppState>) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::op_abort(&r)).await
}

#[tauri::command]
async fn rebase_skip(state: State<'_, AppState>) -> Res<bool> {
    let r = repo(&state)?;
    blocking(move || git::rebase_skip(&r)).await
}

#[tauri::command]
async fn resolve_side(state: State<'_, AppState>, path: String, side: String) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::resolve_side(&r, &path, &side)).await
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

#[tauri::command]
async fn fetch(state: State<'_, AppState>) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::fetch(&r)).await
}

// ---------------------------------------------------------------- history actions

#[tauri::command]
async fn undo_commit(state: State<'_, AppState>, sha: String) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::undo_commit(&r, &sha)).await
}

#[tauri::command]
async fn reset(state: State<'_, AppState>, sha: String, mode: String, head: String) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::reset(&r, &sha, &mode, &head)).await
}

#[tauri::command]
async fn drops_pushed(state: State<'_, AppState>, sha: String) -> Res<bool> {
    let r = repo(&state)?;
    blocking(move || git::drops_pushed(&r, &sha)).await
}

#[tauri::command]
async fn revert(state: State<'_, AppState>, sha: String) -> Res<bool> {
    let r = repo(&state)?;
    blocking(move || git::revert(&r, &sha)).await
}

#[tauri::command]
async fn checkout_commit(state: State<'_, AppState>, sha: String) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::checkout_commit(&r, &sha)).await
}

#[tauri::command]
async fn create_branch_at(state: State<'_, AppState>, name: String, sha: String) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::create_branch_at(&r, &name, &sha)).await
}

#[tauri::command]
async fn create_tag(state: State<'_, AppState>, name: String, sha: String) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || git::create_tag(&r, &name, &sha)).await
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
async fn pr_list(app: AppHandle, filter: String) -> Res<Vec<github::Pull>> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::list(&state.github, &repo(&state)?, &filter)
    })
    .await
}

#[tauri::command]
async fn pr_detail(app: AppHandle, number: u64) -> Res<github::PullDetail> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::detail(&state.github, &repo(&state)?, number)
    })
    .await
}

#[tauri::command]
async fn pr_attachments(
    app: AppHandle,
    number: u64,
) -> Res<std::collections::HashMap<String, String>> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::attachments(&state.github, &repo(&state)?, number)
    })
    .await
}

#[tauri::command]
async fn pr_files(
    state: State<'_, AppState>,
    number: u64,
    base_ref: String,
    base_sha: String,
    head_sha: String,
) -> Res<github::PullFiles> {
    let r = repo(&state)?;
    blocking(move || github::files(&r, number, &base_ref, &base_sha, &head_sha)).await
}

#[tauri::command]
async fn pr_create(
    app: AppHandle,
    title: String,
    body: String,
    head: String,
    base: String,
    draft: bool,
) -> Res<github::Pull> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::create(
            &state.github,
            &repo(&state)?,
            &title,
            &body,
            &head,
            &base,
            draft,
        )
    })
    .await
}

#[tauri::command]
async fn pr_merge(app: AppHandle, number: u64, method: String) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::merge(&state.github, &repo(&state)?, number, &method)
    })
    .await
}

#[tauri::command]
async fn pr_set_open(app: AppHandle, number: u64, open: bool) -> Res<github::Pull> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::set_open(&state.github, &repo(&state)?, number, open)
    })
    .await
}

#[tauri::command]
async fn pr_review(app: AppHandle, number: u64, event: String, body: String) -> Res<()> {
    blocking(move || {
        let state = app.state::<AppState>();
        github::review(&state.github, &repo(&state)?, number, &event, &body)
    })
    .await
}

#[tauri::command]
async fn pr_checkout(
    state: State<'_, AppState>,
    number: u64,
    head_ref: String,
    same_repo: bool,
) -> Res<()> {
    let r = repo(&state)?;
    blocking(move || github::checkout(&r, number, &head_ref, same_repo)).await
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

#[tauri::command]
fn open_url(url: String) -> Res<()> {
    github::open_url(&url)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
        .manage(AppState::default())
        .setup(|app| {
            if let Some(webview) = app.get_webview_window("main") {
                display::unlock_high_refresh_rate(&webview);
                titlebar::setup(&webview);
                // TEMP dev probe: evaluates JS dropped into GITVIBER_PROBE (debug builds only).
                #[cfg(debug_assertions)]
                if let Ok(path) = std::env::var("GITVIBER_PROBE") {
                    let w = webview.clone();
                    std::thread::spawn(move || loop {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        if let Ok(js) = std::fs::read_to_string(&path) {
                            let _ = std::fs::remove_file(&path);
                            let _ = w.eval(&js);
                        }
                    });
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_repo,
            status,
            log,
            commit_files,
            diff_pair,
            media,
            list_dir,
            read_file,
            branches,
            switch_branch,
            worktrees,
            worktree_changes,
            add_worktree,
            stage,
            unstage,
            discard,
            commit,
            push,
            pull,
            fetch,
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
            undo_commit,
            reset,
            drops_pushed,
            revert,
            checkout_commit,
            create_branch_at,
            create_tag,
            github_web_url,
            gh_account,
            pr_list,
            pr_detail,
            pr_attachments,
            pr_files,
            pr_create,
            pr_merge,
            pr_set_open,
            pr_review,
            pr_checkout,
            open_url,
            pty_spawn,
            pty_write,
            pty_resize,
            pty_kill
        ])
        .run(context)
        .expect("error while running GitViber");
}
