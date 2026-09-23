//! The menu bar. Its items run the page's commands (src/lib/commands.ts): a click emits
//! "menu" with the item's id, and the page reports back each item's shortcut and whether it
//! applies right now (src/lib/menu.ts), so the menu follows the user's key bindings.

use std::collections::HashMap;
use std::sync::Mutex;
use tauri::menu::{
    CheckMenuItem, IsMenuItem, Menu, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu,
};
use tauri::{AppHandle, Manager, Wry};

/// The items the page updates, by id.
pub struct Handles {
    items: HashMap<String, MenuItemKind<Wry>>,
    recent: Submenu<Wry>,
    recent_shown: Mutex<Vec<Recent>>,
}

#[derive(serde::Deserialize)]
pub struct ItemState {
    enabled: bool,
    /// A chord as commands.ts writes it ("shift+cmd+e"); one muda can't parse shows none.
    accelerator: Option<String>,
    checked: Option<bool>,
}

#[derive(serde::Deserialize, PartialEq, Clone)]
pub struct Recent {
    path: String,
    title: String,
}

struct Builder<'a> {
    app: &'a AppHandle,
    items: HashMap<String, MenuItemKind<Wry>>,
}

impl Builder<'_> {
    /// Disabled until the page says the command applies.
    fn command(&mut self, id: &str, text: &str) -> tauri::Result<MenuItem<Wry>> {
        let item = MenuItem::with_id(self.app, id, text, false, None::<&str>)?;
        self.items
            .insert(id.into(), MenuItemKind::MenuItem(item.clone()));
        Ok(item)
    }

    fn check(&mut self, id: &str, text: &str) -> tauri::Result<CheckMenuItem<Wry>> {
        let item = CheckMenuItem::with_id(self.app, id, text, false, false, None::<&str>)?;
        self.items
            .insert(id.into(), MenuItemKind::Check(item.clone()));
        Ok(item)
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let mut b = Builder {
        app,
        items: HashMap::new(),
    };
    let native =
        |f: fn(&AppHandle, Option<&str>) -> tauri::Result<PredefinedMenuItem<Wry>>| f(app, None);
    let sep = || PredefinedMenuItem::separator(app);
    let submenu =
        |text: &str, items: &[&dyn IsMenuItem<Wry>]| Submenu::with_items(app, text, true, items);

    let app_menu = submenu(
        "GitViber",
        &[
            // Opens the app's own About window: a native panel can't hold links or buttons.
            &b.command("app.about", "About GitViber")?,
            &sep()?,
            &b.command("workbench.openSettings", "Settings…")?,
            &sep()?,
            &native(PredefinedMenuItem::services)?,
            &sep()?,
            // Named here: the default takes the binary's name, "gitviber".
            &PredefinedMenuItem::hide(app, Some("Hide GitViber"))?,
            &native(PredefinedMenuItem::hide_others)?,
            &native(PredefinedMenuItem::show_all)?,
            &sep()?,
            &PredefinedMenuItem::quit(app, Some("Quit GitViber"))?,
        ],
    )?;

    let recent = submenu("Open Recent", &[])?;
    fill_recent(app, &recent, &[])?;
    let file = submenu(
        "File",
        &[
            &b.command("file.openRepo", "Open Repository…")?,
            &recent,
            &sep()?,
            &b.command("terminal.new", "New Terminal")?,
            &sep()?,
            &b.command("tab.close", "Close Tab")?,
            &sep()?,
            &b.command("file.reveal", "Reveal in Finder")?,
        ],
    )?;

    // The native items, so text fields keep their own undo, clipboard and selection.
    let edit = submenu(
        "Edit",
        &[
            &native(PredefinedMenuItem::undo)?,
            &native(PredefinedMenuItem::redo)?,
            &sep()?,
            &native(PredefinedMenuItem::cut)?,
            &native(PredefinedMenuItem::copy)?,
            &native(PredefinedMenuItem::paste)?,
            &native(PredefinedMenuItem::select_all)?,
        ],
    )?;

    let view = submenu(
        "View",
        &[
            &b.command("view.changes", "Changes")?,
            &b.command("view.history", "History")?,
            &b.command("view.pulls", "Pull Requests")?,
            &b.command("view.issues", "Issues")?,
            &b.command("history.search", "Search History")?,
            &sep()?,
            &b.command("view.toggleGitPanel", "Toggle Git Panel")?,
            &b.command("view.toggleExplorer", "Toggle Explorer")?,
            &b.command("view.showExplorer", "Show Explorer")?,
            &b.command("terminal.toggle", "Toggle Terminal")?,
            &sep()?,
            &b.check("diff.toggleSplit", "Split Diff")?,
            &b.check("diff.toggleCollapse", "Collapse Unchanged Lines")?,
            &b.check("editor.toggleWrap", "Word Wrap")?,
            &b.check("editor.toggleBlame", "Blame")?,
            &sep()?,
            &b.command("view.zoomIn", "Zoom In")?,
            &b.command("view.zoomOut", "Zoom Out")?,
            &b.command("view.zoomReset", "Actual Size")?,
            &b.command("editor.fontZoomIn", "Increase Code Font Size")?,
            &b.command("editor.fontZoomOut", "Decrease Code Font Size")?,
            &b.command("editor.fontZoomReset", "Reset Code Font Size")?,
            &sep()?,
            &b.command("repo.refresh", "Refresh")?,
            &b.command("window.reload", "Reload Window")?,
            &sep()?,
            &native(PredefinedMenuItem::fullscreen)?,
        ],
    )?;

    let go = submenu(
        "Go",
        &[
            &b.command("review.nextFile", "Next Changed File")?,
            &b.command("review.prevFile", "Previous Changed File")?,
            &b.command("review.toggleViewed", "Toggle File Viewed")?,
            &sep()?,
            &b.command("diff.nextChange", "Next Change")?,
            &b.command("diff.prevChange", "Previous Change")?,
        ],
    )?;

    let git = submenu(
        "Git",
        &[
            &b.command("git.commit", "Commit")?,
            &b.command("git.toggleStage", "Stage / Unstage Changes")?,
            &b.command("git.discard", "Discard Changes")?,
            &sep()?,
            &b.command("git.fetch", "Fetch")?,
            &b.command("git.pull", "Pull")?,
            &b.command("git.push", "Push")?,
            &sep()?,
            &b.command("git.undo", "Undo Git Action")?,
            &b.command("git.redo", "Redo Git Action")?,
        ],
    )?;

    let window = submenu(
        "Window",
        &[
            &native(PredefinedMenuItem::minimize)?,
            &native(PredefinedMenuItem::maximize)?,
            &sep()?,
            &native(PredefinedMenuItem::bring_all_to_front)?,
        ],
    )?;

    let help = submenu(
        "Help",
        &[
            &b.command("help.readme", "GitViber Help")?,
            &b.command("help.shortcuts", "Keyboard Shortcuts")?,
            &sep()?,
            &b.command("help.reportBug", "Report a Bug…")?,
            &b.command("help.releaseNotes", "Release Notes")?,
            &b.command("help.license", "View License")?,
        ],
    )?;

    // Window lists the open windows; Help gets macOS's search box, which finds menu items.
    #[cfg(target_os = "macos")]
    {
        window.set_as_windows_menu_for_nsapp()?;
        help.set_as_help_menu_for_nsapp()?;
    }

    let menu = Menu::with_items(
        app,
        &[&app_menu, &file, &edit, &view, &go, &git, &window, &help],
    )?;
    app.manage(Handles {
        items: b.items,
        recent,
        recent_shown: Mutex::new(Vec::new()),
    });
    Ok(menu)
}

/// Applies what the page reports; `recent` is None when the list hasn't changed.
pub fn update(
    app: &AppHandle,
    handles: &Handles,
    items: HashMap<String, ItemState>,
    recent: Option<Vec<Recent>>,
) -> tauri::Result<()> {
    for (id, state) in items {
        match handles.items.get(&id) {
            Some(MenuItemKind::MenuItem(item)) => {
                item.set_enabled(state.enabled)?;
                item.set_accelerator(state.accelerator)?;
            }
            Some(MenuItemKind::Check(item)) => {
                item.set_enabled(state.enabled)?;
                item.set_accelerator(state.accelerator)?;
                if let Some(checked) = state.checked {
                    item.set_checked(checked)?;
                }
            }
            _ => {}
        }
    }
    if let Some(list) = recent {
        let mut shown = handles.recent_shown.lock().unwrap();
        if *shown != list {
            fill_recent(app, &handles.recent, &list)?;
            *shown = list;
        }
    }
    Ok(())
}

/// macOS moves a key equivalent to the key that types it on a US layout, so on Turkish Q
/// ⌘, showed as ⌘Ö. The page matches the character typed; the menu must show that one.
#[cfg(target_os = "macos")]
pub fn keep_typed_key_equivalents() {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject, Bool};

    unsafe fn walk(menu: *mut AnyObject) {
        if menu.is_null() {
            return;
        }
        let items: *mut AnyObject = msg_send![menu, itemArray];
        let count: usize = msg_send![items, count];
        for i in 0..count {
            let item: *mut AnyObject = msg_send![items, objectAtIndex: i];
            let _: () = msg_send![item, setAllowsAutomaticKeyEquivalentLocalization: Bool::NO];
            walk(msg_send![item, submenu]);
        }
    }
    let Some(class) = AnyClass::get(c"NSApplication") else {
        return;
    };
    unsafe {
        let app: *mut AnyObject = msg_send![class, sharedApplication];
        walk(msg_send![app, mainMenu]);
    }
}

fn fill_recent(app: &AppHandle, menu: &Submenu<Wry>, list: &[Recent]) -> tauri::Result<()> {
    while menu.remove_at(0)?.is_some() {}
    for r in list {
        let id = format!("recent:{}", r.path);
        menu.append(&MenuItem::with_id(app, id, &r.title, true, None::<&str>)?)?;
    }
    if !list.is_empty() {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    let clear = MenuItem::with_id(
        app,
        "recent.clear",
        "Clear Menu",
        !list.is_empty(),
        None::<&str>,
    )?;
    menu.append(&clear)
}
