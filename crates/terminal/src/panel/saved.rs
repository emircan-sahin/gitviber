//! The session save: every window's tabs (names, splits, where each shell was, the agent it ran)
//! in terminals.json, and each pane's history in a file of its own beside it, so saving one pane
//! doesn't rewrite them all. They're offered back in the next run's first window, its agents
//! resumed as Settings → Terminal says. A round runs SAVE_MS after output, taking the stalest
//! pane due (serializing is milliseconds a pane), and everything goes at quit.

use std::collections::{BTreeMap, HashSet};
use std::time::Instant;

use gitviber_ui::settings::ResumeMode as SettingsResume;
use gitviber_ui::store::{config_dir, write_atomic, write_in_background};
use gitviber_ui::workspace::Toast;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::attention::now_secs;
use super::*;
use crate::agent::{self, ResumeMode, SAVE_MS, SaveState, SavedAgent, due_for_save, resume_of};
use crate::history::{HISTORY_LINES, serialize};
use crate::layout::{Layout, Split};

const FILE: &str = "terminals.json";
const HISTORY_DIR: &str = "terminal-history";
const HINT_KEY: &str = "agentNotifyHint";
const TABS_KEY: &str = "tabs";

fn path() -> PathBuf {
    config_dir().join(FILE)
}

fn history_dir() -> PathBuf {
    config_dir().join(HISTORY_DIR)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct SavedSet {
    /// Unix seconds.
    pub saved_at: f64,
    pub active: usize,
    pub groups: Vec<SavedGroup>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct SavedGroup {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub focused: usize,
    /// Its leaves index `panes`; checked as it's read back (`saved_layout`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<Value>,
    pub panes: Vec<SavedPane>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct SavedPane {
    pub cwd: String,
    /// Where its shell was, when not `cwd`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Its history's file in terminal-history/.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<Value>,
}

/// terminals.json: the tabs to offer next run, and whether the hint to turn notifications on was
/// shown.
#[derive(Clone, Debug, Default, PartialEq)]
struct Store {
    /// Last run's tabs, offered once in this one.
    saved: SavedSet,
    /// Each window's tabs this run, by its panel's `save_key`: written one window after another.
    windows: BTreeMap<u64, SavedSet>,
    offered: bool,
    hint_shown: bool,
    /// Tabs were dropped as read: the file is written again, and their histories go.
    stale: bool,
}

impl gpui::Global for Store {}

impl Store {
    fn read() -> Store {
        let text = std::fs::read_to_string(path()).unwrap_or_default();
        Store::parse(&text, |dir| Path::new(dir).is_dir())
    }

    /// The tabs whose folders are still there (`exists`): a removed worktree's would come back,
    /// and resume its old conversation, should a new worktree take its path. A save from when
    /// each worktree had its own tabs comes back as one list, the most recently used last.
    fn parse(text: &str, exists: impl Fn(&str) -> bool) -> Store {
        let Ok(mut map) = serde_json::from_str::<serde_json::Map<String, Value>>(text) else {
            return Store::default();
        };
        let hint_shown = map.remove(HINT_KEY).and_then(|v| v.as_bool()) == Some(true);
        let (saved, old) = match map.remove(TABS_KEY) {
            Some(v) => (serde_json::from_value(v).unwrap_or_default(), false),
            None => {
                let sets: Vec<SavedSet> = map
                    .into_iter()
                    .filter_map(|(_, v)| serde_json::from_value(v).ok())
                    .collect();
                let n = sets.len();
                let sets = last_session(sets);
                let dropped = sets.len() < n;
                (merge(sets), dropped)
            }
        };
        let (saved, gone) = present(saved, exists);
        Store {
            saved,
            hint_shown,
            stale: old || gone,
            ..Default::default()
        }
    }

    /// What the file holds: the windows' tabs, or last run's until a window has saved.
    fn tabs(&self) -> SavedSet {
        if self.windows.is_empty() {
            self.saved.clone()
        } else {
            merge(self.windows.values().cloned().collect())
        }
    }

    fn text(&self) -> String {
        let mut map = serde_json::Map::new();
        let tabs = self.tabs();
        if !tabs.groups.is_empty() {
            map.insert(
                TABS_KEY.into(),
                serde_json::to_value(tabs).unwrap_or_default(),
            );
        }
        if self.hint_shown {
            map.insert(HINT_KEY.into(), Value::Bool(true));
        }
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default()
    }

    /// The history files the file names, and last run's all this run: any window's offer may
    /// still be taken up. They go at the next run's first write.
    fn named(&self) -> HashSet<String> {
        let mut out = history_names(&self.tabs());
        out.extend(history_names(&self.saved));
        out
    }
}

fn history_names(set: &SavedSet) -> HashSet<String> {
    set.groups
        .iter()
        .flat_map(|g| &g.panes)
        .filter_map(|p| p.history.clone())
        .collect()
}

/// The per-worktree sets saved within a day of the newest, oldest first: that save kept a set for
/// every folder ever shown, and the rest would all come back at once. One without a time stays.
fn last_session(mut sets: Vec<SavedSet>) -> Vec<SavedSet> {
    const DAY: f64 = 24. * 3600.;
    let newest = sets.iter().map(|s| s.saved_at).fold(0., f64::max);
    sets.retain(|s| s.saved_at <= 0. || s.saved_at >= newest - DAY);
    sets.sort_by(|a, b| a.saved_at.total_cmp(&b.saved_at));
    sets
}

/// `sets` one after another, open on the tab the most recently saved one had open.
fn merge(sets: Vec<SavedSet>) -> SavedSet {
    let mut out = SavedSet::default();
    for set in sets.into_iter().filter(|s| !s.groups.is_empty()) {
        if set.saved_at >= out.saved_at {
            out.saved_at = set.saved_at;
            out.active = out.groups.len() + set.active.min(set.groups.len() - 1);
        }
        out.groups.extend(set.groups);
    }
    out
}

/// `set` without the panes whose folder is gone; true when one was.
fn present(set: SavedSet, exists: impl Fn(&str) -> bool) -> (SavedSet, bool) {
    let (active, mut gone) = (set.active, false);
    let mut out = SavedSet {
        saved_at: set.saved_at,
        ..Default::default()
    };
    for (i, mut g) in set.groups.into_iter().enumerate() {
        let n = g.panes.len();
        g.panes.retain(|p| exists(&p.cwd));
        if g.panes.len() < n {
            gone = true;
            // Its splits and focus index the panes it had.
            g.layout = None;
            g.focused = 0;
        }
        if i == active {
            out.active = out.groups.len();
        }
        if !g.panes.is_empty() {
            out.groups.push(g);
        }
    }
    out.active = out.active.min(out.groups.len().saturating_sub(1));
    (out, gone)
}

fn store(cx: &mut App) -> &mut Store {
    if !cx.has_global::<Store>() {
        cx.set_global(Store::read());
    }
    cx.global_mut::<Store>()
}

pub(super) fn notify_hint_shown(cx: &mut App) -> bool {
    store(cx).hint_shown
}

pub(super) fn set_notify_hint_shown(cx: &mut App) {
    store(cx).hint_shown = true;
    let text = store(cx).text();
    write_in_background(cx, path(), text);
}

fn new_file_name() -> String {
    let hex = gitviber_core::local_socket::random_hex(8).unwrap_or_else(|| {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        format!("{n:x}")
    });
    format!("{hex}.ans")
}

/// A history file's name as saved: ours, never a path out of the folder.
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
        && !name.starts_with('.')
}

/// Splits nested deeper than any screen shows: a hand-edited save, not a layout.
const MAX_DEPTH: usize = 16;

/// `raw` as a layout of `n` panes by index, each leaf once, its sizes rescaled to 100 as a divider
/// drag and a close take them; else a row of them, as an older save or a broken one would leave it.
pub(super) fn saved_layout(raw: Option<&Value>, n: usize) -> Layout {
    fn read(v: &Value, n: usize, seen: &mut Vec<bool>, depth: usize) -> Option<Layout> {
        if let Some(i) = v.as_u64() {
            let i = i as usize;
            if i >= n || seen[i] {
                return None;
            }
            seen[i] = true;
            return Some(Layout::Pane(i as PaneId));
        }
        if depth >= MAX_DEPTH {
            return None;
        }
        let dir = match v.get("dir")?.as_str()? {
            "row" => Dir::Row,
            "col" => Dir::Col,
            _ => return None,
        };
        let children: Vec<Layout> = v
            .get("children")?
            .as_array()?
            .iter()
            .map(|c| read(c, n, seen, depth + 1))
            .collect::<Option<_>>()?;
        let sizes: Vec<f64> = v
            .get("sizes")?
            .as_array()?
            .iter()
            .map(Value::as_f64)
            .collect::<Option<_>>()?;
        let total: f64 = sizes.iter().sum();
        (children.len() >= 2
            && sizes.len() == children.len()
            && sizes.iter().all(|s| s.is_finite() && *s > 0.)
            && total.is_finite())
        .then(|| {
            Layout::Split(Split {
                dir,
                children,
                sizes: sizes.iter().map(|s| s * 100. / total).collect(),
            })
        })
    }
    let mut seen = vec![false; n];
    match raw.and_then(|v| read(v, n, &mut seen, 0)) {
        Some(l) if seen.iter().all(|s| *s) => l,
        _ => layout::row(&(0..n as PaneId).collect::<Vec<_>>()),
    }
}

fn layout_value(l: &Layout, index: &impl Fn(PaneId) -> usize) -> Value {
    match l {
        Layout::Pane(id) => Value::from(index(*id)),
        Layout::Split(s) => serde_json::json!({
            "dir": if s.dir == Dir::Row { "row" } else { "col" },
            "children": s.children.iter().map(|c| layout_value(c, index)).collect::<Vec<_>>(),
            "sizes": s.sizes,
        }),
    }
}

/// The old app's save (`gitviber.terminals`: one list of tabs, each pane's history inline): its
/// tabs, histories into files.
fn from_legacy(saved: &Value, write: &mut dyn FnMut(&str, &str)) -> SavedSet {
    let mut out = SavedSet {
        saved_at: saved
            .get("savedAt")
            .and_then(Value::as_f64)
            .map_or(0., |ms| ms / 1000.),
        ..Default::default()
    };
    let active = saved.get("active").and_then(Value::as_u64).unwrap_or(0) as usize;
    let Some(groups) = saved.get("groups").and_then(Value::as_array) else {
        return out;
    };
    for (gi, g) in groups.iter().enumerate() {
        let Some(panes) = g
            .get("panes")
            .and_then(Value::as_array)
            .filter(|p| !p.is_empty())
        else {
            continue;
        };
        let panes: Vec<SavedPane> = panes
            .iter()
            .filter_map(|p| {
                let cwd = p.get("cwd")?.as_str()?.to_string();
                let history = p
                    .get("history")
                    .and_then(Value::as_str)
                    .filter(|h| !h.is_empty())
                    .map(|h| {
                        let name = new_file_name();
                        write(&name, h);
                        name
                    });
                Some(SavedPane {
                    cwd,
                    dir: p.get("dir").and_then(Value::as_str).map(String::from),
                    name: p.get("name").and_then(Value::as_str).map(String::from),
                    history,
                    agent: p.get("agent").cloned(),
                })
            })
            .collect();
        if panes.len()
            != g.get("panes")
                .and_then(Value::as_array)
                .map_or(0, |p| p.len())
            || panes.is_empty()
        {
            continue;
        }
        if gi == active {
            out.active = out.groups.len();
        }
        out.groups.push(SavedGroup {
            name: g.get("name").and_then(Value::as_str).map(String::from),
            focused: g.get("focused").and_then(Value::as_u64).unwrap_or(0) as usize,
            layout: g.get("layout").cloned(),
            panes,
        });
    }
    out
}

/// On the first launch, the old app's terminals, unless this build has saved its own.
pub(crate) fn import_legacy(cx: &mut App) {
    if path().exists() {
        return;
    }
    let hint = gitviber_ui::legacy::value("gitviber.agentNotifyHint")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let tabs = gitviber_ui::legacy::value("gitviber.terminals").map(|v| {
        let dir = history_dir();
        from_legacy(&v, &mut |name, text| {
            let _ = std::fs::create_dir_all(&dir);
            let _ = write_atomic(&dir.join(name), text);
        })
    });
    if tabs.as_ref().is_none_or(|s| s.groups.is_empty()) && !hint {
        return;
    }
    let s = store(cx);
    s.saved = tabs.unwrap_or_default();
    s.hint_shown = hint;
    let text = s.text();
    write_in_background(cx, path(), text);
}

/// Each pane's shell's folder as its process has it, whatever runs in front of it.
fn shell_dirs(core: &Core, shells: Vec<(PaneId, u32)>) -> Vec<(PaneId, u32, PathBuf)> {
    shells
        .into_iter()
        .filter_map(|(id, pty)| Some((id, pty, core.ptys.cwd(pty)?)))
        .collect()
}

/// The offer's wording: "Restore 3 terminals from last session and resume 1 agent?", and the
/// folders they were in.
fn offer_text(saved: &SavedSet, mode: ResumeMode) -> (String, String) {
    let panes: Vec<&SavedPane> = saved.groups.iter().flat_map(|g| &g.panes).collect();
    let agents = panes
        .iter()
        .filter(|p| {
            agent::restored_agent(p.agent.as_ref()).is_some_and(|a| {
                Some(a.cwd.as_str()) == p.dir.as_deref().or(Some(p.cwd.as_str()))
                    && Path::new(&a.cwd).is_dir()
            })
        })
        .count();
    let plural = |n: usize, w: &str| {
        if n == 1 {
            format!("1 {w}")
        } else {
            format!("{n} {w}s")
        }
    };
    let resume = if agents > 0 && mode != ResumeMode::Off {
        format!(" and resume {}", plural(agents, "agent"))
    } else {
        String::new()
    };
    let mut folders: Vec<String> = vec![];
    for p in &panes {
        let name = folder_name(Path::new(&p.cwd));
        let name = if name.is_empty() { p.cwd.clone() } else { name };
        if !folders.contains(&name) {
            folders.push(name);
        }
    }
    (
        format!(
            "Restore {} from last session{resume}?",
            plural(panes.len(), "terminal")
        ),
        folders.join(", "),
    )
}

fn resume_mode(cx: &App) -> ResumeMode {
    match Settings::get(cx).resume_agents {
        SettingsResume::Type => ResumeMode::Type,
        SettingsResume::Run => ResumeMode::Run,
        SettingsResume::Off => ResumeMode::Off,
    }
}

impl TerminalPanel {
    fn save_state(&self, id: PaneId, cx: &App) -> Option<SaveState> {
        let slot = self.panes.get(&id)?;
        let pane = slot.view.read(cx);
        let ms = |t: Instant| t.saturating_duration_since(self.epoch).as_millis() as u64;
        Some(SaveState {
            dirty: pane.is_dirty(),
            wrote_at: ms(pane.last_output),
            serialized_at: slot.serialized_at,
        })
    }

    /// A round SAVE_MS from now, unless one is due already.
    pub(super) fn save_soon(&mut self, cx: &mut Context<Self>) {
        if self.round.is_some() || self.bound.is_none() {
            return;
        }
        self.round = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(SAVE_MS))
                .await;
            let Ok(asked) = this.update(cx, |this, cx| this.ask_shell_dirs(false, cx)) else {
                return;
            };
            asked.await;
            let _ = this.update(cx, |this, cx| {
                this.round = None;
                // Agents in every pane: one still printing isn't due for a while, and a short
                // first task would go unnoticed.
                this.refresh_agents(None, cx);
                this.save(false, cx);
            });
        }));
    }

    /// Where the shells with output since the last save (or `all` of them) are now, asked of their
    /// processes off the UI thread: a `cd` is saved without the integration's prompt marks too, and
    /// a program's OSC 7 (a file manager browsing) isn't taken for where its shell is.
    pub(super) fn ask_shell_dirs(&self, all: bool, cx: &mut Context<Self>) -> Task<()> {
        let shells = self.shells(all, cx);
        if shells.is_empty() {
            return Task::ready(());
        }
        let core = self.core.clone();
        cx.spawn(async move |this, cx| {
            let dirs = cx
                .background_spawn(async move { shell_dirs(&core, shells) })
                .await;
            let _ = this.update(cx, |this, cx| this.shells_are_in(dirs, cx));
        })
    }

    fn shells(&self, all: bool, cx: &App) -> Vec<(PaneId, u32)> {
        self.panes
            .iter()
            .filter(|(_, s)| all || s.view.read(cx).is_dirty())
            .filter_map(|(id, s)| Some((*id, s.view.read(cx).pty_id()?)))
            .collect()
    }

    fn shells_are_in(&mut self, dirs: Vec<(PaneId, u32, PathBuf)>, cx: &mut Context<Self>) {
        for (id, pty, dir) in dirs {
            if let Some(slot) = self.panes.get(&id) {
                slot.view.update(cx, |p, _| p.shell_is_in(pty, dir));
            }
        }
        // A header without a title names the folder.
        cx.notify();
    }

    /// Serializes the panes due (`all`: every changed one) and writes what changed.
    pub(super) fn save(&mut self, all: bool, cx: &mut Context<Self>) {
        let ids: Vec<PaneId> = self.panes.keys().copied().collect();
        let states: Vec<SaveState> = ids
            .iter()
            .filter_map(|id| self.save_state(*id, cx))
            .collect();
        let now = Instant::now()
            .saturating_duration_since(self.epoch)
            .as_millis() as u64;
        let dir = history_dir();
        for i in due_for_save(&states, now, all) {
            let id = ids[i];
            let Some(slot) = self.panes.get_mut(&id) else {
                continue;
            };
            let text = serialize(&slot.view.read(cx).session.emu.lock().term, HISTORY_LINES);
            slot.view.update(cx, |p, _| p.take_dirty());
            slot.serialized_at = now;
            // The alt screen shows: what was saved before it stands.
            let Some(text) = text else { continue };
            let name = slot.history_file.get_or_insert_with(new_file_name).clone();
            let _ = std::fs::create_dir_all(&dir);
            if all {
                let _ = write_atomic(&dir.join(&name), &text);
            } else {
                write_in_background(cx, dir.join(&name), text);
            }
            slot.history_saved = true;
        }
        self.write_tabs(all, cx);
        if !all && self.panes.values().any(|s| s.view.read(cx).is_dirty()) {
            self.save_soon(cx);
        }
    }

    fn saved_set(&self, set: &TabSet, cx: &App) -> SavedSet {
        let infos: Vec<PaneId> = set
            .groups
            .iter()
            .flat_map(|g| g.panes.iter().copied())
            .collect();
        let agents = agent::saved_agents(
            &infos
                .iter()
                .map(|id| self.panes.get(id).and_then(|s| s.agent.as_ref()))
                .collect::<Vec<_>>(),
        );
        let agent_of = |id: PaneId| {
            infos
                .iter()
                .position(|p| *p == id)
                .and_then(|i| agents[i].clone())
        };
        SavedSet {
            saved_at: now_secs(),
            active: set
                .groups
                .iter()
                .position(|g| Some(g.id) == set.active)
                .unwrap_or(0),
            groups: set
                .groups
                .iter()
                .map(|g| SavedGroup {
                    name: g.name.clone(),
                    focused: g.panes.iter().position(|p| *p == g.focused).unwrap_or(0),
                    layout: Some(layout_value(&g.layout, &|id| {
                        g.panes.iter().position(|p| *p == id).unwrap_or(0)
                    })),
                    panes: g
                        .panes
                        .iter()
                        .filter_map(|id| {
                            let slot = self.panes.get(id)?;
                            let p = slot.view.read(cx);
                            let (cwd, dir) = (
                                p.cwd().to_string_lossy().into_owned(),
                                p.dir().to_string_lossy().into_owned(),
                            );
                            Some(SavedPane {
                                dir: (dir != cwd).then_some(dir),
                                cwd,
                                name: slot.name.clone(),
                                history: slot
                                    .history_saved
                                    .then(|| slot.history_file.clone())
                                    .flatten(),
                                agent: agent_of(*id).and_then(|a| serde_json::to_value(a).ok()),
                            })
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// This window's tabs into the store: the file is written when they changed, and history
    /// files it no longer names go.
    fn write_tabs(&mut self, now: bool, cx: &mut Context<Self>) {
        // Offered and none opened since: last run's stay for the offer.
        let mine = match &self.offer {
            Some(offer) if self.tabs.groups.is_empty() => offer.clone(),
            _ => self.saved_set(&self.tabs, cx),
        };
        let same = |a: &SavedSet, b: &SavedSet| {
            SavedSet {
                saved_at: 0.,
                ..a.clone()
            } == SavedSet {
                saved_at: 0.,
                ..b.clone()
            }
        };
        let s = store(cx);
        let mut changed = std::mem::take(&mut s.stale);
        if s.windows
            .get(&self.save_key)
            .is_none_or(|old| !same(old, &mine))
        {
            s.windows.insert(self.save_key, mine);
            changed = true;
        }
        if !changed {
            return;
        }
        let text = s.text();
        let named = s.named();
        if now {
            let _ = write_atomic(&path(), &text);
        } else {
            write_in_background(cx, path(), text);
        }
        let dir = history_dir();
        let live: HashSet<String> = self
            .panes
            .values()
            .filter_map(|s| s.history_file.clone())
            .collect();
        cx.background_spawn(async move {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                return;
            };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(".ans") && !named.contains(&name) && !live.contains(&name) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        })
        .detach();
    }

    /// The app quits: agents looked up as they are now (one that exited since the last round
    /// isn't resumed; one whose shell the app already ended keeps what was known), every changed
    /// pane saved, all of it written before the process goes.
    pub(super) fn save_at_quit(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<(PaneId, u32)> = self
            .panes
            .iter()
            .filter_map(|(id, s)| Some((*id, s.view.read(cx).pty_id()?)))
            .collect();
        let alive: Vec<bool> = ids
            .iter()
            .map(|(_, p)| self.core.ptys.shell(*p).is_some())
            .collect();
        let running = gitviber_core::api::app::pty_agents(
            &self.core,
            ids.iter()
                .zip(&alive)
                .filter(|(_, a)| **a)
                .map(|((_, p), _)| *p)
                .collect(),
        )
        .unwrap_or_default();
        for ((id, pty), alive) in ids.into_iter().zip(alive) {
            if let Some(slot) = self.panes.get_mut(&id) {
                let read = running.get(&pty).map(agent::PaneAgent::from_core);
                slot.agent = agent::at_quit(slot.agent.as_ref(), alive, read);
            }
        }
        let dirs = shell_dirs(&self.core, self.shells(true, cx));
        self.shells_are_in(dirs, cx);
        self.save(true, cx);
    }

    /// Last run's terminals, offered in this run's first window: asked, not automatic, as each
    /// starts a shell. Until then (or if the toast is closed) the save keeps them, unless
    /// terminals opened since take their place.
    pub(super) fn restore(&mut self, cx: &mut Context<Self>) {
        if self.bound.is_none() {
            return;
        }
        let s = store(cx);
        if std::mem::replace(&mut s.offered, true) || s.saved.groups.is_empty() {
            return;
        }
        let saved = s.saved.clone();
        s.windows.insert(self.save_key, saved.clone());
        let (title, folders) = offer_text(&saved, resume_mode(cx));
        self.offer = Some(saved);
        let weak = cx.weak_entity();
        let toast = Toast::info(folders)
            .title(title)
            .action("Restore", move |window, cx| {
                let _ = weak.update(cx, |p, cx| p.accept_restore(window, cx));
            });
        if let Some(ws) = self.workspace() {
            ws.update(cx, |ws, cx| ws.toast(toast, cx));
        }
    }

    /// The offered terminals, after any opened since; their agents resumed where their folder is
    /// still there.
    pub fn accept_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(saved) = self.offer.take() else {
            return;
        };
        let mode = resume_mode(cx);
        let dir = history_dir();
        let when = gitviber_ui::widgets::full_date(saved.saved_at as i64);
        let mut groups = vec![];
        for g in &saved.groups {
            let mut ids = vec![];
            for p in &g.panes {
                let shell_dir = p.dir.as_ref().map(PathBuf::from).filter(|d| d.is_dir());
                let agent: Option<SavedAgent> = agent::restored_agent(p.agent.as_ref());
                let resume = agent
                    .filter(|a| {
                        Some(a.cwd.as_str()) == p.dir.as_deref().or(Some(p.cwd.as_str()))
                            && Path::new(&a.cwd).is_dir()
                    })
                    .and_then(|a| resume_of(&a, mode));
                let history = p
                    .history
                    .as_deref()
                    .filter(|n| safe_name(n))
                    .and_then(|n| std::fs::read_to_string(dir.join(n)).ok())
                    .filter(|h| !h.is_empty());
                let id = self.make_pane(
                    PathBuf::from(&p.cwd),
                    shell_dir,
                    resume.as_ref().map(|r| r.1.clone()),
                    window,
                    cx,
                );
                if let Some(slot) = self.panes.get_mut(&id) {
                    slot.name = p.name.clone();
                    // Its file holds what the pane shows: saved again only once it changes.
                    slot.history_file = history.as_ref().and(p.history.clone());
                    slot.history_saved = slot.history_file.is_some();
                    let mut text = String::new();
                    if let Some(h) = &history {
                        text.push_str(h);
                        text.push_str(&format!(
                            "\x1b[0m\r\n\x1b[2m── Restored from {when} ──\x1b[0m\r\n"
                        ));
                    }
                    if let Some((hint, _)) = &resume {
                        text.push_str(&format!("\x1b[2m{hint}\x1b[0m\r\n"));
                    }
                    if !text.is_empty() {
                        slot.view
                            .update(cx, |v, _| v.session.emu.lock().feed(text.as_bytes()));
                    }
                }
                ids.push(id);
            }
            if ids.is_empty() {
                continue;
            }
            let layout = layout::map_panes(&saved_layout(g.layout.as_ref(), ids.len()), &|i| {
                ids[i as usize]
            });
            let gid = fresh_id();
            groups.push(Group {
                id: gid,
                name: g.name.clone(),
                focused: ids.get(g.focused).copied().unwrap_or(ids[0]),
                panes: ids,
                layout,
            });
        }
        if groups.is_empty() {
            return;
        }
        let active = groups.get(saved.active).or(groups.first()).map(|g| g.id);
        self.tabs.groups.extend(groups);
        self.tabs.active = active;
        self.apply_dims(cx);
        if let Some(ws) = self.workspace() {
            ws.update(cx, |ws, cx| ws.set_open(Slot::Bottom, true, cx));
            self.focus_active(window, cx);
        }
        self.save_soon(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};
    use serde_json::json;

    /// A `cd` in a shell without the integration, which no prompt mark tells of, is saved: the
    /// shell's process is asked where it is, off the UI thread.
    #[gpui::test]
    fn a_cd_without_the_integration_is_saved(cx: &mut TestAppContext) {
        let base = std::env::temp_dir().join(format!("gitviber-term-cd-{}", std::process::id()));
        std::fs::create_dir_all(base.join("sub")).unwrap();
        let base = base.canonicalize().unwrap();
        let sub = base.join("sub");
        // SAFETY: nextest gives each test its own process; nothing else reads it meanwhile.
        unsafe { std::env::set_var("SHELL", "/bin/sh") };
        // The shell's reads wake the pane from another thread: no backtrace for each.
        cx.executor().allow_parking();
        cx.executor().forbid_parking();
        cx.update(|cx| {
            gpui_kit::init(cx);
            gitviber_ui::test_support::install_theme(cx);
            crate::init(cx);
        });
        let core = Arc::new(Core::new(Arc::new(|_| {})));
        let config = TerminalConfig {
            shell_integration: false,
            ..TerminalConfig::default()
        };
        let (panel, cx) = cx.add_window_view(|window, cx| {
            TerminalPanel::new(
                core.clone(),
                base.clone(),
                config,
                TerminalTheme::dark(),
                window,
                cx,
            )
        });
        panel.update_in(cx, |p, window, cx| {
            p.open_terminal(base.clone(), None, window, cx)
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let pane = panel.read_with(cx, |p, _| p.active_pane()).expect("a pane");
        let wait_in = |dir: &Path, cx: &mut VisualTestContext| {
            let deadline = Instant::now() + std::time::Duration::from_secs(10);
            loop {
                cx.run_until_parked();
                let now = pane
                    .read_with(cx, |p, _| p.pty_id())
                    .and_then(|id| core.ptys.cwd(id));
                if now.as_deref() == Some(dir) {
                    return;
                }
                assert!(Instant::now() < deadline, "the shell is in {now:?}");
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        };
        wait_in(&base, cx);
        pane.read_with(cx, |p, _| p.session.write("cd sub\r"));
        wait_in(&sub, cx);
        assert_eq!(pane.read_with(cx, |p, _| p.dir().to_path_buf()), base);

        panel
            .update(cx, |p, cx| p.ask_shell_dirs(true, cx))
            .detach();
        cx.run_until_parked();
        let saved = panel.read_with(cx, |p, cx| p.saved_set(&p.tabs, cx));
        assert_eq!(
            saved.groups[0].panes[0].dir.as_deref(),
            sub.to_str(),
            "{saved:?}"
        );
        panel.update(cx, |p, cx| p.close_all(cx));
        cx.update(|window, _| window.remove_window());
        cx.run_until_parked();
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A save restored: each tab's splits, sizes and focused pane come back under the new panes'
    /// ids, and the same tab is open.
    #[gpui::test]
    fn saved_tabs_come_back_split_as_they_were_under_new_ids(cx: &mut TestAppContext) {
        // Settings (how agents resume) and the save in a folder of the test's own.
        let base = gitviber_ui::test_support::config_dir("term-restore");
        // SAFETY: nextest gives each test its own process; no shell has started yet.
        unsafe { std::env::set_var("SHELL", "/bin/sh") };
        cx.executor().allow_parking();
        cx.executor().forbid_parking();
        cx.update(|cx| {
            gitviber_ui::init(cx);
            crate::init(cx);
        });
        let core = Arc::new(Core::new(Arc::new(|_| {})));
        let (panel, cx) = cx.add_window_view(|window, cx| {
            TerminalPanel::new(
                core.clone(),
                base.clone(),
                TerminalConfig::default(),
                TerminalTheme::dark(),
                window,
                cx,
            )
        });
        panel.update_in(cx, |p, window, cx| {
            p.open_terminal(base.clone(), None, window, cx);
            p.split(Dir::Row, None, window, cx);
            p.split(Dir::Col, None, window, cx);
            p.open_terminal(base.clone(), None, window, cx);
            p.split(Dir::Col, None, window, cx);
            // A divider dragged: the first tab's sizes aren't even.
            let set = &mut p.tabs;
            let g = &mut set.groups[0];
            g.layout = layout::resize(&g.layout, &[], &[30., 70.]);
            g.focused = g.panes[0];
            set.active = Some(g.id);
        });
        type Tab = (Vec<PaneId>, Layout, PaneId);
        let tabs = |cx: &mut VisualTestContext| -> (Vec<Tab>, Option<usize>) {
            panel.read_with(cx, |p, _| {
                let s = &p.tabs;
                let tabs = s
                    .groups
                    .iter()
                    .map(|g| (g.panes.clone(), g.layout.clone(), g.focused))
                    .collect();
                (tabs, s.groups.iter().position(|g| Some(g.id) == s.active))
            })
        };
        let (before, open) = tabs(cx);
        assert!(matches!(&before[0].1, Layout::Split(s) if s.sizes == [30., 70.]));
        panel.update_in(cx, |p, window, cx| {
            let saved = p.saved_set(&p.tabs, cx);
            p.close_all(cx);
            p.offer = Some(saved);
            p.accept_restore(window, cx);
        });
        let (after, reopened) = tabs(cx);
        assert_eq!((after.len(), reopened), (before.len(), open));
        for ((old, layout, focused), (new, restored, refocused)) in before.iter().zip(&after) {
            assert!(new.iter().all(|id| !old.contains(id)), "new ids");
            let to_new = |id: PaneId| new[old.iter().position(|p| *p == id).unwrap()];
            assert_eq!(
                rounded(restored),
                rounded(&layout::map_panes(layout, &to_new))
            );
            assert_eq!(*refocused, to_new(*focused));
        }
        panel.update(cx, |p, cx| p.close_all(cx));
        cx.update(|window, _| window.remove_window());
        cx.run_until_parked();
    }

    #[test]
    fn the_old_apps_tabs_come_back_in_their_order_with_their_histories_in_files() {
        let old = serde_json::json!({
            "savedAt": 1_700_000_000_000u64,
            "active": 1,
            "groups": [
                {"focused": 0, "panes": [{"cwd": "/w/a", "history": "\u{1b}[31mred\u{1b}[0m\r\nmore"}]},
                {"name": "api", "focused": 1, "layout": {"dir": "row", "children": [0, 1], "sizes": [0.5, 0.5]},
                 "panes": [{"cwd": "/w/b", "history": ""}, {"cwd": "/w/b", "dir": "/w/b/src", "name": "srv", "history": "x",
                            "agent": {"name": "Claude Code", "command": "claude --resume s-1", "cwd": "/w/b/src"}}]},
                {"focused": 0, "panes": []},
                {"focused": 0, "panes": [{"cwd": "/w/a", "history": ""}]}
            ]
        });
        let mut files = vec![];
        let tabs = from_legacy(&old, &mut |name, text| {
            files.push((name.to_string(), text.to_string()))
        });
        let cwds: Vec<&str> = tabs
            .groups
            .iter()
            .map(|g| g.panes[0].cwd.as_str())
            .collect();
        assert_eq!(cwds, ["/w/a", "/w/b", "/w/a"]);
        assert_eq!(tabs.saved_at, 1_700_000_000.);
        let (a, b) = (&tabs.groups[0], &tabs.groups[1]);
        assert_eq!(
            (tabs.active, b.name.as_deref(), b.focused),
            (1, Some("api"), 1)
        );
        let srv = &b.panes[1];
        assert_eq!(
            (srv.dir.as_deref(), srv.name.as_deref()),
            (Some("/w/b/src"), Some("srv"))
        );
        assert_eq!(
            agent::restored_agent(srv.agent.as_ref()).map(|a| a.command),
            Some("claude --resume s-1".into())
        );
        // Empty histories make no file; the others are written as they were.
        assert_eq!(files.len(), 2);
        assert_eq!(a.panes[0].history.as_deref(), Some(files[0].0.as_str()));
        assert!(files[0].1.starts_with("\u{1b}[31mred"));
        assert!(b.panes[0].history.is_none());
        assert_eq!(
            saved_layout(b.layout.as_ref(), 2),
            Layout::Split(Split {
                dir: Dir::Row,
                children: vec![Layout::Pane(0), Layout::Pane(1)],
                sizes: vec![50., 50.]
            })
        );
    }

    #[test]
    fn a_broken_layout_is_a_row_and_a_history_name_stays_in_its_folder() {
        let row = layout::row(&[0, 1, 2]);
        assert_eq!(saved_layout(None, 3), row);
        assert_eq!(
            saved_layout(
                Some(&serde_json::json!({"dir": "row", "children": [0, 0, 1], "sizes": [1, 1, 1]})),
                3
            ),
            row
        );
        assert_eq!(saved_layout(Some(&serde_json::json!(7)), 3), row);
        assert_eq!(
            saved_layout(Some(&serde_json::json!(0)), 1),
            Layout::Pane(0)
        );
        let split = |children: Value, sizes: Value| serde_json::json!({"dir": "row", "children": children, "sizes": sizes});
        let two = layout::row(&[0, 1]);
        for (bad, why) in [
            (split(json!([0, 1]), json!([50, -50])), "a negative size"),
            (split(json!([0, 1]), json!([50, 0])), "a zero size"),
            (
                split(json!([0, 1]), json!([1e308, 1e308])),
                "sizes past f64",
            ),
            (split(json!([0, 1]), json!([50, "50"])), "a string size"),
            (split(json!([0, 1]), json!([50])), "a size missing"),
            (split(json!([0]), json!([100])), "a split of one"),
            (split(json!([0, null]), json!([50, 50])), "a null child"),
            (split(json!([0, "1"]), json!([50, 50])), "a string child"),
            (
                split(json!([0, {"children": [1]}]), json!([50, 50])),
                "a child without a direction",
            ),
            (
                json!({"dir": "diagonal", "children": [0, 1], "sizes": [50, 50]}),
                "no such direction",
            ),
        ] {
            assert_eq!(saved_layout(Some(&bad), 2), two, "{why}");
        }
        assert_eq!(
            saved_layout(Some(&split(json!([0, 1]), json!([50, 50]))), 3),
            row,
            "a pane missing from it"
        );
        assert!(safe_name("0a1b.ans"));
        assert!(!safe_name("../x.ans") && !safe_name("/etc/passwd") && !safe_name(".ans"));
    }

    /// A divider drag and a close take a split's sizes as percents: a save's are made so.
    #[test]
    fn saved_sizes_are_rescaled_to_100_and_splits_nest_16_deep() {
        let saved = json!({"dir": "col", "children": [0, 1], "sizes": [1, 3]});
        assert_eq!(
            saved_layout(Some(&saved), 2),
            Layout::Split(Split {
                dir: Dir::Col,
                children: vec![Layout::Pane(0), Layout::Pane(1)],
                sizes: vec![25., 75.]
            })
        );
        // Each split's first child a pane, its second the next split: `depth` splits, one in another.
        let nested = |depth: usize| {
            let mut v = json!(depth);
            for i in (0..depth).rev() {
                v = json!({"dir": if i % 2 == 0 { "row" } else { "col" }, "children": [i, v], "sizes": [50, 50]});
            }
            v
        };
        assert_ne!(
            saved_layout(Some(&nested(MAX_DEPTH)), MAX_DEPTH + 1),
            layout::row(&(0..=MAX_DEPTH as PaneId).collect::<Vec<_>>())
        );
        let deeper = MAX_DEPTH + 1;
        assert_eq!(
            saved_layout(Some(&nested(deeper)), deeper + 1),
            layout::row(&(0..=deeper as PaneId).collect::<Vec<_>>()),
            "nested past any screen"
        );
    }

    /// Sizes to a millionth: a save's rescale may differ in the last bits.
    fn rounded(l: &Layout) -> Layout {
        match l {
            Layout::Pane(id) => Layout::Pane(*id),
            Layout::Split(s) => Layout::Split(Split {
                dir: s.dir,
                children: s.children.iter().map(rounded).collect(),
                sizes: s.sizes.iter().map(|x| (x * 1e6).round() / 1e6).collect(),
            }),
        }
    }

    /// `l` saved with its panes by their place in `order` (a tab's panes), through the file's
    /// text, and restored under the ids `new` gives each place.
    fn round_trip(l: &Layout, order: &[PaneId], new: impl Fn(usize) -> PaneId) -> Layout {
        let value = layout_value(l, &|id| order.iter().position(|p| *p == id).unwrap());
        let text = serde_json::to_string(&value).unwrap();
        let back: Value = serde_json::from_str(&text).unwrap();
        layout::map_panes(
            &saved_layout(Some(&back), order.len()),
            &|i| new(i as usize),
        )
    }

    #[test]
    fn a_save_restores_the_same_tree_under_the_new_panes_ids() {
        let split = |dir, children, sizes| {
            Layout::Split(Split {
                dir,
                children,
                sizes,
            })
        };
        let l = split(
            Dir::Col,
            vec![
                split(
                    Dir::Row,
                    vec![
                        Layout::Pane(7),
                        split(
                            Dir::Col,
                            vec![Layout::Pane(9), Layout::Pane(8)],
                            vec![40., 60.],
                        ),
                    ],
                    vec![30., 70.],
                ),
                Layout::Pane(12),
            ],
            vec![65., 35.],
        );
        let order = layout::leaves(&l);
        let ids = [101, 102, 103, 104];
        let want = layout::map_panes(&l, &|id| ids[order.iter().position(|p| *p == id).unwrap()]);
        assert_eq!(rounded(&round_trip(&l, &order, |i| ids[i])), rounded(&want));
        // Whatever a run of splits and closes leaves, its panes in the order they opened.
        let mut seed: u32 = 7;
        let mut random = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed >> 8
        };
        let (mut l, mut live, mut next) = (Layout::Pane(1), vec![1], 2);
        for _ in 0..300 {
            let pick = live[random() as usize % live.len()];
            if live.len() > 1 && random() % 5 < 2 {
                l = layout::remove_pane(&l, pick).unwrap();
                live.retain(|&id| id != pick);
            } else {
                let dir = if random() % 2 == 0 {
                    Dir::Row
                } else {
                    Dir::Col
                };
                l = layout::split_pane(&l, pick, next, dir);
                live.push(next);
                next += 1;
            }
            let new = |i: usize| 1000 + i as PaneId;
            let want =
                layout::map_panes(&l, &|id| new(live.iter().position(|p| *p == id).unwrap()));
            assert_eq!(rounded(&round_trip(&l, &live, new)), rounded(&want));
        }
    }

    #[test]
    fn the_offer_says_how_many_and_where() {
        let dir = std::env::temp_dir().to_string_lossy().into_owned();
        let agent =
            serde_json::json!({"name": "Claude Code", "command": "claude --resume a", "cwd": dir});
        let pane = |cwd: &str, agent: Option<Value>| SavedPane {
            cwd: cwd.into(),
            agent,
            ..Default::default()
        };
        let set = SavedSet {
            groups: vec![
                SavedGroup {
                    panes: vec![pane(&dir, Some(agent)), pane(&dir, None)],
                    ..Default::default()
                },
                SavedGroup {
                    panes: vec![pane("/w/api", None)],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let here = folder_name(Path::new(&dir));
        assert_eq!(
            offer_text(&set, ResumeMode::Type),
            (
                "Restore 3 terminals from last session and resume 1 agent?".into(),
                format!("{here}, api")
            )
        );
        assert_eq!(
            offer_text(&set, ResumeMode::Off).0,
            "Restore 3 terminals from last session?"
        );
    }

    #[test]
    fn the_tabs_read_back_as_written() {
        let set = SavedSet {
            saved_at: 5.,
            active: 0,
            groups: vec![SavedGroup {
                name: None,
                focused: 0,
                layout: Some(Value::from(0)),
                panes: vec![SavedPane {
                    cwd: "/w".into(),
                    history: Some("ab.ans".into()),
                    ..Default::default()
                }],
            }],
        };
        let mut store = Store {
            hint_shown: true,
            ..Default::default()
        };
        store.windows.insert(1, set.clone());
        let read = Store::parse(&store.text(), |_| true);
        assert_eq!(
            (read.saved, read.hint_shown, read.stale),
            (set, true, false)
        );
        assert_eq!(store.named(), HashSet::from(["ab.ans".to_string()]));
    }

    fn tab(cwds: &[&str], name: &str) -> SavedGroup {
        SavedGroup {
            name: Some(name.into()),
            focused: cwds.len() - 1,
            layout: Some(
                json!({"dir": "row", "children": (0..cwds.len()).collect::<Vec<_>>(), "sizes": vec![1; cwds.len()]}),
            ),
            panes: cwds
                .iter()
                .map(|c| SavedPane {
                    cwd: c.to_string(),
                    ..Default::default()
                })
                .collect(),
        }
    }

    fn names(set: &SavedSet) -> Vec<&str> {
        set.groups
            .iter()
            .filter_map(|g| g.name.as_deref())
            .collect()
    }

    /// A save from when each worktree had its own tabs: every worktree's come back as one list,
    /// the most recently used last and its open tab open; a removed folder's panes are left out.
    #[test]
    fn a_save_of_tabs_per_worktree_reads_back_as_one_list() {
        let set = |at: f64, active: usize, groups: Vec<SavedGroup>| SavedSet {
            saved_at: at,
            active,
            groups,
        };
        let old = json!({
            "/w/app": set(30., 1, vec![tab(&["/w/app"], "a1"), tab(&["/w/app"], "a2")]),
            "/w/app/.wt/fix": set(10., 0, vec![tab(&["/w/app/.wt/fix", "/gone"], "f1")]),
            "/w/api": set(20., 0, vec![tab(&["/gone"], "gone"), tab(&["/w/api"], "p1")]),
            "agentNotifyHint": true,
        });
        let read = Store::parse(&old.to_string(), |dir| dir != "/gone");
        assert_eq!(names(&read.saved), ["f1", "p1", "a1", "a2"]);
        assert_eq!(read.saved.active, 3, "the newest set's open tab");
        assert_eq!(read.saved.saved_at, 30.);
        assert!(read.hint_shown && read.stale);
        // A split that lost a pane is a row of the rest.
        let f1 = &read.saved.groups[0];
        assert_eq!((f1.panes.len(), f1.focused, &f1.layout), (1, 0, &None));
        // Written back as one list.
        let back: Value = serde_json::from_str(&read.text()).unwrap();
        assert_eq!(back.as_object().unwrap().len(), 2, "{back}");
        let tabs: SavedSet = serde_json::from_value(back[TABS_KEY].clone()).unwrap();
        assert_eq!(tabs, read.saved);
    }

    /// Of a save of tabs per worktree, only the sets saved within a day of the newest come back:
    /// it kept every folder ever shown. Sets without a time all stay.
    #[test]
    fn a_save_of_tabs_per_worktree_offers_only_its_last_session() {
        let set = |at: f64, name: &str| SavedSet {
            saved_at: at,
            active: 0,
            groups: vec![tab(&["/w/a"], name)],
        };
        let t = 1_700_000_000.;
        let old = json!({
            "/w/a": set(t, "now"),
            "/w/b": set(t - 3600., "hour ago"),
            "/w/c": set(t - 30. * 86400., "month ago"),
        });
        let read = Store::parse(&old.to_string(), |_| true);
        assert_eq!(names(&read.saved), ["hour ago", "now"]);
        assert!(read.stale, "written back without the old set");

        let untimed = json!({ "/w/a": set(0., "a"), "/w/b": set(0., "b") });
        let read = Store::parse(&untimed.to_string(), |_| true);
        assert_eq!(names(&read.saved).len(), 2);
        assert!(!read.stale);
    }

    /// Last run's histories stay named all this run, after every window saved its own tabs:
    /// another window's offer may still be taken up.
    #[test]
    fn last_runs_histories_stay_named_through_this_run() {
        let mut old = tab(&["/w/a"], "a");
        old.panes[0].history = Some("old.ans".into());
        let mut store = Store {
            saved: SavedSet {
                groups: vec![old],
                ..Default::default()
            },
            ..Default::default()
        };
        store.windows.insert(
            1,
            SavedSet {
                groups: vec![tab(&["/w/b"], "b")],
                ..Default::default()
            },
        );
        assert!(store.named().contains("old.ans"));
    }

    /// Two windows' tabs go one after the other; a window's offer stands in for it until it has
    /// tabs of its own.
    #[test]
    fn every_windows_tabs_are_saved_together() {
        let mut store = Store {
            saved: SavedSet {
                groups: vec![tab(&["/w/a"], "last run")],
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            names(&store.tabs()),
            ["last run"],
            "nothing saved this run yet"
        );
        let one = SavedSet {
            saved_at: 2.,
            active: 1,
            groups: vec![tab(&["/w/a"], "x"), tab(&["/w/b"], "y")],
        };
        store.windows.insert(1, store.saved.clone());
        store.windows.insert(2, one);
        let tabs = store.tabs();
        assert_eq!((names(&tabs), tabs.active), (vec!["last run", "x", "y"], 2));
        store.windows.insert(2, SavedSet::default());
        assert_eq!(names(&store.tabs()), ["last run"]);
    }
}
