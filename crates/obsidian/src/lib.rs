//! GitViber's Obsidian vault, beside the code: the Obsidian panel (right sidebar, id "vault")
//! with the vault's files, and the tabs of the "vault" selection: a note as Obsidian's reading
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

use gitviber_ui::workspace::{
    PanelSpec, Selection, Slot, TabHandle, Workspace, register_opener, register_panel,
};
use gpui_kit::{App, AppContext as _, WeakEntity, Window};

pub use canvas_view::CanvasView;
pub use editor::NoteEditor;
pub use note::NoteView;
pub use panel::{PANEL_ID, VaultPanel};
pub use vaults::{Vaults, VaultsEvent};
pub use view::{Mode, VaultTab};

/// Registers the Obsidian panel (after Explorer and Search) and the opener of "vault" tabs; on
/// the first launch, takes the old app's picked vault and unsaved note typing.
pub fn init(cx: &mut App) {
    vaults::import_legacy();
    edits::import_legacy();
    register_panel(
        cx,
        PanelSpec::new(PANEL_ID, Slot::Right, "Obsidian", |ctx, window, cx| {
            cx.new(|cx| VaultPanel::new(ctx, window, cx))
        })
        .order(2)
        .per_window(),
    );
    register_opener(cx, "vault", open_selection);
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
