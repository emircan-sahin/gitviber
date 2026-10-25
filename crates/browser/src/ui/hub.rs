//! What the pages' views report (browser-state, -console, -picked, -download…), kept per tab id
//! on the UI thread for the tabs to read, and the events that concern the workspace (an agent's
//! tab opened or closed, a popup) handed to agents.rs. A closed tab's entry goes with it.

use super::format::Pick;
use crate::{Event, PageState};
use futures::channel::mpsc;
use futures::StreamExt as _;
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Global, Task};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// A tab's page as its view told it.
#[derive(Default)]
pub struct Page {
    pub state: Option<PageState>,
    /// How often its view parked: a new one is due each time it's shown after.
    pub parks: u32,
    pub errors: u32,
    pub warnings: u32,
    pub downloads: Vec<Download>,
    dismissed: HashSet<u64>,
    /// The picker's news since the tab last took it: a pick, or None when it ended without one.
    pub picked: Vec<Option<Pick>>,
    /// The pane whose agent opened it; None for the user's own tabs.
    pub agent: Option<u32>,
    /// Opened for the user (`tab new --show`) by this pane: theirs until given back.
    pub shown_by: Option<u32>,
    /// The user took it from its agents with a press or a key of their own.
    pub taken: bool,
    /// Its pane closed: the user's now.
    pub released: bool,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub tab: String,
    pub key: u64,
    pub name: String,
    /// going, done, failed, cancelled.
    pub state: String,
    pub received: i64,
    /// -1 until the server says.
    pub total: i64,
    pub path: Option<String>,
    pub why: Option<String>,
}

/// The most downloads a tab lists; the oldest ended one makes room.
const DOWNLOADS: usize = 4;

/// How long an ended download stays listed, unless dismissed first.
const ENDED_SHOWN: Duration = Duration::from_secs(60);

/// `list` with this report of a download: a new entry, or its entry changed. Not one dismissed
/// already, nor a new one when the list is full of downloads going.
pub fn reported(list: &mut Vec<Download>, now: Download, dismissed: &HashSet<u64>) {
    if dismissed.contains(&now.key) {
        return;
    }
    if let Some(d) = list.iter_mut().find(|d| d.key == now.key) {
        *d = now;
        return;
    }
    while list.len() >= DOWNLOADS {
        match list.iter().position(|d| d.state != "going") {
            Some(ended) => drop(list.remove(ended)),
            None => return,
        }
    }
    list.push(now);
}

/// Something of tab `id` changed, or happened to it.
#[derive(Clone, Debug, PartialEq)]
pub enum HubEvent {
    Changed(String),
    /// Its page took the keys.
    Focused(String),
    /// Its view parked: the tab makes another when it shows again.
    Parked(String),
}

#[derive(Default)]
pub struct Hub {
    pages: HashMap<String, Page>,
}

impl EventEmitter<HubEvent> for Hub {}

struct HubGlobal {
    hub: Entity<Hub>,
    _pump: Task<()>,
}

impl Global for HubGlobal {}

impl Hub {
    pub fn global(cx: &App) -> Option<Entity<Hub>> {
        cx.try_global::<HubGlobal>().map(|g| g.hub.clone())
    }

    pub fn page(&self, id: &str) -> Option<&Page> {
        self.pages.get(id)
    }

    pub fn page_mut(&mut self, id: &str) -> &mut Page {
        self.pages.entry(id.to_string()).or_default()
    }

    /// A closed tab's page, forgotten: its view went with it, and a reopened one starts afresh.
    pub fn forget(&mut self, id: &str) {
        self.pages.remove(id);
    }

    pub fn dismiss_download(&mut self, id: &str, key: u64, cx: &mut Context<Self>) {
        if let Some(p) = self.pages.get_mut(id) {
            p.dismissed.insert(key);
            p.downloads.retain(|d| d.key != key);
            cx.emit(HubEvent::Changed(id.to_string()));
        }
    }

    pub fn take_picked(&mut self, id: &str) -> Vec<Option<Pick>> {
        self.pages
            .get_mut(id)
            .map(|p| std::mem::take(&mut p.picked))
            .unwrap_or_default()
    }

    /// The tab ids this page set holds, for the leak tests.
    #[cfg(test)]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// One event of a view's, on the UI thread. Those of the workspace go on to agents.rs.
    fn hear(&mut self, event: Event, cx: &mut Context<Self>) -> Option<Event> {
        #[derive(Deserialize)]
        struct Id {
            id: String,
        }
        let id_of = |e: &Event| {
            serde_json::from_value::<Id>(e.payload.clone())
                .ok()
                .map(|i| i.id)
        };
        match event.name {
            "browser-state" => {
                let state: PageState = serde_json::from_value(event.payload).ok()?;
                let id = state.id.clone();
                self.page_mut(&id).state = Some(state);
                cx.emit(HubEvent::Changed(id));
            }
            "browser-parked" => {
                let id = id_of(&event)?;
                let page = self.page_mut(&id);
                page.state = None;
                page.parks += 1;
                cx.emit(HubEvent::Parked(id));
            }
            "browser-console" => {
                #[derive(Deserialize)]
                struct Logged {
                    id: String,
                    errors: u32,
                    warnings: u32,
                }
                let l: Logged = serde_json::from_value(event.payload).ok()?;
                let page = self.page_mut(&l.id);
                (page.errors, page.warnings) = (l.errors, l.warnings);
                cx.emit(HubEvent::Changed(l.id));
            }
            "browser-picked" => {
                #[derive(Deserialize)]
                struct Picked {
                    id: String,
                    pick: Option<Pick>,
                }
                let p: Picked = serde_json::from_value(event.payload).ok()?;
                self.page_mut(&p.id).picked.push(p.pick);
                cx.emit(HubEvent::Changed(p.id));
            }
            "browser-download" => {
                let d: Download = serde_json::from_value(event.payload).ok()?;
                let (id, key, ended) = (d.tab.clone(), d.key, d.state != "going");
                let page = self.page_mut(&id);
                let dismissed = std::mem::take(&mut page.dismissed);
                reported(&mut page.downloads, d, &dismissed);
                page.dismissed = dismissed;
                cx.emit(HubEvent::Changed(id.clone()));
                if ended {
                    cx.spawn(async move |hub, cx| {
                        cx.background_executor().timer(ENDED_SHOWN).await;
                        let _ = hub.update(cx, |hub, cx| hub.dismiss_download(&id, key, cx));
                    })
                    .detach();
                }
            }
            "browser-focus" => {
                let id = id_of(&event)?;
                cx.emit(HubEvent::Focused(id));
            }
            "browser-owner" => {
                #[derive(Deserialize)]
                struct Owned {
                    id: String,
                    user: bool,
                }
                let o: Owned = serde_json::from_value(event.payload).ok()?;
                let page = self.page_mut(&o.id);
                page.taken = o.user;
                // Given back, a tab shown to the user is an agent's like any other.
                if !o.user {
                    if let Some(pty) = page.shown_by.take() {
                        page.agent = Some(pty);
                    }
                }
                cx.emit(HubEvent::Changed(o.id));
            }
            "browser-released" => {
                #[derive(Deserialize)]
                struct Released {
                    ids: Vec<String>,
                }
                let r: Released = serde_json::from_value(event.payload).ok()?;
                for id in r.ids {
                    self.page_mut(&id).released = true;
                    cx.emit(HubEvent::Changed(id));
                }
            }
            "browser-open" => {
                #[derive(Deserialize)]
                struct Opened {
                    id: String,
                    pty: u32,
                    show: bool,
                }
                if let Ok(o) = serde_json::from_value::<Opened>(event.payload.clone()) {
                    let page = self.page_mut(&o.id);
                    if o.show {
                        page.shown_by = Some(o.pty);
                    } else {
                        page.agent = Some(o.pty);
                    }
                }
                return Some(event);
            }
            _ => return Some(event),
        }
        None
    }
}

/// The sink the views send to, and the hub that hears it on the UI thread. Once, at launch.
pub(super) fn install(cx: &mut App, workspace_event: fn(Event, &mut App)) -> crate::Sink {
    let (tx, mut rx) = mpsc::unbounded::<Event>();
    let hub = cx.new(|_| Hub::default());
    let weak = hub.downgrade();
    let pump = cx.spawn(async move |cx| {
        while let Some(event) = rx.next().await {
            let rest = weak.update(cx, |hub, cx| hub.hear(event, cx));
            match rest {
                Ok(Some(event)) => cx.update(|cx| workspace_event(event, cx)),
                Ok(None) => {}
                Err(_) => break,
            }
        }
    });
    cx.set_global(HubGlobal { hub, _pump: pump });
    std::sync::Arc::new(move |event| {
        let _ = tx.unbounded_send(event);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(key: u64, state: &str) -> Download {
        Download {
            tab: "t".into(),
            key,
            name: format!("f{key}"),
            state: state.into(),
            received: 0,
            total: -1,
            path: None,
            why: None,
        }
    }

    /// An ended download leaves the list a minute later, and isn't listed again if reported.
    #[gpui::test]
    fn an_ended_download_drops_from_the_list_after_a_minute(cx: &mut gpui::TestAppContext) {
        let sink = cx.update(|cx| install(cx, |_, _| {}));
        let hub = cx.update(|cx| Hub::global(cx).unwrap());
        let report = |key: u64, state: &str| {
            sink(Event {
                name: "browser-download",
                payload: serde_json::json!({
                    "tab": "t", "key": key, "name": format!("f{key}"), "state": state,
                    "received": 1, "total": 1,
                }),
            })
        };
        let listed = |cx: &mut gpui::TestAppContext| {
            hub.read_with(cx, |h, _| {
                h.page("t")
                    .map(|p| p.downloads.iter().map(|d| d.key).collect::<Vec<_>>())
                    .unwrap_or_default()
            })
        };
        report(1, "going");
        report(2, "going");
        cx.run_until_parked();
        report(1, "done");
        cx.run_until_parked();
        cx.executor()
            .advance_clock(ENDED_SHOWN - Duration::from_secs(1));
        cx.run_until_parked();
        assert_eq!(listed(cx), [1, 2], "shown a while once ended");
        cx.executor().advance_clock(Duration::from_secs(2));
        cx.run_until_parked();
        assert_eq!(listed(cx), [2], "the one going stays");
        report(1, "done");
        cx.run_until_parked();
        assert_eq!(listed(cx), [2], "gone for good");
    }

    #[test]
    fn a_tab_lists_a_few_downloads_the_oldest_ended_making_room() {
        let mut list = Vec::new();
        let none = HashSet::new();
        for k in 0..4 {
            reported(&mut list, d(k, "going"), &none);
        }
        reported(&mut list, d(9, "going"), &none);
        assert_eq!(list.len(), 4, "full of downloads going: no room");
        reported(&mut list, d(1, "done"), &none);
        reported(&mut list, d(9, "going"), &none);
        assert_eq!(list.iter().map(|d| d.key).collect::<Vec<_>>(), [0, 2, 3, 9]);
        let dismissed = HashSet::from([2]);
        reported(&mut list, d(2, "done"), &dismissed);
        assert_eq!(list.iter().find(|x| x.key == 2).unwrap().state, "going");
    }
}
