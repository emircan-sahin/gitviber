use crate::state::{blocking, AppState, Res};
use crate::{
    agents, cli, clipboard, errors, git, launch, menu, notifications, process, pty, updates,
};
use std::path::Path;
use tauri::{AppHandle, Emitter, Manager, State};

/// Any folder, unlike the repo commands: the shell can `cd` anywhere the user can anyway.
/// `integration`: load the shell integration, whose scripts live in the app's cache folder.
/// `folder`: where the shell it continues was last (a split, a restore), if that's still there.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn pty_spawn(
    app: AppHandle,
    state: State<'_, AppState>,
    cwd: String,
    cols: u16,
    rows: u16,
    integration: bool,
    folder: Option<String>,
    output: tauri::ipc::Channel<tauri::ipc::Response>,
    exit: tauri::ipc::Channel<Option<pty::Exit>>,
) -> Res<pty::Spawned> {
    let cwd = folder.filter(|f| Path::new(f).is_dir()).unwrap_or(cwd);
    let scripts = integration
        .then(|| app.path().app_cache_dir().ok())
        .flatten()
        .map(|dir| dir.join("shell-integration"));
    state.ptys.spawn(
        Path::new(&cwd),
        cols,
        rows,
        scripts.as_deref(),
        output,
        exit,
    )
}

/// Off the main thread: a paste into a program that isn't reading blocks until it reads.
#[tauri::command]
pub async fn pty_write(state: State<'_, AppState>, id: u32, data: String, binary: bool) -> Res<()> {
    let writer = state.ptys.writer(id)?;
    blocking(move || pty::write(&writer, &pty::input_bytes(&data, binary))).await
}

/// Sync, so it runs on the main thread, where AppKit's pasteboard and GTK's clipboard belong.
#[tauri::command]
pub fn terminal_paste() -> Res<clipboard::Paste> {
    clipboard::read()
}

/// Sync for the same reason as `terminal_paste`.
#[tauri::command]
pub fn copy_files(paths: Vec<String>) -> Res<()> {
    clipboard::copy_files(paths)
}

/// What a program in the terminal copies (OSC 52). Sync for the same reason as `terminal_paste`.
#[tauri::command]
pub fn terminal_copy(text: String) -> Res<()> {
    clipboard::write_text(&text)
}

/// The `gitviber` command (cli.rs).
#[tauri::command]
pub async fn install_cli() -> Res<String> {
    blocking(cli::install).await
}

/// Paths opened from outside the window since the page last asked (opened.rs). Off the main
/// thread: resolving a path on a stalled network volume can hang.
#[tauri::command]
pub async fn take_opened(app: AppHandle) -> Res<crate::opened::Taken> {
    blocking(move || Ok(crate::opened::take(&app))).await
}

#[tauri::command]
pub fn keep_dropped(paths: Vec<String>) -> Vec<String> {
    clipboard::keep_dropped(paths)
}

#[tauri::command]
pub fn pty_resize(state: State<'_, AppState>, id: u32, cols: u16, rows: u16) -> Res<()> {
    state.ptys.resize(id, cols, rows)
}

#[tauri::command]
pub fn pty_ack(state: State<'_, AppState>, id: u32, bytes: usize) {
    state.ptys.ack(id, bytes)
}

#[tauri::command]
pub fn pty_cwd(state: State<'_, AppState>, id: u32) -> Option<String> {
    state.ptys.cwd(id).map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn pty_kill(state: State<'_, AppState>, id: u32) {
    state.agents.forget(id);
    state.ptys.kill(id)
}

/// How many terminals (of `ids`, or all) are running a command, which restarting the app or
/// killing them would stop.
#[tauri::command]
pub fn pty_busy(state: State<'_, AppState>, ids: Option<Vec<u32>>) -> usize {
    state.ptys.busy(ids.as_deref())
}

/// The coding agents (agents.rs) the terminals `ids` run, by id; a terminal with none isn't
/// listed. Each one's state file is watched from then on, its changes sent as "agent-state".
#[tauri::command]
pub async fn pty_agents(
    app: AppHandle,
    ids: Vec<u32>,
) -> Res<std::collections::HashMap<u32, agents::Agent>> {
    blocking(move || {
        let state = app.state::<AppState>();
        let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
            return Ok(Default::default());
        };
        let mut running = std::collections::HashMap::new();
        for id in ids {
            let leader = state.ptys.foreground(id);
            let sink = || -> agents::Sink {
                let app = app.clone();
                std::sync::Arc::new(move |id, state| {
                    let _ = app.emit("agent-state", AgentState { id, state });
                })
            };
            if let Some(agent) = state.agents.agent(id, leader, &home, sink) {
                running.insert(id, agent);
            }
        }
        Ok(running)
    })
    .await
}

#[derive(serde::Serialize, Clone)]
struct AgentState {
    id: u32,
    state: Option<&'static str>,
}

/// Which of `paths` are folders still: a restored agent resumes only where it ran.
#[tauri::command]
pub fn folders_left(paths: Vec<String>) -> Vec<bool> {
    paths.iter().map(|p| Path::new(p).is_dir()).collect()
}

/// The page saved what it keeps on the way out (menu::quit).
#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}

/// See updates.rs.
#[tauri::command]
pub fn update_mode(app: AppHandle) -> Option<&'static str> {
    updates::mode(app.config())
}

/// What a bug report asks for: app version and commit, OS, git, gh and the web view. Nothing
/// about the user or their repos.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    version: String,
    commit: String,
    os: String,
    arch: String,
    git: Option<String>,
    /// `gh --version`'s version, None when the GitHub CLI isn't installed.
    gh: Option<String>,
    /// "WebKit 20621.1.15" on macOS, "WebKitGTK 2.46.3" on Linux.
    webview: Option<String>,
}

#[tauri::command]
pub async fn about(app: AppHandle) -> Res<About> {
    let version = app.package_info().version.to_string();
    blocking(move || {
        let state = app.state::<AppState>();
        let checked = state.git.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let text = |program: &str, args: &[&str]| {
            let mut cmd = std::process::Command::new(program);
            cmd.args(args)
                .env("PATH", process::search_path())
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            process::spawn(&mut cmd)
                .and_then(|c| c.wait_with_output())
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };
        let os = match std::env::consts::OS {
            "macos" => format!(
                "macOS {}",
                text("sw_vers", &["-productVersion"]).unwrap_or_default()
            ),
            // "Ubuntu 24.04.1 LTS", "Fedora Linux 40 (Workstation Edition)"
            "linux" => std::fs::read_to_string("/etc/os-release")
                .ok()
                .and_then(|f| {
                    let name = f.lines().find_map(|l| l.strip_prefix("PRETTY_NAME="))?;
                    Some(name.trim_matches('"').to_string())
                })
                .unwrap_or_else(|| "Linux".into()),
            other => other.to_string(),
        };
        let engine = match std::env::consts::OS {
            "macos" => "WebKit",
            "linux" => "WebKitGTK",
            _ => "WebView",
        };
        Ok(About {
            version,
            commit: env!("GITVIBER_COMMIT").to_string(),
            os: os.trim().to_string(),
            arch: std::env::consts::ARCH.to_string(),
            // git_info checks it at launch; asked again only if that found none.
            git: checked
                .and_then(|g| g.version)
                .or_else(|| git::check_install().version),
            // "gh version 2.62.0 (2024-11-14)\nhttps://github.com/cli/cli/releases/…"
            gh: text("gh", &["--version"]).and_then(|v| {
                let first = v.lines().next()?;
                Some(first.trim_start_matches("gh version ").to_string())
            }),
            webview: tauri::webview_version()
                .ok()
                .map(|v| format!("{engine} {v}")),
        })
    })
    .await
}

#[tauri::command]
pub fn open_url(url: String) -> Res<()> {
    launch::open_url(&url)
}

/// Help → Show Logs: the error log selected in the file manager, or its folder before the first
/// error has created it.
#[tauri::command]
pub async fn show_logs() -> Res<()> {
    let file = errors::file().ok_or("The log folder could not be found.")?;
    let path = if file.exists() {
        file
    } else {
        file.parent().unwrap_or(file)
    };
    let path = path.to_path_buf();
    blocking(move || launch::reveal(path)).await
}

/// The page's errors (src/lib/app/errorLog.ts), into the app's error log. Async, so the file
/// write for every error toast stays off the main thread.
#[tauri::command]
pub async fn log_error(source: String, message: String) {
    errors::write(&source, &message);
}

#[tauri::command]
pub fn set_menu(
    app: AppHandle,
    handles: State<'_, menu::Handles>,
    items: std::collections::HashMap<String, menu::ItemState>,
    recent: Option<Vec<menu::Recent>>,
) -> Res<()> {
    menu::update(&app, &handles, items, recent).map_err(|e| e.to_string())
}

/// Whether the OS shows GitViber's notifications (notifications.rs).
#[tauri::command]
pub async fn notification_permission() -> Res<notifications::Permission> {
    blocking(notifications::permission).await
}

/// Asks the OS for it; waits while the user answers its prompt.
#[tauri::command]
pub async fn notification_request() -> Res<notifications::Permission> {
    blocking(notifications::request).await
}

/// `target`: given back with "notification-click" when the user clicks it.
#[tauri::command]
pub async fn notification_send(
    app: AppHandle,
    title: String,
    body: String,
    target: Option<String>,
) -> Res<()> {
    blocking(move || notifications::send(&app, &title, &body, target.as_deref())).await
}

#[tauri::command]
pub async fn notification_settings(app: AppHandle) -> Res<()> {
    let id = app.config().identifier.clone();
    blocking(move || launch::notification_settings(&id)).await
}
