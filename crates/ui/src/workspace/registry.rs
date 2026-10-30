//! What feature crates add to every workspace, registered once at startup from their `init`:
//! side and bottom panels, top bar and status bar items, quick open's source.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::{AnyElement, App, Entity, Global, SharedString, WeakEntity, Window};

use super::item::{Panel, PanelHandle};
use super::{Project, Workspace};
use crate::fuzzy::Candidates;

/// Where a panel lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    /// Git work: Changes, History, PRs, Issues.
    Left,
    /// Files: Explorer, Search.
    Right,
    /// The terminal.
    Bottom,
}

/// What a panel or bar item is built with.
#[derive(Clone)]
pub struct PanelContext {
    pub workspace: WeakEntity<Workspace>,
    /// None while no project is open.
    pub project: Option<Entity<Project>>,
}

type BuildPanel = Rc<dyn Fn(&PanelContext, &mut Window, &mut App) -> PanelHandle>;

#[derive(Clone)]
pub struct PanelSpec {
    /// Stable, as the views' focus commands and the saved layout name it ("changes", "explorer").
    pub id: &'static str,
    pub slot: Slot,
    pub title: SharedString,
    /// Lower comes first in the panel's tabs.
    pub order: i32,
    /// Built again for each project; else built once per window and kept (the terminal).
    pub per_project: bool,
    build: BuildPanel,
}

impl PanelSpec {
    /// A panel built the first time it shows, by `build`.
    pub fn new<V: Panel>(
        id: &'static str,
        slot: Slot,
        title: impl Into<SharedString>,
        build: impl Fn(&PanelContext, &mut Window, &mut App) -> Entity<V> + 'static,
    ) -> Self {
        Self {
            id,
            slot,
            title: title.into(),
            order: 0,
            per_project: true,
            build: Rc::new(move |ctx, window, cx| PanelHandle::new(build(ctx, window, cx))),
        }
    }

    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }

    /// Kept across project switches.
    pub fn per_window(mut self) -> Self {
        self.per_project = false;
        self
    }

    pub(crate) fn build(
        &self,
        ctx: &PanelContext,
        window: &mut Window,
        cx: &mut App,
    ) -> PanelHandle {
        (self.build)(ctx, window, cx)
    }
}

/// Where a bar item goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BarSlot {
    /// The top bar's chips after the logo: project, branch, worktree, pull request.
    TopChips,
    /// Before the top bar's panel toggles.
    TopActions,
    StatusLeft,
    StatusRight,
}

/// Draws the item; None leaves it out for now (no project, nothing to say).
pub type RenderBarItem = Rc<dyn Fn(&PanelContext, &mut Window, &mut App) -> Option<AnyElement>>;

#[derive(Clone)]
pub struct BarItem {
    /// Stable, so the app's placeholder for it gives way ("branch", "pull").
    pub id: &'static str,
    pub slot: BarSlot,
    pub order: i32,
    pub render: RenderBarItem,
}

impl BarItem {
    pub fn new(
        id: &'static str,
        slot: BarSlot,
        render: impl Fn(&PanelContext, &mut Window, &mut App) -> Option<AnyElement> + 'static,
    ) -> Self {
        Self {
            id,
            slot,
            order: 0,
            render: Rc::new(render),
        }
    }

    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }
}

/// Quick open's files (⌘P) for a project: paths relative to it, shared as read, so opening the
/// palette copies nothing.
#[derive(Clone)]
pub struct QuickOpen {
    pub files: Arc<Candidates>,
    /// Opens one of `files`.
    pub open: OpenFile,
}

/// Opens a file of quick open's list by its relative path.
pub type OpenFile = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// Quick open's files for a project: all of them; the palette matches and ranks.
pub type QuickOpenSource = Rc<dyn Fn(&PanelContext, &mut App) -> QuickOpen>;

#[derive(Default)]
pub struct Registry {
    panels: Vec<PanelSpec>,
    bar_items: Vec<BarItem>,
    quick_open: Option<QuickOpenSource>,
}

impl Global for Registry {}

impl Registry {
    pub fn get(cx: &App) -> &Registry {
        cx.global::<Registry>()
    }

    pub fn all_panels(&self) -> &[PanelSpec] {
        &self.panels
    }

    /// A slot's panels in their order.
    pub fn panels(&self, slot: Slot) -> Vec<PanelSpec> {
        let mut out: Vec<PanelSpec> = self
            .panels
            .iter()
            .filter(|p| p.slot == slot)
            .cloned()
            .collect();
        out.sort_by_key(|p| p.order);
        out
    }

    pub fn panel(&self, id: &str) -> Option<&PanelSpec> {
        self.panels.iter().find(|p| p.id == id)
    }

    pub fn bar_items(&self, slot: BarSlot) -> Vec<BarItem> {
        let mut out: Vec<BarItem> = self
            .bar_items
            .iter()
            .filter(|b| b.slot == slot)
            .cloned()
            .collect();
        out.sort_by_key(|b| b.order);
        out
    }

    pub fn quick_open(&self) -> Option<QuickOpenSource> {
        self.quick_open.clone()
    }
}

/// Adds a panel to every workspace; one with the same id replaces it (a feature's real panel
/// over the app's placeholder).
pub fn register_panel(cx: &mut App, spec: PanelSpec) {
    let registry = cx.default_global::<Registry>();
    registry.panels.retain(|p| p.id != spec.id);
    registry.panels.push(spec);
}

/// Adds a panel unless one with its id is there: for placeholders.
pub fn register_panel_default(cx: &mut App, spec: PanelSpec) {
    if cx.default_global::<Registry>().panel(spec.id).is_none() {
        register_panel(cx, spec);
    }
}

pub fn register_bar_item(cx: &mut App, item: BarItem) {
    let registry = cx.default_global::<Registry>();
    registry.bar_items.retain(|b| b.id != item.id);
    registry.bar_items.push(item);
}

pub fn register_bar_item_default(cx: &mut App, item: BarItem) {
    if !cx
        .default_global::<Registry>()
        .bar_items
        .iter()
        .any(|b| b.id == item.id)
    {
        register_bar_item(cx, item);
    }
}

pub fn register_quick_open(cx: &mut App, source: QuickOpenSource) {
    cx.default_global::<Registry>().quick_open = Some(source);
}
