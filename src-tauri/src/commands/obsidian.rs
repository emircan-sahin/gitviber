//! Obsidian vaults in the explorer (obsidian.rs). Each command names its vault by path, and
//! only a vault Obsidian lists is accepted.

use crate::git::FileText;
use crate::obsidian::{self, Entry, Vault};
use crate::state::{blocking, AppState, Res};
use std::path::Path;
use tauri::{AppHandle, State};

/// `f` with the vault's folder, on the blocking pool.
async fn in_vault<T: Send + 'static>(
    vault: String,
    f: impl FnOnce(&Path) -> Res<T> + Send + 'static,
) -> Res<T> {
    blocking(move || f(&obsidian::root(&vault)?)).await
}

#[tauri::command]
pub async fn vaults() -> Res<Vec<Vault>> {
    blocking(|| Ok(obsidian::vaults())).await
}

#[tauri::command]
pub async fn vault_list_dir(vault: String, path: String) -> Res<Vec<Entry>> {
    in_vault(vault, move |r| obsidian::list_dir(r, &path)).await
}

#[tauri::command]
pub async fn vault_files(vault: String) -> Res<Vec<String>> {
    in_vault(vault, obsidian::list_files).await
}

#[tauri::command]
pub async fn vault_read_file(vault: String, path: String) -> Res<FileText> {
    in_vault(vault, move |r| Ok(obsidian::read_file(r, &path))).await
}

/// Raw bytes, as the repo's media command sends them.
#[tauri::command]
pub async fn vault_media(vault: String, path: String) -> Res<tauri::ipc::Response> {
    let bytes = in_vault(vault, move |r| obsidian::read_media(r, &path)).await?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub async fn vault_write_file(vault: String, path: String, content: String) -> Res<()> {
    in_vault(vault, move |r| obsidian::write_file(r, &path, &content)).await
}

/// Shows a vault file in the file manager.
#[tauri::command]
pub async fn vault_reveal(vault: String, path: String) -> Res<()> {
    in_vault(vault, move |r| {
        crate::launch::reveal(obsidian::resolve(r, &path)?)
    })
    .await
}

/// Opens a vault file in Obsidian.
#[tauri::command]
pub async fn vault_open_in_obsidian(vault: String, path: String) -> Res<()> {
    in_vault(vault, move |r| {
        crate::launch::open_in_obsidian(&obsidian::resolve(r, &path)?)
    })
    .await
}

/// Watches `vault` (none: stops watching), replacing the vault watched before.
#[tauri::command]
pub async fn vault_watch(
    app: AppHandle,
    state: State<'_, AppState>,
    vault: Option<String>,
) -> Res<()> {
    let watcher = match vault {
        Some(v) => {
            obsidian::root(&v)?;
            Some(obsidian::watch(app, v)?)
        }
        None => None,
    };
    *state
        .vault_watcher
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = watcher;
    Ok(())
}
