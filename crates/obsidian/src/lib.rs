//! GitViber's Obsidian vault, beside the code: a section under the Explorer's files with the
//! vault's files, and the tabs of the "vault" selection: a note as Obsidian's reading
//! view shows it (wikilinks, embeds, callouts, highlights, tags, properties) or as its source
//! to edit and save, a canvas drawn natively, media in the media view. Vaults are the ones
//! Obsidian lists (Settings → Obsidian picks which show); the one on show is watched. Call
//! [`init`] once at startup, after gitviber-files'.

// The GPUI derive macros name `gpui::…`; the Kit answers to that name.
extern crate gpui_kit as gpui;

pub mod canvas;
mod canvas_view;
mod editor;
mod edits;
pub mod links;
pub mod markup;
mod nfd;
mod note;
pub mod panel;
mod svg;
pub mod vaults;
mod view;

use std::rc::Rc;

use gitviber_files::ExplorerPanel;
use gitviber_files::explorer::{self, set_section};
use gitviber_ui::workspace::{PanelHandle, Selection, TabHandle, Workspace, register_opener};
use gpui_kit::{App, AppContext as _, Entity, WeakEntity, Window};

pub use canvas_view::CanvasView;
pub use editor::NoteEditor;
pub use note::NoteView;
pub use panel::VaultSection;
pub use vaults::{Vaults, VaultsEvent};
pub use view::{Mode, VaultTab};

/// Puts the vault section under the Explorer's files and registers the opener of "vault" tabs;
/// on the first launch, takes the old app's picked vault, closed section and unsaved note typing.
pub fn init(cx: &mut App) {
    vaults::import_legacy();
    edits::import_legacy();
    set_section(
        cx,
        Rc::new(|ctx, room, window, cx| {
            PanelHandle::new(cx.new(|cx| VaultSection::new(ctx, room, window, cx)))
        }),
    );
    register_opener(cx, "vault", open_selection);
}

/// Shows the vault's tree: the Explorer shown, its vault section opened and focused. False when Obsidian lists no vault to show (or is turned off).
pub fn show_vault(workspace: &Entity<Workspace>, window: &mut Window, cx: &mut App) -> bool {
    workspace.update(cx, |ws, cx| ws.show_panel(explorer::PANEL_ID, cx));
    // Built outside the workspace's update: a panel may read the workspace as it's made.
    let section = Workspace::ensure_panel(workspace, explorer::PANEL_ID, window, cx)
        .and_then(|p| p.view().downcast::<ExplorerPanel>().ok())
        .and_then(|e| e.read(cx).section())
        .and_then(|s| s.view().downcast::<VaultSection>().ok());
    let Some(section) = section.filter(|s| s.read(cx).shown(cx)) else {
        return false;
    };
    section.update(cx, |s, cx| s.open_and_focus(window, cx));
    true
}

fn open_selection(selection: &Selection, window: &mut Window, cx: &mut App) -> Option<TabHandle> {
    let Selection::Vault { vault, path } = selection else {
        return None;
    };
    // The workspace is being updated (it called this): only its handle is taken here.
    let ws = Workspace::for_window(window, cx)?.downgrade();
    let (vault, path) = (vault.clone(), path.clone());
    Some(TabHandle::new(
        cx.new(|cx| VaultTab::new(ws, vault, path, window, cx)),
    ))
}

/// Opens (or shows) the tab of a file in a vault (its folder), scrolled to `anchor` (a heading
/// or ^block) when there's one. `preview`: a single click's tab, which the next preview replaces.
pub fn open_vault_file(
    workspace: &WeakEntity<Workspace>,
    vault: &str,
    path: &str,
    anchor: &str,
    preview: bool,
    window: &mut Window,
    cx: &mut App,
) {
    note::open_in_tab(workspace, vault, path, anchor, preview, window, cx);
}

#[cfg(test)]
mod tests;
