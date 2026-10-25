//! Obsidian vaults in the explorer (obsidian.rs). Each command names its vault by path, and
//! only a vault Obsidian lists is accepted.

use crate::git::FileText;
use crate::obsidian::{self, Entry, Vault};
use crate::state::{Core, Res};
use std::path::Path;

/// `f` with the vault's folder.
fn in_vault<T>(vault: String, f: impl FnOnce(&Path) -> Res<T>) -> Res<T> {
    f(&obsidian::root(&vault)?)
}

pub fn vaults(_core: &Core) -> Res<Vec<Vault>> {
    Ok(obsidian::vaults())
}

pub fn vault_list_dir(_core: &Core, vault: String, path: String) -> Res<Vec<Entry>> {
    in_vault(vault, move |r| obsidian::list_dir(r, &path))
}

pub fn vault_files(_core: &Core, vault: String) -> Res<Vec<String>> {
    in_vault(vault, obsidian::list_files)
}

pub fn vault_read_file(_core: &Core, vault: String, path: String) -> Res<FileText> {
    in_vault(vault, move |r| Ok(obsidian::read_file(r, &path)))
}

/// Raw bytes, as the repo's media command sends them.
pub fn vault_media(_core: &Core, vault: String, path: String) -> Res<Vec<u8>> {
    in_vault(vault, move |r| obsidian::read_media(r, &path))
}

pub fn vault_write_file(_core: &Core, vault: String, path: String, content: String) -> Res<()> {
    in_vault(vault, move |r| obsidian::write_file(r, &path, &content))
}

/// Shows a vault file in the file manager.
pub fn vault_reveal(_core: &Core, vault: String, path: String) -> Res<()> {
    in_vault(vault, move |r| {
        crate::launch::reveal(obsidian::resolve(r, &path)?)
    })
}

/// Opens a vault file in Obsidian.
pub fn vault_open_in_obsidian(_core: &Core, vault: String, path: String) -> Res<()> {
    in_vault(vault, move |r| {
        crate::launch::open_in_obsidian(&obsidian::resolve(r, &path)?)
    })
}

/// Watches `vault` (none: stops watching), replacing the vault watched before.
pub fn vault_watch(core: &Core, vault: Option<String>) -> Res<()> {
    let watcher = match vault {
        Some(v) => {
            obsidian::root(&v)?;
            Some(obsidian::watch(core.events(), v)?)
        }
        None => None,
    };
    *core.vault_watcher.lock().unwrap_or_else(|e| e.into_inner()) = watcher;
    Ok(())
}
