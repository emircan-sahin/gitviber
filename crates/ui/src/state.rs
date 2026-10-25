//! What the app remembers rather than what the user set: the window's place, the panels' sizes,
//! recent projects, each worktree's open tabs. Kept as state.json beside settings.json; an
//! unreadable file starts fresh, and a field that doesn't read drops alone.

use std::cell::Cell;
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{App, Global, Task};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::store;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowGeometry {
    #[serde(deserialize_with = "lenient")]
    pub x: f32,
    #[serde(deserialize_with = "lenient")]
    pub y: f32,
    #[serde(deserialize_with = "lenient")]
    pub width: f32,
    #[serde(deserialize_with = "lenient")]
    pub height: f32,
    #[serde(deserialize_with = "lenient")]
    pub maximized: bool,
    #[serde(deserialize_with = "lenient")]
    pub fullscreen: bool,
}

/// A fixed panel as the user left it: its open size in pixels, and whether they closed it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PanelSize {
    #[serde(deserialize_with = "lenient")]
    pub size: Option<f32>,
    #[serde(deserialize_with = "lenient")]
    pub collapsed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PanelSizes {
    #[serde(deserialize_with = "lenient")]
    pub left: PanelSize,
    #[serde(deserialize_with = "lenient")]
    pub right: PanelSize,
    #[serde(deserialize_with = "lenient")]
    pub bottom: PanelSize,
}

/// A tab as saved: what it shows (a `Selection`, kept as JSON so one this build can't read
/// drops alone) and whether it was the preview tab.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TabSnapshot {
    #[serde(deserialize_with = "lenient")]
    pub key: String,
    pub sel: serde_json::Value,
    #[serde(deserialize_with = "lenient")]
    pub preview: bool,
}

/// One worktree's workspace as it was left.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    #[serde(deserialize_with = "lenient_list")]
    pub tabs: Vec<TabSnapshot>,
    #[serde(deserialize_with = "lenient")]
    pub active: Option<String>,
    /// The left panel shown, by id ("changes", "history"…), and the right one.
    #[serde(deserialize_with = "lenient")]
    pub list_tab: Option<String>,
    #[serde(deserialize_with = "lenient")]
    pub right_panel: Option<String>,
    /// Viewed marks: `kind:path` → the file's content signature when it was marked.
    #[serde(deserialize_with = "lenient_list")]
    pub viewed: Vec<(String, String)>,
    /// The full ref the branch is reviewed against, while Changes shows that review.
    #[serde(deserialize_with = "lenient")]
    pub review: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SavedWorkspace {
    pub root: String,
    #[serde(flatten)]
    pub snapshot: WorkspaceSnapshot,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    #[serde(deserialize_with = "lenient")]
    pub window: Option<WindowGeometry>,
    #[serde(deserialize_with = "lenient")]
    pub panels: PanelSizes,
    /// Project roots, the latest first.
    #[serde(deserialize_with = "lenient_list")]
    pub recent_projects: Vec<String>,
    /// Command ids, the latest first, for the palette's empty query.
    #[serde(deserialize_with = "lenient_list")]
    pub recent_commands: Vec<String>,
    /// By worktree root, the latest saved first.
    #[serde(deserialize_with = "lenient_list")]
    pub workspaces: Vec<SavedWorkspace>,
    #[serde(deserialize_with = "lenient")]
    pub settings_section: Option<String>,
}

const RECENT_PROJECTS: usize = 20;
pub(crate) const RECENT_COMMANDS: usize = 30;
/// Agent worktrees come and go; keep only the most recently used.
pub(crate) const SAVED_WORKSPACES: usize = 30;

/// `item` first in `list`, once, at most `cap` long.
pub fn bump(list: &mut Vec<String>, item: &str, cap: usize) {
    list.retain(|i| i != item);
    list.insert(0, item.to_string());
    list.truncate(cap);
}

impl UiState {
    pub fn add_recent_project(&mut self, root: &str) {
        bump(&mut self.recent_projects, root, RECENT_PROJECTS);
    }

    pub fn add_recent_command(&mut self, id: &str) {
        bump(&mut self.recent_commands, id, RECENT_COMMANDS);
    }

    pub fn workspace(&self, root: &str) -> Option<&WorkspaceSnapshot> {
        self.workspaces
            .iter()
            .find(|w| w.root == root)
            .map(|w| &w.snapshot)
    }

    /// Saves `root`'s workspace first, keeping the latest SAVED_WORKSPACES roots.
    pub fn save_workspace(&mut self, root: &str, snapshot: WorkspaceSnapshot) {
        self.workspaces.retain(|w| w.root != root);
        self.workspaces.insert(
            0,
            SavedWorkspace {
                root: root.to_string(),
                snapshot,
            },
        );
        self.workspaces.truncate(SAVED_WORKSPACES);
    }
}

#[derive(Default)]
struct SaveLater(Option<Task<()>>);

impl Global for SaveLater {}
impl Global for UiState {}

impl UiState {
    pub fn get(cx: &App) -> &UiState {
        cx.global::<UiState>()
    }

    /// Changes the state and stores it now.
    pub fn update(cx: &mut App, f: impl FnOnce(&mut UiState)) {
        f(cx.global_mut::<UiState>());
        save(cx);
    }

    /// Changes the state and stores it a moment later, once: for what changes many times a
    /// second (a window being dragged, a panel being resized).
    pub fn update_later(cx: &mut App, f: impl FnOnce(&mut UiState)) {
        f(cx.global_mut::<UiState>());
        if cx.default_global::<SaveLater>().0.is_some() {
            return;
        }
        let timer = cx.background_executor().timer(Duration::from_millis(500));
        let task = cx.spawn(async move |cx| {
            timer.await;
            cx.update(|cx| {
                cx.global_mut::<SaveLater>().0 = None;
                save(cx);
            });
        });
        cx.global_mut::<SaveLater>().0 = Some(task);
    }
}

pub fn state_path() -> PathBuf {
    store::config_dir().join("state.json")
}

/// Writes the keys that changed into the file as it is now: another process's other keys stay.
fn save(cx: &mut App) {
    if let Ok(serde_json::Value::Object(map)) = serde_json::to_value(UiState::get(cx)) {
        store::save_object(cx, state_path(), map);
    }
}

/// Stores what `update_later` hasn't yet, on this thread: for quitting.
pub fn flush(cx: &mut App) {
    if cx.default_global::<SaveLater>().0.take().is_some() {
        save(cx);
    }
    store::flush(cx);
}

thread_local! {
    /// Set while state.json is read when a field held what this build can't take.
    static DROPPED: Cell<bool> = const { Cell::new(false) };
}

/// A field that doesn't read is its default; the rest of the file still counts.
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(serde_json::from_value(value).unwrap_or_else(|_| {
        DROPPED.set(true);
        T::default()
    }))
}

/// A list keeps the items that read.
fn lenient_list<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
    let Value::Array(items) = Value::deserialize(d)? else {
        DROPPED.set(true);
        return Ok(Vec::new());
    };
    Ok(items
        .into_iter()
        .filter_map(|item| {
            serde_json::from_value(item)
                .inspect_err(|_| DROPPED.set(true))
                .ok()
        })
        .collect())
}

/// The state in `map`, and whether some of it didn't read.
fn read(map: Map<String, Value>) -> Option<(UiState, bool)> {
    DROPPED.set(false);
    let state = serde_json::from_value(Value::Object(map)).ok()?;
    Some((state, DROPPED.take()))
}

/// A file that can't be read is moved aside (`state.json.bad-<time>`) rather than overwritten.
/// One with a field this build can't take keeps the rest: the file as it was is copied aside,
/// then saved without that field, so the next launch reads it whole.
pub(crate) fn init(cx: &mut App) {
    let path = state_path();
    let (state, dropped) = match store::read_object(&path).map(read) {
        Some(Some(read)) => read,
        Some(None) => {
            store::quarantine(&path);
            store::read_object(&path);
            (UiState::default(), false)
        }
        None => (UiState::default(), false),
    };
    cx.set_global(state);
    if dropped {
        store::keep_copy(&path);
        save(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_moves_to_front_and_caps() {
        let mut list = vec!["a".to_string(), "b".into(), "c".into()];
        bump(&mut list, "c", 3);
        assert_eq!(list, ["c", "a", "b"]);
        bump(&mut list, "d", 3);
        assert_eq!(list, ["d", "c", "a"]);
    }

    #[test]
    fn saved_workspaces_keep_the_latest_thirty() {
        let mut s = UiState::default();
        for i in 0..40 {
            s.save_workspace(
                &format!("/w/{i}"),
                WorkspaceSnapshot {
                    active: Some(format!("t{i}")),
                    ..Default::default()
                },
            );
        }
        s.save_workspace("/w/35", WorkspaceSnapshot::default());
        assert_eq!(s.workspaces.len(), 30);
        assert_eq!(s.workspaces[0].root, "/w/35");
        assert!(s.workspace("/w/5").is_none());
        assert_eq!(s.workspace("/w/39").unwrap().active.as_deref(), Some("t39"));
        let back: UiState = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn a_wrong_typed_field_drops_alone() {
        let map = serde_json::json!({
            "window": {"x": 10, "y": 20, "width": "wide", "height": 700},
            "panels": {"left": {"size": "x", "collapsed": true}, "right": {"size": 280}},
            "recentProjects": ["/a", 5, "/b"],
            "recentCommands": "palette",
            "workspaces": [
                {"root": 1},
                {"root": "/w", "active": "k", "tabs": [{"key": "k", "preview": "yes"}]},
            ],
            "settingsSection": 5,
        });
        let Value::Object(map) = map else {
            unreachable!()
        };
        let (s, dropped) = read(map).unwrap();
        assert!(dropped);
        let window = s.window.clone().unwrap();
        assert_eq!(
            (window.x, window.y, window.width, window.height),
            (10., 20., 0., 700.)
        );
        assert_eq!(
            s.panels.left,
            PanelSize {
                size: None,
                collapsed: true
            }
        );
        assert_eq!(s.panels.right.size, Some(280.));
        assert_eq!(s.recent_projects, ["/a", "/b"]);
        assert!(s.recent_commands.is_empty());
        assert_eq!(s.workspaces.len(), 1);
        let w = s.workspace("/w").unwrap();
        assert_eq!(w.active.as_deref(), Some("k"));
        assert_eq!(w.tabs[0].key, "k");
        assert!(!w.tabs[0].preview);
        assert!(s.settings_section.is_none());
        // A whole file reads without a drop.
        let Value::Object(map) = serde_json::to_value(&s).unwrap() else {
            unreachable!()
        };
        assert_eq!(read(map), Some((s, false)));
    }

    /// The file as it was is copied aside once, and saved again without the field that didn't
    /// read: the next launch finds it whole and copies nothing.
    #[gpui_kit::test]
    fn a_partly_read_file_is_kept_once_and_saved(cx: &mut gpui_kit::TestAppContext) {
        let dir = crate::test_support::config_dir("state-partial");
        let original =
            r#"{"recentProjects": ["/a"], "panels": {"left": {"size": "x"}}, "theirs": 1}"#;
        std::fs::write(dir.join("state.json"), original).unwrap();
        let copies = || {
            std::fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("state.json.bad-")
                })
                .map(|e| e.path())
                .collect::<Vec<_>>()
        };
        cx.update(init);
        cx.run_until_parked();
        cx.update(flush);
        assert_eq!(
            cx.update(|cx| UiState::get(cx).recent_projects.clone()),
            ["/a"]
        );
        let kept = copies();
        assert_eq!(kept.len(), 1);
        assert_eq!(std::fs::read_to_string(&kept[0]).unwrap(), original);
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("state.json")).unwrap())
                .unwrap();
        assert_eq!(saved["panels"]["left"]["size"], Value::Null);
        assert_eq!(saved["recentProjects"], serde_json::json!(["/a"]));
        assert_eq!(saved["theirs"], 1);
        cx.update(init);
        cx.run_until_parked();
        assert_eq!(copies().len(), 1);
    }

    #[test]
    fn a_broken_or_partial_file_reads() {
        assert!(serde_json::from_str::<UiState>("{oops").is_err());
        let s: UiState = serde_json::from_str(
            r#"{"recentProjects": ["/x"], "panels": {"left": {"size": 300}}}"#,
        )
        .unwrap();
        assert_eq!(s.recent_projects, ["/x"]);
        assert_eq!(s.panels.left.size, Some(300.0));
        assert!(!s.panels.left.collapsed);
    }
}
