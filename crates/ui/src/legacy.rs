//! The old app's data, carried over on the native app's first launch. The Tauri app kept it all
//! in WebKit's localStorage: a sqlite file of UTF-16 values. It's read once and read-only, through
//! the system's sqlite3 (no sqlite in the build); a store that isn't there or can't be read
//! imports nothing, and says nothing.
//!
//! `import_once` writes settings.json and state.json before they load, and holds the old keys
//! while the app's `init` runs: a feature crate's store takes its own from `value` in its `init`,
//! when its file isn't there yet. `finish` lets them go.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};

use serde_json::{Map, Value};

use crate::settings::{DEFAULT_FONT_SIZE, DEFAULT_TERMINAL_FONT_SIZE, Settings};
use crate::state::{
    PanelSize, PanelSizes, RECENT_COMMANDS, SAVED_WORKSPACES, SavedWorkspace, TabSnapshot, UiState,
    WorkspaceSnapshot,
};
use crate::store;

const SQLITE: &str = "/usr/bin/sqlite3";
/// Written once an import ran, so it never runs again.
const MARKER: &str = "migrated";

/// The old store: each key's text.
type Entries = HashMap<String, String>;

static IMPORTING: Mutex<Option<Entries>> = Mutex::new(None);

fn importing() -> MutexGuard<'static, Option<Entries>> {
    IMPORTING.lock().unwrap_or_else(|e| e.into_inner())
}

/// The old app's JSON under `key`, while the first launch imports; None otherwise, or when it
/// isn't JSON.
pub fn value(key: &str) -> Option<Value> {
    serde_json::from_str(&text(key)?).ok()
}

/// The old app's text under `key`, as stored: for a value kept unquoted (gitviber.last).
pub fn text(key: &str) -> Option<String> {
    importing().as_ref()?.get(key).cloned()
}

/// On the first launch (no settings.json, no state.json, never imported), the old app's data:
/// the settings, recent projects and commands, each worktree's tabs and the panel sizes go into
/// their files here, the rest stays readable through `value` until `finish`. Call before
/// `gitviber_ui::init` loads the files.
pub fn import_once() {
    // A snapshot pictures the app as its config dir has it.
    if std::env::var_os("GITVIBER_SNAPSHOT").is_some() {
        return;
    }
    let dir = store::config_dir();
    if first_launch(&dir)
        && let Some(entries) = import_from(&dir, &stores())
    {
        *importing() = Some(entries);
    }
}

/// Every store has taken what it imports: the old keys go.
pub fn finish() {
    *importing() = None;
}

fn first_launch(dir: &Path) -> bool {
    ["settings.json", "state.json", MARKER]
        .iter()
        .all(|name| !dir.join(name).exists())
}

/// Imports the first of `candidates` that reads as the app's store into `dir`; its entries.
fn import_from(dir: &Path, candidates: &[PathBuf]) -> Option<Entries> {
    let (from, entries) = candidates
        .iter()
        .find_map(|path| read(path).map(|e| (path, e)))?;
    let json = |key: &str| {
        entries
            .get(key)
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
    };
    if let Some(Value::Object(old)) = json("gitviber.settings.v2") {
        write(&dir.join("settings.json"), &settings(old));
    }
    let state = ui_state(&json, entries.get("gitviber.last"));
    if state != UiState::default() {
        write(
            &dir.join("state.json"),
            &serde_json::to_string_pretty(&state).unwrap_or_default(),
        );
    }
    let marker = serde_json::json!({ "from": from.to_string_lossy() });
    write(&dir.join(MARKER), &marker.to_string());
    Some(entries)
}

fn write(path: &Path, text: &str) {
    if let Err(err) = store::write_atomic(path, text) {
        eprintln!("gitviber: couldn't write {}: {err}", path.display());
    }
}

// MARK: The old store

/// Where the old app's own store can be, the latest written first. Not WebsiteDataStore's on
/// macOS: those are the browser tabs'.
fn stores() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    const NESTED: &str = "LocalStorage/localstorage.sqlite3";
    let mut found: Vec<PathBuf> = if cfg!(target_os = "macos") {
        let base = home.join("Library/WebKit").join(store::APP_ID);
        nested(&base.join("WebsiteData/Default"), NESTED)
    } else if cfg!(windows) {
        // WebView2 keeps Chromium's leveldb: nothing sqlite3 reads.
        Vec::new()
    } else {
        // Tauri points WebKitGTK at $XDG_DATA_HOME/<id>: its newer layout is macOS's, the older
        // one a file per origin.
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        let base = data.join(store::APP_ID);
        let mut found = nested(&base.join("storage"), NESTED);
        found.extend(
            children(&base.join("localstorage"))
                .into_iter()
                .filter(|p| p.extension().is_some_and(|e| e == "localstorage")),
        );
        found
    };
    found.sort_by_key(|p| std::cmp::Reverse(std::fs::metadata(p).and_then(|m| m.modified()).ok()));
    found
}

fn children(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// The `<dir>/*/*/<tail>` files there are.
fn nested(dir: &Path, tail: &str) -> Vec<PathBuf> {
    children(dir)
        .iter()
        .flat_map(|d| children(d))
        .map(|d| d.join(tail))
        .filter(|p| p.is_file())
        .collect()
}

/// The store's keys and values; None when it can't be read or isn't the app's. Opened
/// read-only, never written: the old app may be running, and its own data stays as it was.
fn read(path: &Path) -> Option<Entries> {
    // -init: a ~/.sqliterc can't change the output. Hex: no value can break a line.
    let out = Command::new(SQLITE)
        .args(["-batch", "-readonly", "-init", "/dev/null"])
        .args(["-noheader", "-list", "-separator", "|"])
        .arg(uri(path))
        .arg("select hex(key), typeof(value), hex(value) from ItemTable")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let entries: Entries = String::from_utf8(out.stdout)
        .ok()?
        .lines()
        .filter_map(entry)
        .collect();
    entries
        .keys()
        .any(|k| k.starts_with("gitviber"))
        .then_some(entries)
}

/// sqlite reads `?` and `#` in a URI as its own, and `%` as an escape.
fn uri(path: &Path) -> String {
    let mut out = String::from("file:");
    for c in path.to_string_lossy().chars() {
        match c {
            '%' => out.push_str("%25"),
            '?' => out.push_str("%3f"),
            '#' => out.push_str("%23"),
            c => out.push(c),
        }
    }
    out + "?mode=ro"
}

/// One row: WebKit stores values as UTF-16LE blobs; one kept as text is UTF-8.
fn entry(line: &str) -> Option<(String, String)> {
    let mut parts = line.splitn(3, '|');
    let key = String::from_utf8(unhex(parts.next()?)?).ok()?;
    let kind = parts.next()?;
    let bytes = unhex(parts.next()?)?;
    let value = if kind == "blob" {
        let (pairs, rest) = bytes.as_chunks::<2>();
        if !rest.is_empty() {
            return None;
        }
        let units: Vec<u16> = pairs.iter().map(|p| u16::from_le_bytes(*p)).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8(bytes).ok()?
    };
    Some((key, value))
}

fn unhex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect()
}

// MARK: Settings and state

/// The old settings as this build keeps them, with what the old app's load still migrated from
/// older builds' forms (it read them so on every launch, and saved them only on a change).
fn settings(mut old: Map<String, Value>) -> String {
    let appearance = old
        .get("appearance")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let dark_default = old.get("darkTheme").is_none_or(|t| t == "dark");
    let dim_variant = old.get("darkVariant").and_then(Value::as_str) == Some("dim");
    // Dimmed was an appearance of its own before the named themes, and System's dark a variant.
    if appearance == "dim" || (appearance != "dark" && dim_variant && dark_default) {
        old.insert("darkTheme".into(), "dim".into());
    }
    if appearance == "dim" {
        old.insert("appearance".into(), "dark".into());
    }
    old.remove("darkVariant");
    // Translucency was Off, Subtle or Strong before the sliders.
    let level = old.remove("translucency");
    let level = match level.as_ref().and_then(Value::as_str) {
        Some("off") => Some((100, 0)),
        Some("subtle") => Some((85, 20)),
        Some("strong") => Some((70, 30)),
        _ => None,
    };
    if let Some((opacity, blur)) = level
        && !old.contains_key("windowOpacity")
    {
        old.insert("windowOpacity".into(), opacity.into());
        old.insert("backgroundBlur".into(), blur.into());
    }
    // Up to 0.1.5 the terminal took the code font's size: one a user enlarged stays.
    if !old.contains_key("terminalFontSize") {
        let code = old
            .get("codeFontSize")
            .and_then(Value::as_f64)
            .unwrap_or(DEFAULT_FONT_SIZE as f64);
        let size = code.round().max(DEFAULT_TERMINAL_FONT_SIZE as f64);
        old.insert("terminalFontSize".into(), size.into());
    }
    let (settings, unknown) = Settings::from_value(&Value::Object(old));
    settings.to_json(&unknown)
}

/// Up to 0.1.10 the panels' layouts were kept as percentages of the window, under these.
const SHARES_OF_WIDTH: &[&str] = &["react-resizable-panels:gitviber-main-v4"];
const SHARES_OF_HEIGHT: &[&str] = &[
    "react-resizable-panels:gitviber-viewer-v1:editor:terminal",
    "react-resizable-panels:gitviber-viewer-v1:editor",
];
/// The window those shares are taken of: the size the window opens at, as both apps have it, since
/// its place isn't carried over.
const WINDOW: (f64, f64) = (1480.0, 920.0);

/// `ids`' sizes in pixels from the first of `keys` that has a share for each of them: 0% was
/// closed, at the size it reopens at.
fn from_shares(
    json: &dyn Fn(&str) -> Option<Value>,
    keys: &[&str],
    ids: &[&str],
    total: f64,
) -> Option<Value> {
    let layout = keys.iter().filter_map(|k| json(k)).find(|l| {
        ids.iter()
            .all(|id| l.get(*id).is_some_and(Value::is_number))
    })?;
    let sizes = ids.iter().map(|id| {
        let share = layout[*id].as_f64().unwrap_or_default();
        let size = if share > 0.0 {
            serde_json::json!({ "size": (share / 100.0 * total).round() })
        } else {
            serde_json::json!({ "collapsed": true })
        };
        (id.to_string(), size)
    });
    Some(Value::Object(sizes.collect()))
}

fn ui_state(json: &dyn Fn(&str) -> Option<Value>, last: Option<&String>) -> UiState {
    let sides = json("gitviber-sides-v1")
        .or_else(|| from_shares(json, SHARES_OF_WIDTH, &["list", "files"], WINDOW.0));
    let bottom = json("gitviber-bottom-v1")
        .or_else(|| from_shares(json, SHARES_OF_HEIGHT, &["terminal"], WINDOW.1));
    let panel = |group: &Option<Value>, id: &str| {
        let saved = group.as_ref().and_then(|g| g.get(id));
        PanelSize {
            size: saved
                .and_then(|s| s.get("size")?.as_f64())
                .filter(|s| *s > 0.0)
                .map(|s| s as f32),
            collapsed: saved.and_then(|s| s.get("collapsed")) == Some(&Value::Bool(true)),
        }
    };
    let mut recent_commands = strings(json("gitviber.palette.commands").as_ref());
    recent_commands.truncate(RECENT_COMMANDS);
    UiState {
        panels: PanelSizes {
            left: panel(&sides, "list"),
            right: panel(&sides, "files"),
            bottom: panel(&bottom, "terminal"),
        },
        recent_projects: recent_projects(json("gitviber.recent").as_ref(), last),
        recent_commands,
        workspaces: workspaces(json("gitviber.workspaces").as_ref()),
        ..UiState::default()
    }
}

fn strings(list: Option<&Value>) -> Vec<String> {
    list.and_then(Value::as_array)
        .map(|l| {
            l.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The old list is the user's order and `last` the one to reopen; here the first is reopened.
fn recent_projects(list: Option<&Value>, last: Option<&String>) -> Vec<String> {
    let mut recent: Vec<String> = Vec::new();
    for root in strings(list) {
        if !recent.contains(&root) {
            recent.push(root);
        }
    }
    // The list holds main worktrees; the last one open may be a linked worktree of one.
    if let Some(last) = last {
        let main = main_of(last);
        if let Some(i) = recent
            .iter()
            .position(|r| r == last || main.as_ref() == Some(r))
        {
            let root = recent.remove(i);
            recent.insert(0, root);
        }
    }
    recent
}

/// A linked worktree's main worktree, from its `.git` file: "gitdir: <main>/.git/worktrees/<name>".
fn main_of(root: &str) -> Option<String> {
    let text = std::fs::read_to_string(Path::new(root).join(".git")).ok()?;
    let gitdir = text.trim().strip_prefix("gitdir: ")?;
    Some(gitdir.rsplit_once("/.git/worktrees/")?.0.to_string())
}

/// The old object by worktree root, the latest saved last; here the latest first. A snapshot
/// the old app wouldn't have restored is left out, and so is a tab that can't be read.
fn workspaces(saved: Option<&Value>) -> Vec<SavedWorkspace> {
    let Some(saved) = saved.and_then(Value::as_object) else {
        return Vec::new();
    };
    saved
        .iter()
        .rev()
        .filter_map(|(root, s)| {
            Some(SavedWorkspace {
                root: root.clone(),
                snapshot: snapshot(s)?,
            })
        })
        .take(SAVED_WORKSPACES)
        .collect()
}

fn snapshot(saved: &Value) -> Option<WorkspaceSnapshot> {
    let text = |key: &str| saved.get(key)?.as_str().map(str::to_string);
    let tabs = saved
        .get("tabs")?
        .as_array()?
        .iter()
        .filter_map(|t| serde_json::from_value::<TabSnapshot>(t.clone()).ok())
        .filter(|t| t.sel.get("kind").is_some_and(Value::is_string))
        .collect();
    let viewed = saved
        .get("viewed")?
        .as_array()?
        .iter()
        .filter_map(|v| serde_json::from_value::<(String, String)>(v.clone()).ok())
        .collect();
    Some(WorkspaceSnapshot {
        tabs,
        active: text("active"),
        list_tab: text("listTab"),
        right_panel: None,
        viewed,
        // "": a review asked for and no base picked yet; not one to go back to.
        review: text("review").filter(|r| !r.is_empty()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Appearance;
    use crate::theme::ThemeId;
    use crate::workspace::{Selection, selection_key};

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gitviber-legacy-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A store as WebKit writes it: values as UTF-16LE blobs. None without the sqlite3 CLI.
    fn fixture(dir: &Path, items: &[(&str, &str)]) -> Option<PathBuf> {
        if !Path::new(SQLITE).exists() {
            eprintln!("no {SQLITE}: skipped");
            return None;
        }
        let path = dir.join("localstorage.sqlite3");
        let mut sql = String::from(
            "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB NOT NULL ON CONFLICT FAIL);\n",
        );
        for (key, value) in items {
            let hex: String = value
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .map(|b| format!("{b:02X}"))
                .collect();
            sql.push_str(&format!(
                "INSERT INTO ItemTable VALUES ('{}', X'{hex}');\n",
                key.replace('\'', "''")
            ));
        }
        let mut child = Command::new(SQLITE)
            .arg(&path)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write as _;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(sql.as_bytes())
            .unwrap();
        assert!(child.wait().unwrap().success());
        Some(path)
    }

    fn read_state(dir: &Path) -> UiState {
        serde_json::from_str(&std::fs::read_to_string(dir.join("state.json")).unwrap()).unwrap()
    }

    const COMMIT: &str = r#"{"sha": "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b", "shortSha": "1a2b3c4",
        "authorName": "A", "authorEmail": "a@x", "timestamp": 1700000000, "committerName": "A",
        "committedAt": 1700000000, "parents": ["0f"], "refs": [], "subject": "Fix", "body": "",
        "unpushed": false, "onOrigin": true, "notInHead": false}"#;
    const FILE: &str = r#"{"path": "src/a.rs", "oldPath": null, "status": "M", "additions": 3,
        "deletions": 1, "oid": null, "indexOid": null, "conflict": null, "mode": null,
        "submodule": null, "nested": null}"#;

    /// Tabs as session.ts saved them, with the keys selection.ts gave them.
    fn old_tabs() -> (String, Vec<&'static str>) {
        let tabs = [
            (r#"{"kind": "file", "path": "README.md"}"#.to_string(), "file::README.md"),
            (format!(r#"{{"kind": "unstaged", "file": {FILE}}}"#), "unstaged::src/a.rs"),
            (
                format!(r#"{{"kind": "commit", "commit": {COMMIT}, "file": {FILE}}}"#),
                "commit:1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b:src/a.rs",
            ),
            (
                format!(
                    r#"{{"kind": "pr-file", "range": {{"number": 7, "label": "x", "base": "b1", "head": "h1"}}, "file": {FILE}}}"#
                ),
                "pr-file:7@b1..h1:src/a.rs",
            ),
            (
                r#"{"kind": "issue", "issue": {"number": 3, "title": "Bug", "state": "OPEN",
                  "stateReason": null, "author": "mona", "labels": [{"name": "bug", "color": "f00", "description": ""}],
                  "assignees": [], "comments": 2, "createdAt": "2026-01-01T00:00:00Z",
                  "updatedAt": "2026-01-02T00:00:00Z", "url": "https://github.com/o/r/issues/3"}}"#
                    .to_string(),
                "issue:https://github.com/o/r/issues/3",
            ),
            (r#"{"kind": "changes", "list": "staged"}"#.to_string(), "changes::All Staged Changes"),
            (
                r#"{"kind": "compare", "base": {"ref": "refs/heads/main", "label": "main"},
                  "head": {"ref": "refs/heads/x", "label": "x"}, "mergeBase": true}"#
                    .to_string(),
                "compare::Compare",
            ),
        ];
        let json = tabs
            .iter()
            .map(|(sel, key)| format!(r#"{{"key": "{key}", "sel": {sel}, "preview": false}}"#))
            .collect::<Vec<_>>()
            .join(",");
        (json, tabs.iter().map(|(_, key)| *key).collect())
    }

    #[test]
    fn a_first_launch_takes_the_old_apps_data() {
        let dir = temp("import");
        let (tabs, keys) = old_tabs();
        let older =
            r#"{"tabs": [], "active": null, "listTab": "pulls", "viewed": [], "review": null}"#;
        let newer = format!(
            r#"{{"tabs": [{tabs}], "active": "unstaged::src/a.rs", "listTab": "history",
                "viewed": [["unstaged:src/a.rs", "sig"]], "review": ""}}"#
        );
        let workspaces = format!(r#"{{"/p/a": {older}, "/p/Café 🎉": {newer}}}"#);
        let settings = r#"{"appearance": "dim", "codeFontSize": 15.5, "uiScale": 1, "lightTheme": "rose-pine-dawn",
            "terminalLineHeight": 1, "keybindings": {"view.toggleExplorer": ["cmd+e"]}, "futureThing": [1]}"#;
        let Some(store) = fixture(
            &dir,
            &[
                ("gitviber.settings.v2", settings),
                (
                    "gitviber.recent",
                    r#"["/p/a", "/p/Café 🎉", "/p/c", "/p/a", null]"#,
                ),
                ("gitviber.last", "/p/Café 🎉"),
                (
                    "gitviber.palette.commands",
                    r#"["view.pulls", "file.openRepo"]"#,
                ),
                ("gitviber.workspaces", &workspaces),
                (
                    "gitviber-sides-v1",
                    r#"{"list": {"size": 300}, "files": {"size": 250, "collapsed": true}}"#,
                ),
                ("gitviber-bottom-v1", r#"{"terminal": {"size": 210}}"#),
                (
                    "gitviber.terminals",
                    r#"{"savedAt": 1, "active": 0, "groups": []}"#,
                ),
            ],
        ) else {
            return;
        };
        let config = dir.join("config");
        assert!(first_launch(&config));
        let entries = import_from(&config, &[dir.join("missing.sqlite3"), store]).unwrap();
        assert!(!first_launch(&config));
        assert!(entries.contains_key("gitviber.terminals"));

        let text = std::fs::read_to_string(config.join("settings.json")).unwrap();
        let s = Settings::from_json(&text);
        assert_eq!(s.appearance, Appearance::Dark);
        assert_eq!(s.dark_theme, ThemeId::Dim);
        assert_eq!(s.light_theme, ThemeId::RosePineDawn);
        assert_eq!(s.code_font_size, 15.5);
        assert_eq!(s.terminal_font_size, 16.0);
        assert_eq!(s.ui_scale, 1.0);
        assert_eq!(s.terminal_line_height, 1.0);
        assert_eq!(s.keybindings["view.toggleExplorer"], ["cmd+e"]);
        let raw: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(raw["futureThing"], serde_json::json!([1]));

        let state = read_state(&config);
        assert_eq!(state.recent_projects, ["/p/Café 🎉", "/p/a", "/p/c"]);
        assert_eq!(state.recent_commands, ["view.pulls", "file.openRepo"]);
        assert_eq!(state.panels.left.size, Some(300.0));
        assert!(!state.panels.left.collapsed);
        assert_eq!(state.panels.right.size, Some(250.0));
        assert!(state.panels.right.collapsed);
        assert_eq!(state.panels.bottom.size, Some(210.0));
        let roots: Vec<&str> = state.workspaces.iter().map(|w| w.root.as_str()).collect();
        assert_eq!(roots, ["/p/Café 🎉", "/p/a"]);
        let ws = state.workspace("/p/Café 🎉").unwrap();
        assert_eq!(ws.list_tab.as_deref(), Some("history"));
        assert_eq!(ws.active.as_deref(), Some("unstaged::src/a.rs"));
        assert_eq!(
            ws.viewed,
            [("unstaged:src/a.rs".to_string(), "sig".to_string())]
        );
        assert_eq!(ws.review, None);
        // Each tab reads as the selection it was, keyed as the old app keyed it, and survives a save.
        assert_eq!(ws.tabs.len(), keys.len());
        for (tab, key) in ws.tabs.iter().zip(keys) {
            let sel: Selection = serde_json::from_value(tab.sel.clone()).unwrap();
            assert_eq!(selection_key(&sel), key);
            assert_eq!(tab.key, key);
            let back: Selection =
                serde_json::from_value(serde_json::to_value(&sel).unwrap()).unwrap();
            assert_eq!(selection_key(&back), key);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn translucency_from_before_the_sliders_becomes_an_opacity_and_a_blur() {
        let read = |old: Value| {
            let Value::Object(old) = old else {
                unreachable!()
            };
            let text = settings(old);
            let raw: Value = serde_json::from_str(&text).unwrap();
            assert!(raw.get("translucency").is_none());
            let s = Settings::from_json(&text);
            (s.window_opacity, s.background_blur)
        };
        for (level, want) in [
            ("off", (100.0, 0.0)),
            ("subtle", (85.0, 20.0)),
            ("strong", (70.0, 30.0)),
        ] {
            assert_eq!(
                read(serde_json::json!({ "translucency": level })),
                want,
                "{level}"
            );
        }
        // The sliders, once set, win over a leftover level.
        assert_eq!(
            read(
                serde_json::json!({"translucency": "strong", "windowOpacity": 90, "backgroundBlur": 4})
            ),
            (90.0, 4.0)
        );
        assert_eq!(read(serde_json::json!({"translucency": 7})), (100.0, 0.0));
    }

    /// Up to 0.1.10 the sizes were percentages of the window: in pixels at the window the app opens
    /// at, 0% closed; the pixel keys of later builds win.
    #[test]
    fn shares_of_the_window_from_before_0_1_11_come_as_pixels() {
        let entries: HashMap<&str, &str> = [
            (
                "react-resizable-panels:gitviber-main-v4",
                r#"{"list": 20, "viewer": 80, "files": 0}"#,
            ),
            (
                "react-resizable-panels:gitviber-viewer-v1:editor",
                "not json",
            ),
            (
                "react-resizable-panels:gitviber-viewer-v1:editor:terminal",
                r#"{"editor": 70, "terminal": 30}"#,
            ),
        ]
        .into();
        let state = |entries: &HashMap<&str, &str>| {
            ui_state(
                &|k| entries.get(k).and_then(|t| serde_json::from_str(t).ok()),
                None,
            )
        };
        let panels = state(&entries).panels;
        assert_eq!(
            panels.left,
            PanelSize {
                size: Some(296.0),
                collapsed: false
            }
        );
        assert_eq!(
            panels.right,
            PanelSize {
                size: None,
                collapsed: true
            }
        );
        assert_eq!(
            panels.bottom,
            PanelSize {
                size: Some(276.0),
                collapsed: false
            }
        );
        // A layout missing one of its panels isn't one.
        let mut partial = entries.clone();
        partial.insert("react-resizable-panels:gitviber-main-v4", r#"{"list": 20}"#);
        assert_eq!(state(&partial).panels.left, PanelSize::default());
        let mut newer = entries.clone();
        newer.insert("gitviber-sides-v1", r#"{"list": {"size": 380}}"#);
        newer.insert("gitviber-bottom-v1", r#"{"terminal": {"collapsed": true}}"#);
        let panels = state(&newer).panels;
        assert_eq!(panels.left.size, Some(380.0));
        assert_eq!(panels.right, PanelSize::default());
        assert_eq!(
            panels.bottom,
            PanelSize {
                size: None,
                collapsed: true
            }
        );
    }

    #[test]
    fn no_store_imports_nothing() {
        let dir = temp("none");
        assert!(read(&dir.join("localstorage.sqlite3")).is_none());
        assert!(import_from(&dir, &[dir.join("localstorage.sqlite3")]).is_none());
        assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
        // A store that isn't the app's (a browser tab's) isn't one either.
        if let Some(store) = fixture(&dir, &[("theme", "\"dark\"")]) {
            assert!(read(&store).is_none());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_key_leaves_the_others() {
        let dir = temp("broken");
        let Some(store) = fixture(
            &dir,
            &[
                ("gitviber.settings.v2", "{\"uiScale\": 1.2,"),
                ("gitviber.recent", r#"["/p/a"]"#),
                (
                    "gitviber.workspaces",
                    r#"{"/p/bad": {"tabs": 3, "viewed": []}, "/p/a": {"tabs": [{"key": "k", "sel": {"kind": "file", "path": "a"}}, {"sel": 7}], "viewed": [], "listTab": "changes"}}"#,
                ),
            ],
        ) else {
            return;
        };
        let config = dir.join("config");
        let entries = import_from(&config, &[store]).unwrap();
        assert!(!config.join("settings.json").exists());
        let state = read_state(&config);
        assert_eq!(state.recent_projects, ["/p/a"]);
        assert_eq!(state.workspaces.len(), 1);
        assert_eq!(state.workspaces[0].snapshot.tabs.len(), 1);
        // Kept as it is: a store's own crate may read it its way.
        assert_eq!(entries["gitviber.settings.v2"], "{\"uiScale\": 1.2,");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn values_are_there_only_while_importing() {
        *importing() = Some(Entries::from([
            ("gitviber.last".to_string(), "/p/a".to_string()),
            (
                "gitviber.drafts".to_string(),
                r#"{"/p/a": {"summary": "x"}}"#.to_string(),
            ),
        ]));
        assert_eq!(text("gitviber.last").as_deref(), Some("/p/a"));
        assert_eq!(value("gitviber.last"), None);
        assert_eq!(value("gitviber.drafts").unwrap()["/p/a"]["summary"], "x");
        finish();
        assert_eq!(value("gitviber.drafts"), None);
    }

    #[test]
    fn rows_decode_from_hex() {
        assert_eq!(
            entry("6B|blob|6100E9003DD8"),
            Some(("k".into(), "aé\u{FFFD}".into()))
        );
        assert_eq!(entry("6B|text|C3A9"), Some(("k".into(), "é".into())));
        assert_eq!(entry("6B|blob|610"), None);
        assert_eq!(
            uri(Path::new("/a b/100%?#/x")),
            "file:/a b/100%25%3f%23/x?mode=ro"
        );
    }

    #[test]
    fn a_linked_worktree_reopens_its_main() {
        let dir = temp("worktree");
        let linked = dir.join("w");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::write(linked.join(".git"), "gitdir: /p/main/.git/worktrees/w\n").unwrap();
        let list = serde_json::json!(["/p/x", "/p/main"]);
        let last = linked.to_string_lossy().into_owned();
        assert_eq!(
            recent_projects(Some(&list), Some(&last)),
            ["/p/main", "/p/x"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
