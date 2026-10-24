//! The session save: each worktree's tabs (names, splits, where each shell was, the agent it ran)
//! in terminals.json, and each pane's history in a file of its own beside it, so saving one pane
//! doesn't rewrite them all. A worktree's are restored the first time it shows in a run, its
//! agents resumed as Settings → Terminal says. A round runs SAVE_MS after output, taking the
//! stalest pane due (serializing is milliseconds a pane), and everything goes at quit.

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

/// terminals.json: each worktree's set by its path, and whether the hint to turn notifications on
/// was shown.
#[derive(Clone, Debug, Default, PartialEq)]
struct Store {
    sets: BTreeMap<String, SavedSet>,
    hint_shown: bool,
    /// Sets were dropped as read: the file is written again, and their histories go.
    stale: bool,
}

impl gpui::Global for Store {}

impl Store {
    fn read() -> Store {
        let text = std::fs::read_to_string(path()).unwrap_or_default();
        Store::parse(&text, |root| Path::new(root).is_dir())
    }

    /// The sets of worktrees still there (`exists`): a removed one's would come back, and resume
    /// its old conversation, should a new worktree take its path.
    fn parse(text: &str, exists: impl Fn(&str) -> bool) -> Store {
        let Ok(map) = serde_json::from_str::<serde_json::Map<String, Value>>(text) else {
            return Store::default();
        };
        let mut store = Store::default();
        for (k, v) in map {
            if k == HINT_KEY {
                store.hint_shown = v.as_bool().unwrap_or(false);
            } else if !exists(&k) {
                store.stale = true;
            } else if let Ok(set) = serde_json::from_value::<SavedSet>(v) {
                store.sets.insert(k, set);
            }
        }
        store
    }

    fn text(&self) -> String {
        let mut map = serde_json::Map::new();
        for (k, set) in &self.sets {
            map.insert(k.clone(), serde_json::to_value(set).unwrap_or_default());
        }
        if self.hint_shown {
            map.insert(HINT_KEY.into(), Value::Bool(true));
        }
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default()
    }

    /// The history files some set still names.
    fn named(&self) -> HashSet<String> {
        self.sets
            .values()
            .flat_map(|s| &s.groups)
            .flat_map(|g| &g.panes)
            .filter_map(|p| p.history.clone())
            .collect()
    }
}

fn store(cx: &mut App) -> &mut Store {
    if !cx.has_global::<Store>() {
        cx.set_global(Store::read());
    }
    cx.global_mut::<Store>()
}

/// Worktrees whose saved terminals came back this run, in any window: once each.
#[derive(Default)]
struct Restored(HashSet<PathBuf>);

impl gpui::Global for Restored {}

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

/// `raw` as a layout of `n` panes by index, each leaf once; else a row of them, as an older save
/// or a broken one would leave it.
pub(super) fn saved_layout(raw: Option<&Value>, n: usize) -> Layout {
    fn read(v: &Value, n: usize, seen: &mut Vec<bool>) -> Option<Layout> {
        if let Some(i) = v.as_u64() {
            let i = i as usize;
            if i >= n || seen[i] {
                return None;
            }
            seen[i] = true;
            return Some(Layout::Pane(i as PaneId));
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
            .map(|c| read(c, n, seen))
            .collect::<Option<_>>()?;
        let sizes: Vec<f64> = v
            .get("sizes")?
            .as_array()?
            .iter()
            .map(Value::as_f64)
            .collect::<Option<_>>()?;
        (children.len() >= 2
            && sizes.len() == children.len()
            && sizes.iter().all(|s| s.is_finite() && *s > 0.))
        .then_some(Layout::Split(Split {
            dir,
            children,
            sizes,
        }))
    }
    let mut seen = vec![false; n];
    match raw.and_then(|v| read(v, n, &mut seen)) {
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

/// The old app's save (`gitviber.terminals`: one list of tabs for every worktree, each pane's
/// history inline): its tabs by the folder their first pane opened in, histories into files.
fn from_legacy(saved: &Value, write: &mut dyn FnMut(&str, &str)) -> BTreeMap<String, SavedSet> {
    let mut out: BTreeMap<String, SavedSet> = BTreeMap::new();
    let saved_at = saved
        .get("savedAt")
        .and_then(Value::as_f64)
        .map_or(0., |ms| ms / 1000.);
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
        let key = panes[0].cwd.clone();
        let set = out.entry(key).or_insert_with(|| SavedSet {
            saved_at,
            ..Default::default()
        });
        if gi == active {
            set.active = set.groups.len();
        }
        set.groups.push(SavedGroup {
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
    let sets = gitviber_ui::legacy::value("gitviber.terminals").map(|v| {
        let dir = history_dir();
        from_legacy(&v, &mut |name, text| {
            let _ = std::fs::create_dir_all(&dir);
            let _ = write_atomic(&dir.join(name), text);
        })
    });
    if sets.as_ref().is_none_or(|s| s.is_empty()) && !hint {
        return;
    }
    let s = store(cx);
    s.sets = sets.unwrap_or_default();
    s.hint_shown = hint;
    let text = s.text();
    write_in_background(cx, path(), text);
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
    let plural = |n: usize, w: &str| if n == 1 { format!("1 {w}") } else { format!("{n} {w}s") };
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
        format!("Restore {} from last session{resume}?", plural(panes.len(), "terminal")),
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
            let _ = this.update(cx, |this, cx| {
                this.round = None;
                // Agents in every pane: one still printing isn't due for a while, and a short
                // first task would go unnoticed.
                this.refresh_agents(None, cx);
                this.save(false, cx);
            });
        }));
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
        self.write_sets(all, cx);
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

    /// The sets this panel has, into the store: the file is written when one changed, and history
    /// files no set names any more go.
    fn write_sets(&mut self, now: bool, cx: &mut Context<Self>) {
        // The worktrees whose saved terminals this panel took up: another window's are its own.
        let sets: Vec<(String, Option<SavedSet>)> = self
            .restored_here
            .iter()
            // Offered and nothing opened there since: the saved ones stay for the offer.
            .filter(|root| {
                !self.offers.contains_key(*root)
                    || self.sets.get(*root).is_some_and(|s| !s.groups.is_empty())
            })
            .map(|root| {
                let saved = self
                    .sets
                    .get(root)
                    .filter(|s| !s.groups.is_empty())
                    .map(|set| self.saved_set(set, cx));
                (root.to_string_lossy().into_owned(), saved)
            })
            .collect();
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
        // Worktrees removed since (Clean up, `git worktree remove`): their tabs and histories go.
        let gone: Vec<String> = s
            .sets
            .keys()
            .filter(|k| !Path::new(k).is_dir())
            .cloned()
            .collect();
        for root in gone {
            s.sets.remove(&root);
            changed = true;
        }
        for (root, saved) in sets {
            match saved {
                Some(saved) if s.sets.get(&root).is_none_or(|old| !same(old, &saved)) => {
                    s.sets.insert(root, saved);
                    changed = true;
                }
                None if s.sets.remove(&root).is_some() => changed = true,
                _ => {}
            }
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
        self.save(true, cx);
    }

    /// `root`'s terminals from the last run, offered the first time it shows in this one: asked,
    /// not automatic, as each starts a shell. Until then (or if the toast is closed) the save
    /// keeps them, unless terminals opened there since take their place.
    pub(super) fn restore(&mut self, root: &Path, _: &mut Window, cx: &mut Context<Self>) {
        if self.bound.is_none() || !cx.default_global::<Restored>().0.insert(root.to_path_buf()) {
            return;
        }
        self.restored_here.insert(root.to_path_buf());
        let key = root.to_string_lossy().into_owned();
        let Some(saved) = store(cx).sets.get(&key).cloned() else {
            return;
        };
        let (title, folders) = offer_text(&saved, resume_mode(cx));
        self.offers.insert(root.to_path_buf(), saved);
        let (weak, at) = (cx.weak_entity(), root.to_path_buf());
        let toast = Toast::info(folders).title(title).action("Restore", move |window, cx| {
            let _ = weak.update(cx, |p, cx| p.accept_restore(&at, window, cx));
        });
        if let Some(ws) = self.workspace() {
            ws.update(cx, |ws, cx| ws.toast(toast, cx));
        }
    }

    /// The offered terminals of `root`, beside any opened since; its agents resumed where their
    /// folder is still there.
    pub fn accept_restore(&mut self, root: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(saved) = self.offers.remove(root) else {
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
            let gid = self.next_id;
            self.next_id += 1;
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
        let set = self.sets.entry(root.to_path_buf()).or_default();
        set.groups.extend(groups);
        set.active = active;
        self.apply_dims(cx);
        if root == self.root
            && let Some(ws) = self.workspace()
        {
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

    #[test]
    fn the_old_apps_tabs_go_to_their_folders_with_their_histories_in_files() {
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
        let sets = from_legacy(&old, &mut |name, text| {
            files.push((name.to_string(), text.to_string()))
        });
        assert_eq!(sets.keys().collect::<Vec<_>>(), ["/w/a", "/w/b"]);
        let a = &sets["/w/a"];
        assert_eq!((a.groups.len(), a.saved_at), (2, 1_700_000_000.));
        let b = &sets["/w/b"];
        assert_eq!(
            (b.active, b.groups[0].name.as_deref(), b.groups[0].focused),
            (0, Some("api"), 1)
        );
        let srv = &b.groups[0].panes[1];
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
        assert_eq!(
            a.groups[0].panes[0].history.as_deref(),
            Some(files[0].0.as_str())
        );
        assert!(files[0].1.starts_with("\u{1b}[31mred"));
        assert!(b.groups[0].panes[0].history.is_none());
        assert_eq!(
            saved_layout(b.groups[0].layout.as_ref(), 2),
            Layout::Split(Split {
                dir: Dir::Row,
                children: vec![Layout::Pane(0), Layout::Pane(1)],
                sizes: vec![0.5, 0.5]
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
        assert!(safe_name("0a1b.ans"));
        assert!(!safe_name("../x.ans") && !safe_name("/etc/passwd") && !safe_name(".ans"));
    }

    #[test]
    fn the_offer_says_how_many_and_where() {
        let dir = std::env::temp_dir().to_string_lossy().into_owned();
        let agent = serde_json::json!({"name": "Claude Code", "command": "claude --resume a", "cwd": dir});
        let pane = |cwd: &str, agent: Option<Value>| SavedPane { cwd: cwd.into(), agent, ..Default::default() };
        let set = SavedSet {
            groups: vec![
                SavedGroup { panes: vec![pane(&dir, Some(agent)), pane(&dir, None)], ..Default::default() },
                SavedGroup { panes: vec![pane("/w/api", None)], ..Default::default() },
            ],
            ..Default::default()
        };
        let here = folder_name(Path::new(&dir));
        assert_eq!(offer_text(&set, ResumeMode::Type), ("Restore 3 terminals from last session and resume 1 agent?".into(), format!("{here}, api")));
        assert_eq!(offer_text(&set, ResumeMode::Off).0, "Restore 3 terminals from last session?");
    }

    #[test]
    fn a_set_reads_back_as_written() {
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
        // A removed worktree's set is left out, and the file marked for writing again.
        let text = r#"{"/gone": {"groups": []}, "/w": {"active": 0}, "agentNotifyHint": true}"#;
        let read = Store::parse(text, |k| k == "/w");
        assert_eq!(read.sets.keys().collect::<Vec<_>>(), ["/w"]);
        assert!(read.stale && read.hint_shown);
        assert!(!Store::parse(r#"{"/w": {}}"#, |_| true).stale);
        store.sets.insert("/w".into(), set.clone());
        let back: serde_json::Map<String, Value> = serde_json::from_str(&store.text()).unwrap();
        assert_eq!(
            serde_json::from_value::<SavedSet>(back["/w"].clone()).unwrap(),
            set
        );
        assert_eq!(back[HINT_KEY], Value::Bool(true));
        assert_eq!(store.named(), HashSet::from(["ab.ans".to_string()]));
    }
}
