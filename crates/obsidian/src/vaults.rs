//! The vaults the app shows and how current what's read from them is: Obsidian's list (less the
//! ones turned off in Settings), the one picked to show, each vault's file list for links (kept
//! while a view of that vault is open), and core's watcher on the vault a view showed last.
//! Views follow `VaultsEvent`; core's `Event::VaultChanged` arrives through `CoreEvents`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gitviber_core::api;
use gitviber_core::obsidian::Vault;
use gitviber_core::{Event, obsidian};
use gitviber_ui::{CoreEvents, Settings, store};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, Global, SharedString, Subscription, Task,
};
use serde_json::{Map, Value};

use crate::links::LinkIndex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VaultsEvent {
    /// Obsidian's list, the vault picked, or which vaults show changed.
    ListChanged,
    /// Files in `vault` changed: `paths` (vault-relative), or any of them when none are named.
    /// `files`: some came, went or moved.
    Changed {
        vault: String,
        files: bool,
        paths: Vec<String>,
    },
    /// `vault`'s file list was read again: links resolve against the new one.
    IndexChanged(String),
}

impl VaultsEvent {
    /// Whether this says `path` of `vault` may have changed.
    pub fn touches(&self, vault: &str, path: &str) -> bool {
        matches!(self, VaultsEvent::Changed { vault: v, paths, .. }
            if v == vault && (paths.is_empty() || paths.iter().any(|p| p == path)))
    }
}

struct IndexSlot {
    index: Option<Arc<LinkIndex>>,
    _load: Task<()>,
}

pub struct Vaults {
    /// Obsidian's vaults as last read; None before the first read.
    list: Option<Vec<Vault>>,
    picked: Option<String>,
    /// The old app's vault section was left closed (imported; nothing here closes yet).
    collapsed: bool,
    /// What Settings says about vaults, to tell its changes from others'.
    shown: (bool, Vec<String>),
    /// Open views of a vault, the latest last: the watcher follows the last one.
    holds: Vec<(u64, String)>,
    next_hold: u64,
    watched: Option<String>,
    watching: bool,
    indexes: HashMap<String, IndexSlot>,
    _read: Task<()>,
    _watch: Task<()>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<VaultsEvent> for Vaults {}

struct VaultsGlobal(Entity<Vaults>);

impl Global for VaultsGlobal {}

/// Where the vault picked to show is kept, beside the app's settings.
fn picked_file() -> PathBuf {
    store::config_dir().join("vaults.json")
}

/// The old app's picked vault and closed section, as vaults.json keeps them.
fn from_legacy(value: impl Fn(&str) -> Option<Value>) -> Map<String, Value> {
    let mut out = Map::new();
    if let Some(v) =
        value("gitviber.obsidian.vault").filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
    {
        out.insert("picked".into(), v);
    }
    if let Some(v) = value("gitviber.obsidian.collapsed").filter(Value::is_boolean) {
        out.insert("collapsed".into(), v);
    }
    out
}

/// On the first launch, what the old app kept, unless this build has its file already. Written
/// now, not in the background: the model reads the file as soon as a view asks for it.
pub(crate) fn import_legacy() {
    let saved = from_legacy(gitviber_ui::legacy::value);
    let path = picked_file();
    if !saved.is_empty() && !path.exists() {
        let text = serde_json::to_string_pretty(&Value::Object(saved)).unwrap_or_default();
        if let Err(e) = store::write_atomic(&path, &text) {
            eprintln!("gitviber: couldn't keep the old app's vault: {e}");
        }
    }
}

impl Vaults {
    /// The app's one model, made the first time it's asked for (after core is installed).
    pub fn global(cx: &mut App) -> Entity<Vaults> {
        if let Some(g) = cx.try_global::<VaultsGlobal>() {
            return g.0.clone();
        }
        let model = cx.new(Vaults::new);
        cx.set_global(VaultsGlobal(model.clone()));
        model
    }

    fn new(cx: &mut Context<Self>) -> Self {
        let saved = store::read_object(&picked_file()).unwrap_or_default();
        let picked = saved
            .get("picked")
            .and_then(Value::as_str)
            .map(str::to_string);
        let collapsed = saved
            .get("collapsed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let shown = |cx: &App| {
            let s = Settings::get(cx);
            (s.obsidian, s.hidden_vaults.clone())
        };
        let mut subs = vec![cx.observe_global::<Settings>(move |this: &mut Self, cx| {
            let now = shown(cx);
            if now != this.shown {
                this.shown = now;
                cx.emit(VaultsEvent::ListChanged);
                this.rewatch(cx);
            }
        })];
        subs.push(
            cx.subscribe(&CoreEvents::global(cx), |this, _, e: &Event, cx| {
                if let Event::VaultChanged(c) = e {
                    this.changed(c.vault.clone(), c.files, c.paths.clone(), cx);
                }
            }),
        );
        let mut this = Vaults {
            list: None,
            picked,
            collapsed,
            shown: shown(cx),
            holds: Vec::new(),
            next_hold: 0,
            watched: None,
            watching: false,
            indexes: HashMap::new(),
            _read: Task::ready(()),
            _watch: Task::ready(()),
            _subs: subs,
        };
        this.refresh(cx);
        this
    }

    /// Reads Obsidian's vault list again: a vault opened, added or removed in Obsidian shows up.
    /// The vaults open here that no watcher covers are read again too.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let core = gitviber_ui::core(cx);
        self._read = cx.spawn(async move |this, cx| {
            let list = cx
                .background_spawn(async move { api::obsidian::vaults(&core).unwrap_or_default() })
                .await;
            _ = this.update(cx, |this, cx| {
                if this.list.as_ref() != Some(&list) {
                    this.list = Some(list);
                    cx.emit(VaultsEvent::ListChanged);
                    this.rewatch(cx);
                }
            });
        });
        let unwatched: Vec<String> = self
            .held_vaults()
            .into_iter()
            .filter(|v| self.watched.as_ref() != Some(v))
            .collect();
        for vault in unwatched {
            self.changed(vault, true, vec![], cx);
        }
    }

    fn changed(&mut self, vault: String, files: bool, paths: Vec<String>, cx: &mut Context<Self>) {
        if (files || paths.is_empty()) && self.indexes.contains_key(&vault) {
            self.load_index(&vault, cx);
        }
        cx.emit(VaultsEvent::Changed {
            vault,
            files,
            paths,
        });
    }

    /// Every vault Obsidian lists that may show; none while Settings turns Obsidian off.
    pub fn vaults(&self, cx: &App) -> Vec<Vault> {
        let s = Settings::get(cx);
        if !s.obsidian {
            return vec![];
        }
        self.list
            .iter()
            .flatten()
            .filter(|v| !s.hidden_vaults.contains(&v.path))
            .cloned()
            .collect()
    }

    /// The old app's vault section was left closed (its first launch here).
    pub fn collapsed(&self) -> bool {
        self.collapsed
    }

    /// Whether Obsidian's list has been read yet.
    pub fn loaded(&self) -> bool {
        self.list.is_some()
    }

    /// The vault to show: the one picked last, else the last opened in Obsidian.
    pub fn current(&self, cx: &App) -> Option<Vault> {
        let all = self.vaults(cx);
        let picked = self.picked.as_ref();
        all.iter()
            .find(|v| Some(&v.path) == picked)
            .or(all.first())
            .cloned()
    }

    /// Shows `vault` (its path) from now on.
    pub fn pick(&mut self, vault: &str, cx: &mut Context<Self>) {
        if self.picked.as_deref() == Some(vault) {
            return;
        }
        self.picked = Some(vault.to_string());
        let mut ours = Map::new();
        ours.insert("picked".into(), Value::String(vault.to_string()));
        store::save_object(cx, picked_file(), ours);
        cx.emit(VaultsEvent::ListChanged);
    }

    /// A vault's name as Obsidian's switcher shows it: its folder's.
    pub fn name_of(&self, vault: &str) -> SharedString {
        self.list
            .iter()
            .flatten()
            .find(|v| v.path == vault)
            .map(|v| v.name.clone())
            .unwrap_or_else(|| vault.rsplit('/').next().unwrap_or(vault).to_string())
            .into()
    }

    /// A view of `vault` opened: the watcher moves to it, and its file list is kept until the
    /// last view of it lets go (`release`).
    pub fn hold(&mut self, vault: &str, cx: &mut Context<Self>) -> u64 {
        self.next_hold += 1;
        self.holds.push((self.next_hold, vault.to_string()));
        self.rewatch(cx);
        self.next_hold
    }

    pub fn release(&mut self, id: u64, cx: &mut Context<Self>) {
        self.holds.retain(|(h, _)| *h != id);
        let held = self.held_vaults();
        self.indexes.retain(|v, _| held.contains(v));
        self.rewatch(cx);
    }

    fn held_vaults(&self) -> Vec<String> {
        let mut out: Vec<String> = self.holds.iter().map(|(_, v)| v.clone()).collect();
        out.dedup();
        out
    }

    /// `vault`'s file list for links, read in the background the first time and again after
    /// files came or went (then `IndexChanged`); the one before meanwhile, None at first.
    pub fn index(&mut self, vault: &str, cx: &mut Context<Self>) -> Option<Arc<LinkIndex>> {
        if !self.indexes.contains_key(vault) {
            self.indexes.insert(
                vault.to_string(),
                IndexSlot {
                    index: None,
                    _load: Task::ready(()),
                },
            );
            self.load_index(vault, cx);
        }
        self.indexes.get(vault).and_then(|s| s.index.clone())
    }

    fn load_index(&mut self, vault: &str, cx: &mut Context<Self>) {
        let core = gitviber_ui::core(cx);
        let (key, v) = (vault.to_string(), vault.to_string());
        let load = cx.spawn(async move |this, cx| {
            let index = cx
                .background_spawn(async move {
                    let files = api::obsidian::vault_files(&core, v).unwrap_or_default();
                    Arc::new(LinkIndex::new(files))
                })
                .await;
            _ = this.update(cx, |this, cx| {
                if let Some(slot) = this.indexes.get_mut(&key) {
                    slot.index = Some(index);
                    cx.emit(VaultsEvent::IndexChanged(key));
                }
            });
        });
        if let Some(slot) = self.indexes.get_mut(vault) {
            slot._load = load;
        }
    }

    /// The vault core should watch: the one a view showed last, while it may show.
    fn watch_target(&self, cx: &App) -> Option<String> {
        let shown = self.vaults(cx);
        self.holds
            .iter()
            .rev()
            .map(|(_, v)| v)
            .find(|v| shown.iter().any(|s| &s.path == *v))
            .cloned()
    }

    /// Points core's one vault watcher at `watch_target`, one change at a time.
    fn rewatch(&mut self, cx: &mut Context<Self>) {
        // Core's watcher thread would wake a test's deterministic executor.
        if cfg!(test) || self.watching || self.watch_target(cx) == self.watched {
            return;
        }
        self.watching = true;
        self._watch = cx.spawn(async move |this, cx| {
            loop {
                let next = this.update(cx, |this, cx| {
                    let want = this.watch_target(cx);
                    if want == this.watched {
                        this.watching = false;
                        None
                    } else {
                        Some((want, gitviber_ui::core(cx)))
                    }
                });
                let Ok(Some((want, core))) = next else {
                    break;
                };
                let asked = want.clone();
                let result = cx
                    .background_spawn(async move { api::obsidian::vault_watch(&core, asked) })
                    .await;
                if let Err(e) = result {
                    eprintln!("gitviber: couldn't watch the vault: {e}");
                }
                if this.update(cx, |this, _| this.watched = want).is_err() {
                    break;
                }
            }
        });
    }

    /// The vault's folder on disk, when it's one Obsidian lists.
    pub fn root(vault: &str) -> Option<PathBuf> {
        obsidian::root(vault).ok()
    }
}

/// Holds `vault` for the entity `cx` builds, released when it is.
pub(crate) fn hold_for<T: 'static>(vault: &str, cx: &mut Context<T>) {
    let model = Vaults::global(cx);
    let id = model.update(cx, |m, cx| m.hold(vault, cx));
    cx.on_release(move |_, cx| {
        if let Some(g) = cx.try_global::<VaultsGlobal>() {
            g.0.clone().update(cx, |m, cx| m.release(id, cx));
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_apps_vault_and_section_come_over() {
        let old = |key: &str| match key {
            "gitviber.obsidian.vault" => Some(Value::String("/notes/Work".into())),
            "gitviber.obsidian.collapsed" => Some(Value::Bool(true)),
            _ => None,
        };
        let saved = from_legacy(old);
        assert_eq!(
            saved.get("picked"),
            Some(&Value::String("/notes/Work".into()))
        );
        assert_eq!(saved.get("collapsed"), Some(&Value::Bool(true)));
        // Wrong types and empty paths stay behind.
        let odd = |key: &str| match key {
            "gitviber.obsidian.vault" => Some(Value::String(String::new())),
            "gitviber.obsidian.collapsed" => Some(Value::String("yes".into())),
            _ => None,
        };
        assert!(from_legacy(odd).is_empty());
        assert!(from_legacy(|_| None).is_empty());
    }
}
