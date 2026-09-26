use crate::state::{blocking, AppState, Res};
use crate::{clipboard, errors, git, launch, menu, process, updates};
use std::path::Path;
use tauri::{AppHandle, Manager, State};

/// Any folder, unlike the repo commands: the shell can `cd` anywhere the user can anyway.
#[tauri::command]
pub fn pty_spawn(
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
pub fn pty_write(state: State<'_, AppState>, id: u32, data: String) -> Res<()> {
    state.ptys.write(id, &data)
}

/// Sync, so it runs on the main thread, where AppKit's pasteboard belongs.
#[tauri::command]
pub fn terminal_paste() -> Res<clipboard::Paste> {
    clipboard::read()
}

/// Sync for the same reason as `terminal_paste`.
#[tauri::command]
pub fn copy_files(paths: Vec<String>) -> Res<()> {
    clipboard::copy_files(paths)
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
pub fn pty_kill(state: State<'_, AppState>, id: u32) {
    state.ptys.kill(id)
}

/// How many terminals are running a command, which restarting the app would stop.
#[tauri::command]
pub fn pty_busy(state: State<'_, AppState>) -> usize {
    state.ptys.busy()
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
        let text = |cmd: &str, args: &[&str]| {
            std::process::Command::new(cmd)
                .args(args)
                .env("PATH", process::search_path())
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
