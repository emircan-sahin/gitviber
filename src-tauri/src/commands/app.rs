use crate::state::{blocking, AppState, Res};
use crate::{errors, git, launch, menu, process};
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

#[tauri::command]
pub fn pty_resize(state: State<'_, AppState>, id: u32, cols: u16, rows: u16) -> Res<()> {
    state.ptys.resize(id, cols, rows)
}

#[tauri::command]
pub fn pty_kill(state: State<'_, AppState>, id: u32) {
    state.ptys.kill(id)
}

/// What a bug report asks for: app version and commit, OS, and git.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    version: String,
    commit: String,
    os: String,
    arch: String,
    git: Option<String>,
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
            other => other.to_string(),
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
        })
    })
    .await
}

#[tauri::command]
pub fn open_url(url: String) -> Res<()> {
    launch::open_url(&url)
}

/// The page's errors (src/lib/app/errorLog.ts), into the app's error log.
#[tauri::command]
pub fn log_error(source: String, message: String) {
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
