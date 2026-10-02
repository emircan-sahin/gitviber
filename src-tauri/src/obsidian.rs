//! Obsidian vaults, as Obsidian itself lists them (its obsidian.json): their files listed, read,
//! and notes saved back. A vault is only ever one Obsidian lists, and every path is resolved
//! inside it and refused if it would escape it, symlinks included, or go through a hidden
//! folder: Obsidian shows none, and .obsidian/plugins holds code Obsidian runs.

use crate::fs::{confine, read_media_at, read_text_at};
use crate::git::FileText;
use crate::watch::debounce;
use notify::{recommended_watcher, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Vault {
    #[serde(skip)]
    pub id: String,
    /// Its folder's name, as Obsidian's vault switcher shows it.
    pub name: String,
    pub path: String,
    /// Open in Obsidian right now.
    #[serde(skip)]
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

/// Every vault the lists in `configs` name, as written.
fn listed(configs: &[PathBuf]) -> impl Iterator<Item = Vault> + '_ {
    configs
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .flat_map(|json| parse_vaults(&json))
}

/// Every vault Obsidian lists whose folder is there, the last opened first.
pub fn vaults() -> Vec<Vault> {
    vaults_in(&config_files())
}

pub(crate) fn vaults_in(configs: &[PathBuf]) -> Vec<Vault> {
    let mut seen = HashSet::new();
    let mut all: Vec<Vault> = listed(configs)
        .filter(|v| Path::new(&v.path).is_dir())
        .filter(|v| seen.insert(Path::new(&v.path).canonicalize().ok()))
        .collect();
    all.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| a.name.cmp(&b.name)));
    all
}

/// `vault`'s folder, when it's one Obsidian lists: the page can't point these commands anywhere else.
pub fn root(vault: &str) -> Result<PathBuf, String> {
    root_in(&config_files(), vault)
}

/// Runs on every call (a note's 30 images are 30): only the vault asked for is looked at on disk.
pub(crate) fn root_in(configs: &[PathBuf], vault: &str) -> Result<PathBuf, String> {
    listed(configs)
        .find(|v| v.path == vault)
        .map(|v| PathBuf::from(v.path))
        .filter(|p| p.is_dir())
        .ok_or_else(|| format!("not an Obsidian vault: {vault}"))
}

fn hidden(name: &std::ffi::OsStr) -> bool {
    matches!(name.as_encoded_bytes().first(), Some(b'.'))
}

/// `rel` (vault-relative, "/" between folders) inside `root`: crate::fs's confinement, with
/// Obsidian's rule in place of git's: no `..` and nothing hidden, as written or as a link leads.
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    resolve_under(root, &root.canonicalize().map_err(|e| e.to_string())?, rel)
}

/// `resolve` with the root's real path read once, for many paths under it.
fn resolve_under(root: &Path, real_root: &Path, rel: &str) -> Result<PathBuf, String> {
    let escape = || format!("path outside the vault: {rel}");
    let rel_path = Path::new(rel);
    if rel_path
        .components()
        .any(|c| !matches!(c, Component::Normal(n) if !hidden(n)))
    {
        return Err(escape());
    }
    let full = root.join(rel_path);
    let real = confine(real_root, &full).ok_or_else(escape)?;
    // A link inside the vault can still lead into its .obsidian.
    let inside = real.strip_prefix(real_root).map_err(|_| escape())?;
    if inside.components().any(|c| hidden(c.as_os_str())) {
        return Err(escape());
    }
    Ok(full)
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
    let real_root = root.canonicalize().map_err(|e| e.to_string())?;
    let dir = resolve_under(root, &real_root, rel)?;
    Ok(std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|e| !hidden(&e.file_name()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = child(rel, &name);
            let full = resolve_under(root, &real_root, &path).ok()?;
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
    let real_root = root.canonicalize().map_err(|e| e.to_string())?;
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
                Ok(t)
                    if t.is_symlink()
                        && resolve_under(root, &real_root, &path).is_ok_and(|p| p.is_file()) =>
                {
                    out.push(path)
                }
                _ => {}
            }
        }
    }
    Ok(out)
}

pub fn read_file(root: &Path, rel: &str) -> FileText {
    resolve(root, rel)
        .map(|p| read_text_at(&p))
        .unwrap_or_default()
}

pub fn read_media(root: &Path, rel: &str) -> Result<Vec<u8>, String> {
    read_media_at(&resolve(root, rel)?)
}

/// Saves a note edited in the code view, over the file it was read from: nothing is created.
pub fn write_file(root: &Path, rel: &str, content: &str) -> Result<(), String> {
    let path = resolve(root, rel)?;
    if !path.is_file() {
        return Err(format!("{rel} is no longer in the vault"));
    }
    std::fs::write(path, content).map_err(|e| e.to_string())
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct VaultChanged {
    pub vault: String,
    /// Files came or went (or were renamed): the vault's file list is stale, not just texts.
    pub files: bool,
    /// The files changed (vault-relative); none when events were dropped and anything may have.
    pub paths: Vec<String>,
}

/// What one watcher event says about the vault at `root` (`real`: its canonical path): the
/// files it touched, and whether any came, went or moved. Hidden paths don't count: Obsidian
/// rewrites .obsidian/workspace.json as you click around in it. None: nothing that counts.
pub(crate) fn vault_event(
    root: &Path,
    real: &Path,
    event: &notify::Event,
) -> Option<(bool, Vec<String>)> {
    if event.need_rescan() {
        return Some((true, vec![]));
    }
    let paths: Vec<String> = event
        .paths
        .iter()
        .filter_map(|p| p.strip_prefix(root).or_else(|_| p.strip_prefix(real)).ok())
        .filter(|rel| {
            !rel.as_os_str().is_empty() && !rel.components().any(|c| hidden(c.as_os_str()))
        })
        .map(|rel| {
            let parts: Vec<_> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect();
            parts.join("/")
        })
        .collect();
    if paths.is_empty() {
        return None;
    }
    let files = matches!(
        event.kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(notify::event::ModifyKind::Name(_))
            | EventKind::Any
    );
    Some((files, paths))
}

/// Tells the page what changed in the vault, once writes go quiet (watch::debounce).
pub fn watch(app: AppHandle, vault: String) -> Result<RecommendedWatcher, String> {
    let root = PathBuf::from(&vault);
    let real = root.canonicalize().unwrap_or_else(|_| root.clone());
    let (tx, rx) = mpsc::channel::<(bool, Vec<String>)>();
    let mut watcher = recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Some(change) = res.ok().and_then(|e| vault_event(&root, &real, &e)) {
            let _ = tx.send(change);
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(Path::new(&vault), RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    // The thread ends when the watcher (and with it the sender) is dropped.
    std::thread::spawn(move || {
        debounce(rx, |batch| {
            let files = batch.iter().any(|(files, _)| *files);
            // A rescan says nothing about which files: none named, every one may have changed.
            let all = batch.iter().any(|(_, paths)| paths.is_empty());
            let mut paths: Vec<String> = if all {
                vec![]
            } else {
                batch.into_iter().flat_map(|(_, p)| p).collect()
            };
            paths.sort();
            paths.dedup();
            let _ = app.emit(
                "vault-changed",
                VaultChanged {
                    vault: vault.clone(),
                    files: files || all,
                    paths,
                },
            );
        })
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
        // Saving goes over the note it was read from; a new file isn't made.
        assert!(write_file(&vault, "Daily/New.md", "x").is_err());
        let _ = std::fs::remove_dir_all(&vault);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn vaults_come_from_the_list_and_only_those_are_accepted() {
        let dir = temp("config");
        let (work, gone) = (dir.join("Work"), dir.join("Gone"));
        std::fs::create_dir_all(&work).unwrap();
        let config = dir.join("obsidian.json");
        let json = serde_json::json!({ "vaults": {
            "a": { "path": work.to_string_lossy(), "ts": 2 },
            "b": { "path": gone.to_string_lossy(), "ts": 3 },
            "c": { "path": work.to_string_lossy(), "ts": 1 },
        }});
        std::fs::write(&config, json.to_string()).unwrap();
        let configs = [config, dir.join("missing.json")];
        let found = vaults_in(&configs);
        assert_eq!(
            found.len(),
            1,
            "a missing folder is left out, the same folder listed once"
        );
        assert_eq!(found[0].path, work.to_string_lossy());
        assert!(root_in(&configs, &work.to_string_lossy()).is_ok());
        assert!(root_in(&configs, &gone.to_string_lossy()).is_err());
        assert!(root_in(&configs, &dir.to_string_lossy()).is_err());
        // The page never sees Obsidian's ids or open state.
        let sent = serde_json::to_value(&found[0]).unwrap();
        assert!(sent.get("id").is_none() && sent.get("open").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_watcher_event_counts_unless_hidden_and_says_when_files_came_or_went() {
        use notify::event::{CreateKind, DataChange, ModifyKind, RenameMode};
        let (root, real) = (Path::new("/v"), Path::new("/private/v"));
        let event = |kind, paths: &[&str]| notify::Event {
            kind,
            paths: paths.iter().map(PathBuf::from).collect(),
            attrs: Default::default(),
        };
        let edit = EventKind::Modify(ModifyKind::Data(DataChange::Content));
        assert_eq!(
            vault_event(root, real, &event(edit, &["/v/Daily/Today.md"])),
            Some((false, vec!["Daily/Today.md".into()]))
        );
        assert_eq!(
            vault_event(root, real, &event(edit, &["/private/v/A.md"])),
            Some((false, vec!["A.md".into()]))
        );
        assert_eq!(
            vault_event(
                root,
                real,
                &event(EventKind::Create(CreateKind::File), &["/v/New.md"])
            ),
            Some((true, vec!["New.md".into()]))
        );
        assert_eq!(
            vault_event(
                root,
                real,
                &event(
                    EventKind::Modify(ModifyKind::Name(RenameMode::Any)),
                    &["/v/Old.md"]
                )
            )
            .map(|c| c.0),
            Some(true)
        );
        assert_eq!(
            vault_event(root, real, &event(edit, &["/v/.obsidian/workspace.json"])),
            None
        );
        assert_eq!(
            vault_event(root, real, &event(edit, &["/elsewhere/x.md"])),
            None
        );
    }
}
