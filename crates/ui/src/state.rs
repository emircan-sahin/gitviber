//! What the app remembers rather than what the user set: the window's place, the panels' sizes,
//! recent projects, each worktree's open tabs. Kept as state.json beside settings.json; an
//! unreadable file starts fresh.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{App, Global, Task};
use serde::{Deserialize, Serialize};

use crate::store;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
    pub fullscreen: bool,
}

/// A fixed panel as the user left it: its open size in pixels, and whether they closed it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PanelSize {
    pub size: Option<f32>,
    pub collapsed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PanelSizes {
    pub left: PanelSize,
    pub right: PanelSize,
    pub bottom: PanelSize,
}

/// A tab as saved: what it shows (a `Selection`, kept as JSON so one this build can't read
/// drops alone) and whether it was the preview tab.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TabSnapshot {
    pub key: String,
    pub sel: serde_json::Value,
    pub preview: bool,
}

/// One worktree's workspace as it was left.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub tabs: Vec<TabSnapshot>,
    pub active: Option<String>,
    /// The left panel shown, by id ("changes", "history"…), and the right one.
    pub list_tab: Option<String>,
    pub right_panel: Option<String>,
    /// Viewed marks: `kind:path` → the file's content signature when it was marked.
    pub viewed: Vec<(String, String)>,
    /// The full ref the branch is reviewed against, while Changes shows that review.
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
    pub window: Option<WindowGeometry>,
    pub panels: PanelSizes,
    /// Project roots, the latest first.
    pub recent_projects: Vec<String>,
    /// Command ids, the latest first, for the palette's empty query.
    pub recent_commands: Vec<String>,
    /// By worktree root, the latest saved first.
    pub workspaces: Vec<SavedWorkspace>,
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

/// A file that can't be read, or holds what this build can't take, is moved aside
/// (`state.json.bad-<time>`) rather than overwritten.
pub(crate) fn init(cx: &mut App) {
    let path = state_path();
    let state = match store::read_object(&path) {
        Some(map) => serde_json::from_value::<UiState>(serde_json::Value::Object(map))
            .unwrap_or_else(|_| {
                store::quarantine(&path);
                store::read_object(&path);
                UiState::default()
            }),
        None => UiState::default(),
    };
    cx.set_global(state);
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
