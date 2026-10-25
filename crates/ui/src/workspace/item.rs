//! What a feature puts in the center (a diff, a file, a PR) and in the side panels.

use std::rc::Rc;

use futures::channel::oneshot;
use gpui_kit::assets::IconName;
use gpui_kit::{
    AnyView, App, Context, Entity, EntityId, FocusHandle, Render, SharedString, Task, Window,
};

/// A center tab's view. Implement it on the view entity's type and open it with
/// `Workspace::open_tab`; the strip reads the title and icon from the entity as it changes.
pub trait TabItem: Render {
    fn tab_title(&self, cx: &App) -> SharedString;

    /// Dim text after the title ("diff", "staged").
    fn tab_detail(&self, _cx: &App) -> Option<SharedString> {
        None
    }

    fn tab_icon(&self, _cx: &App) -> TabIcon {
        TabIcon::None
    }

    fn tab_tooltip(&self, _cx: &App) -> Option<SharedString> {
        None
    }

    fn closable(&self, _cx: &App) -> bool {
        true
    }

    /// Unsaved edits: the strip marks it, and closing it should ask.
    fn is_dirty(&self, _cx: &App) -> bool {
        false
    }

    /// Where focus goes when the tab is shown or the code view focused (⌘E).
    fn tab_focus(&self, _cx: &App) -> Option<FocusHandle> {
        None
    }

    /// Whether Reopen Closed Tab (⇧⌘T) may build it again from its key; false for a tab that
    /// can't come back as it was (a page an agent opened).
    fn reopenable(&self, _cx: &App) -> bool {
        true
    }

    /// Left out of the tab strip while it keeps running (an agent's page with Settings → Browser
    /// → Show agents' tabs off).
    fn hidden(&self, _cx: &App) -> bool {
        false
    }

    /// Writes its unsaved edits, for the ask before they'd be lost (Save): the task answers true
    /// once they're written, false when they weren't (a failure, an overwrite declined). None: it
    /// can't save them (a comment's draft), so the ask offers to discard them only.
    fn save(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Task<bool>>
    where
        Self: Sized,
    {
        None
    }

    /// Whether `save` can write its unsaved edits; false for a comment's draft.
    fn can_save(&self, _cx: &App) -> bool {
        false
    }

    /// Drops its unsaved edits for good (Don't Save), wherever it kept them.
    fn discard(&mut self, _cx: &mut Context<Self>)
    where
        Self: Sized,
    {
    }

    /// Its unsaved edits outlive it: kept when it closes and back when it opens again (a diff's).
    /// A worktree switch or a quit then needn't ask about them; closing the tab still does.
    fn keeps_edits(&self, _cx: &App) -> bool {
        false
    }
}

/// A save's answer for `TabItem::save`: answered once with `done`; dropped unanswered, it's false.
pub struct SaveDone(Option<oneshot::Sender<bool>>);

impl SaveDone {
    pub fn done(mut self, saved: bool) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(saved);
        }
    }
}

impl Drop for SaveDone {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(false);
        }
    }
}

/// A save's answer and the task `TabItem::save` returns for it.
pub fn save_signal(cx: &App) -> (SaveDone, Task<bool>) {
    let (tx, rx) = oneshot::channel();
    let task = cx
        .background_executor()
        .spawn(async move { rx.await.unwrap_or(false) });
    (SaveDone(Some(tx)), task)
}

/// The icon before a tab's title.
#[derive(Clone, Debug, PartialEq)]
pub enum TabIcon {
    None,
    /// A file's icon, by its path.
    File(SharedString),
    Icon(IconName),
}

trait AnyTab {
    fn title(&self, cx: &App) -> SharedString;
    fn detail(&self, cx: &App) -> Option<SharedString>;
    fn icon(&self, cx: &App) -> TabIcon;
    fn tooltip(&self, cx: &App) -> Option<SharedString>;
    fn closable(&self, cx: &App) -> bool;
    fn dirty(&self, cx: &App) -> bool;
    fn focus(&self, cx: &App) -> Option<FocusHandle>;
    fn reopenable(&self, cx: &App) -> bool;
    fn hidden(&self, cx: &App) -> bool;
    fn save(&self, window: &mut Window, cx: &mut App) -> Option<Task<bool>>;
    fn can_save(&self, cx: &App) -> bool;
    fn discard(&self, cx: &mut App);
    fn keeps_edits(&self, cx: &App) -> bool;
    fn view(&self) -> AnyView;
    fn id(&self) -> EntityId;
}

impl<V: TabItem> AnyTab for Entity<V> {
    fn title(&self, cx: &App) -> SharedString {
        self.read(cx).tab_title(cx)
    }
    fn detail(&self, cx: &App) -> Option<SharedString> {
        self.read(cx).tab_detail(cx)
    }
    fn icon(&self, cx: &App) -> TabIcon {
        self.read(cx).tab_icon(cx)
    }
    fn tooltip(&self, cx: &App) -> Option<SharedString> {
        self.read(cx).tab_tooltip(cx)
    }
    fn closable(&self, cx: &App) -> bool {
        self.read(cx).closable(cx)
    }
    fn dirty(&self, cx: &App) -> bool {
        self.read(cx).is_dirty(cx)
    }
    fn focus(&self, cx: &App) -> Option<FocusHandle> {
        self.read(cx).tab_focus(cx)
    }
    fn reopenable(&self, cx: &App) -> bool {
        self.read(cx).reopenable(cx)
    }
    fn hidden(&self, cx: &App) -> bool {
        self.read(cx).hidden(cx)
    }
    fn save(&self, window: &mut Window, cx: &mut App) -> Option<Task<bool>> {
        self.update(cx, |v, cx| v.save(window, cx))
    }
    fn can_save(&self, cx: &App) -> bool {
        self.read(cx).can_save(cx)
    }
    fn discard(&self, cx: &mut App) {
        self.update(cx, |v, cx| v.discard(cx))
    }
    fn keeps_edits(&self, cx: &App) -> bool {
        self.read(cx).keeps_edits(cx)
    }
    fn view(&self) -> AnyView {
        self.clone().into()
    }
    fn id(&self) -> EntityId {
        self.entity_id()
    }
}

/// A tab's view with its type erased, as the workspace keeps it. Cheap to clone.
#[derive(Clone)]
pub struct TabHandle(Rc<dyn AnyTab>);

impl TabHandle {
    pub fn new<V: TabItem>(entity: Entity<V>) -> Self {
        Self(Rc::new(entity))
    }

    pub fn title(&self, cx: &App) -> SharedString {
        self.0.title(cx)
    }
    pub fn detail(&self, cx: &App) -> Option<SharedString> {
        self.0.detail(cx)
    }
    pub fn icon(&self, cx: &App) -> TabIcon {
        self.0.icon(cx)
    }
    pub fn tooltip(&self, cx: &App) -> Option<SharedString> {
        self.0.tooltip(cx)
    }
    pub fn closable(&self, cx: &App) -> bool {
        self.0.closable(cx)
    }
    pub fn is_dirty(&self, cx: &App) -> bool {
        self.0.dirty(cx)
    }
    pub fn focus_handle(&self, cx: &App) -> Option<FocusHandle> {
        self.0.focus(cx)
    }
    pub fn reopenable(&self, cx: &App) -> bool {
        self.0.reopenable(cx)
    }
    pub fn hidden(&self, cx: &App) -> bool {
        self.0.hidden(cx)
    }
    pub fn save(&self, window: &mut Window, cx: &mut App) -> Option<Task<bool>> {
        self.0.save(window, cx)
    }
    pub fn can_save(&self, cx: &App) -> bool {
        self.0.can_save(cx)
    }
    pub fn discard(&self, cx: &mut App) {
        self.0.discard(cx)
    }
    pub fn keeps_edits(&self, cx: &App) -> bool {
        self.0.keeps_edits(cx)
    }
    pub fn view(&self) -> AnyView {
        self.0.view()
    }
    pub fn entity_id(&self) -> EntityId {
        self.0.id()
    }
}

impl<V: TabItem> From<Entity<V>> for TabHandle {
    fn from(entity: Entity<V>) -> Self {
        Self::new(entity)
    }
}

/// A side panel's view (Changes, History, Explorer…) or the bottom one (the terminal).
pub trait Panel: Render {
    /// A count beside its name in the panel's tab ("Changes 3").
    fn badge(&self, _cx: &App) -> Option<SharedString> {
        None
    }

    /// Where focus goes when the panel is focused (⇧⌘G, F6): its list's current row, say.
    fn panel_focus(&self, _cx: &App) -> Option<FocusHandle> {
        None
    }
}

trait AnyPanel {
    fn badge(&self, cx: &App) -> Option<SharedString>;
    fn focus(&self, cx: &App) -> Option<FocusHandle>;
    fn view(&self) -> AnyView;
}

impl<V: Panel> AnyPanel for Entity<V> {
    fn badge(&self, cx: &App) -> Option<SharedString> {
        self.read(cx).badge(cx)
    }
    fn focus(&self, cx: &App) -> Option<FocusHandle> {
        self.read(cx).panel_focus(cx)
    }
    fn view(&self) -> AnyView {
        self.clone().into()
    }
}

#[derive(Clone)]
pub struct PanelHandle(Rc<dyn AnyPanel>);

impl PanelHandle {
    pub fn new<V: Panel>(entity: Entity<V>) -> Self {
        Self(Rc::new(entity))
    }
    pub fn badge(&self, cx: &App) -> Option<SharedString> {
        self.0.badge(cx)
    }
    pub fn focus_handle(&self, cx: &App) -> Option<FocusHandle> {
        self.0.focus(cx)
    }
    pub fn view(&self) -> AnyView {
        self.0.view()
    }
}

impl<V: Panel> From<Entity<V>> for PanelHandle {
    fn from(entity: Entity<V>) -> Self {
        Self::new(entity)
    }
}

/// A center tab: what it shows, and its view while it has one. A tab restored from a saved
/// workspace, or one not looked at for a while, has only its selection; its view is built
/// when it's shown, so tabs out of sight don't keep their data.
#[derive(Clone)]
pub struct Tab {
    pub(crate) handle: Option<TabHandle>,
    pub(crate) selection: Option<super::Selection>,
}

impl Tab {
    pub fn handle(&self) -> Option<&TabHandle> {
        self.handle.as_ref()
    }

    pub fn selection(&self) -> Option<&super::Selection> {
        self.selection.as_ref()
    }

    /// Left out of the tab strip (`TabItem::hidden`); a tab with no view yet has nothing to hide.
    pub fn hidden(&self, cx: &App) -> bool {
        self.handle.as_ref().is_some_and(|h| h.hidden(cx))
    }

    pub fn title(&self, cx: &App) -> SharedString {
        match (&self.handle, &self.selection) {
            (Some(h), _) => h.title(cx),
            (None, Some(s)) => s.title(),
            _ => SharedString::default(),
        }
    }

    pub fn detail(&self, cx: &App) -> Option<SharedString> {
        match (&self.handle, &self.selection) {
            (Some(h), _) => h.detail(cx),
            (None, Some(s)) => s.detail(),
            _ => None,
        }
    }

    pub fn icon(&self, cx: &App) -> TabIcon {
        match (&self.handle, &self.selection) {
            (Some(h), _) => h.icon(cx),
            (None, Some(s)) => s.icon(),
            _ => TabIcon::None,
        }
    }

    pub fn tooltip(&self, cx: &App) -> Option<SharedString> {
        match (&self.handle, &self.selection) {
            (Some(h), _) => h.tooltip(cx),
            (None, Some(s)) => Some(super::selection_path(s).into()),
            _ => None,
        }
    }

    pub fn closable(&self, cx: &App) -> bool {
        self.handle.as_ref().is_none_or(|h| h.closable(cx))
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        self.handle.as_ref().is_some_and(|h| h.is_dirty(cx))
    }

    pub fn focus_handle(&self, cx: &App) -> Option<FocusHandle> {
        self.handle.as_ref().and_then(|h| h.focus_handle(cx))
    }

    pub fn view(&self) -> Option<AnyView> {
        self.handle.as_ref().map(TabHandle::view)
    }
}
