//! Obsidian vaults, as Obsidian itself lists them (its obsidian.json): their files listed, read,
//! and notes saved back. A vault is only ever one Obsidian lists, and every path is resolved
//! inside it and refused if it would escape it, symlinks included, or go through a hidden
//! folder: Obsidian shows none, and .obsidian/plugins holds code Obsidian runs.

use crate::git::{self, FileText};
use notify::{recommended_watcher, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Vault {
    pub id: String,
    /// Its folder's name, as Obsidian's vault switcher shows it.
    pub name: String,
    pub path: String,
    /// Open in Obsidian right now.
    pub open: bool,
    /// Last opened, in ms since the epoch.
    pub ts: u64,
}

/// Where Obsidian keeps its vault list: its config folder, as Electron places it, and the
/// Flatpak and Snap ones on Linux.
fn config_files() -> Vec<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let home = env("HOME");
    let mut dirs = Vec::new();
    if cfg!(target_os = "macos") {
        dirs.extend(
            home.as_ref()
                .map(|h| h.join("Library/Application Support/obsidian")),
        );
    } else if cfg!(windows) {
        dirs.extend(env("APPDATA").map(|d| d.join("obsidian")));
    } else {
        dirs.extend(
            env("XDG_CONFIG_HOME")
                .or_else(|| home.as_ref().map(|h| h.join(".config")))
                .map(|d| d.join("obsidian")),
        );
        if let Some(h) = &home {
            dirs.push(h.join(".var/app/md.obsidian.Obsidian/config/obsidian"));
            dirs.push(h.join("snap/obsidian/current/.config/obsidian"));
        }
    }
    dirs.into_iter().map(|d| d.join("obsidian.json")).collect()
}

/// The vaults in one obsidian.json: `{"vaults": {"<id>": {"path", "ts", "open"}}}`.
pub fn parse_vaults(json: &str) -> Vec<Vault> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return vec![];
    };
    let Some(vaults) = value.get("vaults").and_then(|v| v.as_object()) else {
        return vec![];
    };
    vaults
        .iter()
        .filter_map(|(id, v)| {
            let path = v.get("path")?.as_str()?.to_string();
            let name = Path::new(&path).file_name()?.to_string_lossy().into_owned();
            Some(Vault {
                id: id.clone(),
                name,
                path,
                open: v.get("open").and_then(|o| o.as_bool()).unwrap_or(false),
                ts: v.get("ts").and_then(|t| t.as_u64()).unwrap_or(0),
            })
        })
        .collect()
}

/// Every vault Obsidian lists whose folder is there, the last opened first.
pub fn vaults() -> Vec<Vault> {
    let mut seen = HashSet::new();
    let mut all: Vec<Vault> = config_files()
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .flat_map(|json| parse_vaults(&json))
        .filter(|v| Path::new(&v.path).is_dir())
        .filter(|v| seen.insert(Path::new(&v.path).canonicalize().ok()))
        .collect();
    all.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| a.name.cmp(&b.name)));
    all
}

/// `vault`'s folder, when it's one Obsidian lists: the page can't point these commands anywhere else.
pub fn root(vault: &str) -> Result<PathBuf, String> {
    vaults()
        .into_iter()
        .find(|v| v.path == vault)
        .map(|v| PathBuf::from(v.path))
        .ok_or_else(|| format!("not an Obsidian vault: {vault}"))
}

fn hidden(name: &std::ffi::OsStr) -> bool {
    matches!(name.as_encoded_bytes().first(), Some(b'.'))
}

/// `rel` (vault-relative, "/" between folders) inside `root`, as crate::fs::resolve does for the
/// repo: no `..`, no hidden folder, and the deepest part that exists really inside the vault.
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let escape = || format!("path outside the vault: {rel}");
    let rel_path = Path::new(rel);
    if rel_path
        .components()
        .any(|c| !matches!(c, Component::Normal(n) if !hidden(n)))
    {
        return Err(escape());
    }
    let real_root = root.canonicalize().map_err(|e| e.to_string())?;
    let full = root.join(rel_path);
    // A path that doesn't exist yet can't be canonicalized: check its deepest existing
    // ancestor, and refuse dangling links (writing would follow them).
    let mut probe = full.as_path();
    let real = loop {
        match probe.canonicalize() {
            Ok(p) => break p,
            Err(_) if probe.symlink_metadata().is_ok() => return Err(escape()),
            Err(_) => probe = probe.parent().ok_or_else(escape)?,
        }
    };
    // A link inside the vault can still lead into its .obsidian.
    match real.strip_prefix(&real_root) {
        Ok(inside) if !inside.components().any(|c| hidden(c.as_os_str())) => Ok(full),
        _ => Err(escape()),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

fn child(rel: &str, name: &str) -> String {
    if rel.is_empty() {
        name.to_string()
    } else {
        format!("{rel}/{name}")
    }
}

/// A folder's entries as Obsidian's file explorer has them: no hidden ones, and no links that
/// lead out of the vault. Unsorted; the page orders them.
pub fn list_dir(root: &Path, rel: &str) -> Result<Vec<Entry>, String> {
    let dir = resolve(root, rel)?;
    Ok(std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|e| !hidden(&e.file_name()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = child(rel, &name);
            let full = resolve(root, &path).ok()?;
            Some(Entry {
                is_dir: full.is_dir(),
                name,
                path,
            })
        })
        .collect())
}

/// More than any vault has; a folder of generated files past it isn't walked further.
const MAX_FILES: usize = 200_000;

/// Every file in the vault, for links to find their notes and attachments. Linked folders
/// aren't walked (one could lead back up), their files are listed under their own folder.
pub fn list_files(root: &Path) -> Result<Vec<String>, String> {
    resolve(root, "")?;
    let mut out = Vec::new();
    let mut dirs = vec![String::new()];
    while let Some(rel) = dirs.pop() {
        let Ok(read) = std::fs::read_dir(root.join(&rel)) else {
            continue;
        };
        for e in read.filter_map(Result::ok) {
            if hidden(&e.file_name()) || out.len() >= MAX_FILES {
                continue;
            }
            let path = child(&rel, &e.file_name().to_string_lossy());
            match e.file_type() {
                Ok(t) if t.is_dir() => dirs.push(path),
                Ok(t) if t.is_file() => out.push(path),
                Ok(t) if t.is_symlink() && resolve(root, &path).is_ok_and(|p| p.is_file()) => {
                    out.push(path)
                }
                _ => {}
            }
        }
    }
    Ok(out)
}

pub fn read_file(root: &Path, rel: &str) -> FileText {
    match resolve(root, rel).and_then(|p| git::read_regular(&p)) {
        Ok(Some(bytes)) => git::to_file_text(bytes),
        Ok(None) => FileText {
            too_large: true,
            exists: true,
            ..Default::default()
        },
        Err(_) => FileText::default(),
    }
}

pub fn read_media(root: &Path, rel: &str) -> Result<Vec<u8>, String> {
    let path = resolve(root, rel)?;
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    if meta.len() > git::MAX_MEDIA_BYTES {
        return Err("File is too large to preview".into());
    }
    std::fs::read(path).map_err(|e| e.to_string())
}

/// Saves a note edited in the code view. Only into a folder that's there: nothing is created.
pub fn write_file(root: &Path, rel: &str, content: &str) -> Result<(), String> {
    let path = resolve(root, rel)?;
    if !path.parent().is_some_and(Path::is_dir) {
        return Err(format!("no such folder in the vault: {rel}"));
    }
    std::fs::write(path, content).map_err(|e| e.to_string())
}

#[derive(Serialize, Clone)]
pub struct VaultChanged {
    pub vault: String,
}

/// Tells the page when files in the vault change, once they go quiet for 150 ms (at least
/// every second while writes go on). Obsidian rewrites .obsidian/workspace.json as you click
/// around in it; hidden paths don't count.
pub fn watch(app: AppHandle, vault: String) -> Result<RecommendedWatcher, String> {
    let root = PathBuf::from(&vault);
    let real = root.canonicalize().unwrap_or_else(|_| root.clone());
    let (tx, rx) = mpsc::channel::<()>();
    let mut watcher = recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        let counts = |p: &PathBuf| {
            let inside = p.strip_prefix(&root).or_else(|_| p.strip_prefix(&real));
            inside.is_ok_and(|rel| !rel.components().any(|c| hidden(c.as_os_str())))
        };
        if event.need_rescan() || event.paths.iter().any(counts) {
            let _ = tx.send(());
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(Path::new(&vault), RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    // The thread ends when the watcher (and with it the sender) is dropped.
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(1)
                && rx.recv_timeout(Duration::from_millis(150)).is_ok()
            {}
            let _ = app.emit(
                "vault-changed",
                VaultChanged {
                    vault: vault.clone(),
                },
            );
        }
    });
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gitviber-vault-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_obsidians_vault_list() {
        let json = r#"{"vaults":{"a1b2":{"path":"/notes/Work Notes","ts":1700000000000,"open":true},"c3d4":{"path":"/notes/Home","ts":1600000000000},"bad":{"ts":1}},"frame":"hidden"}"#;
        let vaults = parse_vaults(json);
        assert_eq!(vaults.len(), 2);
        let work = vaults.iter().find(|v| v.id == "a1b2").unwrap();
        assert_eq!(
            (work.name.as_str(), work.path.as_str(), work.open, work.ts),
            ("Work Notes", "/notes/Work Notes", true, 1700000000000)
        );
        assert!(!vaults.iter().find(|v| v.id == "c3d4").unwrap().open);
        assert!(parse_vaults("not json").is_empty());
        assert!(parse_vaults(r#"{"other":1}"#).is_empty());
    }

    #[test]
    fn paths_stay_inside_the_vault_and_out_of_hidden_folders() {
        let vault = temp("resolve");
        let outside = temp("outside");
        std::fs::create_dir_all(vault.join(".obsidian/plugins")).unwrap();
        std::fs::create_dir_all(vault.join("Daily")).unwrap();
        std::fs::write(vault.join("Daily/Today.md"), "# Today").unwrap();
        std::fs::write(outside.join("secret.md"), "no").unwrap();
        assert!(resolve(&vault, "Daily/Today.md").is_ok());
        assert!(resolve(&vault, "Daily/New.md").is_ok());
        for bad in [
            "../outside/secret.md",
            "/etc/passwd",
            ".obsidian/plugins/x/main.js",
            "Daily/../../x",
            ".trash/old.md",
        ] {
            assert!(resolve(&vault, bad).is_err(), "{bad}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, vault.join("Linked")).unwrap();
            std::os::unix::fs::symlink(vault.join(".obsidian"), vault.join("Config")).unwrap();
            std::os::unix::fs::symlink(vault.join("Daily"), vault.join("Journal")).unwrap();
            assert!(resolve(&vault, "Linked/secret.md").is_err());
            assert!(resolve(&vault, "Config/plugins").is_err());
            assert!(resolve(&vault, "Journal/Today.md").is_ok());
            let names: Vec<String> = list_dir(&vault, "")
                .unwrap()
                .into_iter()
                .map(|e| e.name)
                .collect();
            assert!(names.contains(&"Daily".to_string()) && names.contains(&"Journal".to_string()));
            assert!(!names
                .iter()
                .any(|n| n == ".obsidian" || n == "Linked" || n == "Config"));
        }
        let mut files = list_files(&vault).unwrap();
        files.sort();
        assert_eq!(files, ["Daily/Today.md"]);
        assert!(write_file(&vault, "Daily/Today.md", "# Now").is_ok());
        assert_eq!(read_file(&vault, "Daily/Today.md").text, "# Now");
        assert!(write_file(&vault, "Nope/New.md", "x").is_err());
        let _ = std::fs::remove_dir_all(&vault);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
