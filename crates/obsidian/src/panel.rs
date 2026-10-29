//! The Explorer's vault section, under the files: the vault's folders and files as Obsidian's
//! file explorer lists them, listed folder by folder as they open (the explorer's tree model),
//! notes by their name. Its header opens and closes it (a header at the bottom while closed) and
//! switches among the vaults Obsidian lists; while open it resizes against the files, and the
//! vault on show is watched, so the tree and open notes follow edits made in Obsidian.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gitviber_core::obsidian::Vault;
use gitviber_core::{Core, api};
use gitviber_files::explorer::MIN_FILES;
use gitviber_files::explorer::tree::{LazyTree, Listed, Node, Row, TreeKey, tree_key};
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::FileIcon;
use gitviber_ui::workspace::{Panel, PanelContext, Slot, Toast, Workspace, WorkspaceEvent};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::resizable::HandleEdge;
use gpui_kit::component::{Icon, Sizable as _, resize_handle};
use gpui_kit::{
    App, AppContext as _, Axis, ClickEvent, ClipboardItem, Context, DragMoveEvent, Empty, Entity,
    FocusHandle, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Pixels,
    Render, ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, UniformListScrollHandle, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px, uniform_list,
};

use crate::links::note_name;
use crate::note::open_in_tab;
use crate::vaults::{Vaults, VaultsEvent};

const ROW_HEIGHT: f32 = 24.;
const INDENT: f32 = 12.;
/// The open section's least height (its header and two rows), and its height until dragged.
const MIN_SECTION: f32 = 84.;
const DEFAULT_SECTION: f32 = 240.;

/// As the file manager is named where the file opens.
const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    "Reveal in Finder"
} else if cfg!(windows) {
    "Show in Explorer"
} else {
    "Show in Folder"
};

type MenuAction = Rc<dyn Fn(&mut VaultSection, &mut Window, &mut Context<VaultSection>)>;

/// The divider over the open section, as it's dragged.
#[derive(Clone)]
struct VaultSplit;

impl Render for VaultSplit {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

pub struct VaultSection {
    workspace: WeakEntity<Workspace>,
    core: Arc<Core>,
    model: Entity<Vaults>,
    vault: Option<Vault>,
    /// The section is open (`Vaults::collapsed`, every window's): only then is the vault listed
    /// and watched.
    open: bool,
    /// The files' height above, as the explorer last drew them.
    room: Rc<Cell<Pixels>>,
    hold: Option<u64>,
    tree: LazyTree,
    rows: Vec<Row>,
    /// The vault's root couldn't be listed.
    error: Option<SharedString>,
    /// The keyboard's row, apart from the open tab.
    selected: Option<SharedString>,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    listings: HashMap<SharedString, Task<()>>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl Panel for VaultSection {
    /// None while it isn't drawn: the explorer's ↓ stays in the files.
    fn panel_focus(&self, cx: &App) -> Option<FocusHandle> {
        self.shown(cx).then(|| self.focus.clone())
    }
}

impl VaultSection {
    pub fn new(
        ctx: &PanelContext,
        room: Rc<Cell<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let model = Vaults::global(cx);
        let mut subs = vec![
            // Opened or closed (here or in another window), or resized.
            cx.observe(&model, |this, _, cx| this.sync(cx)),
            cx.subscribe(&model, |this, _, e: &VaultsEvent, cx| match e {
                VaultsEvent::ListChanged => this.sync(cx),
                VaultsEvent::Changed {
                    vault,
                    files,
                    paths,
                } => {
                    if this.vault.as_ref().is_some_and(|v| &v.path == vault)
                        && (*files || paths.is_empty())
                    {
                        this.refresh(cx);
                    }
                }
                VaultsEvent::IndexChanged(_) => {}
            }),
        ];
        if let Some(ws) = ctx.workspace.upgrade() {
            subs.push(cx.subscribe(&ws, |_, _, e: &WorkspaceEvent, cx| {
                if matches!(e, WorkspaceEvent::TabsChanged) {
                    cx.notify();
                }
            }));
        }
        // Back in the window: Obsidian may have opened or added a vault, and the vaults no
        // watcher covers may have changed.
        subs.push(cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.model.update(cx, |m, cx| m.refresh(cx));
            }
        }));
        cx.on_release(|this: &mut Self, cx| {
            this.keep_open_folders(cx);
            if let Some(id) = this.hold.take() {
                this.model.update(cx, |m, cx| m.release(id, cx));
            }
        })
        .detach();
        let focus = cx.focus_handle();
        // Come into from the files (↓ past their last row): the first row is the keyboard's.
        subs.push(cx.on_focus_in(&focus, window, |this, _, cx| {
            if this.selected.is_none()
                && let Some(first) = this.rows.first()
            {
                this.selected = Some(first.node.path.clone());
                cx.notify();
            }
        }));
        let mut panel = VaultSection {
            workspace: ctx.workspace.clone(),
            core: gitviber_ui::core(cx),
            model,
            vault: None,
            open: false,
            room,
            hold: None,
            tree: LazyTree::new(),
            rows: Vec::new(),
            error: None,
            selected: None,
            scroll: UniformListScrollHandle::new(),
            focus,
            listings: HashMap::new(),
            _subs: subs,
            _live: Live::new("VaultSection"),
        };
        panel.sync(cx);
        panel
    }

    /// Follows the vault to show and whether the section is open: another vault, or the section
    /// opening, starts the tree over with the folders it had open; closed, it holds nothing.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let want = model.current(cx);
        let open = !model.collapsed();
        if want.as_ref().map(|v| &v.path) == self.vault.as_ref().map(|v| &v.path)
            && open == self.open
        {
            self.vault = want;
            return cx.notify();
        }
        self.keep_open_folders(cx);
        if let Some(id) = self.hold.take() {
            self.model.update(cx, |m, cx| m.release(id, cx));
        }
        self.tree = LazyTree::new();
        self.rows = Vec::new();
        self.error = None;
        self.selected = None;
        self.listings.clear();
        if let Some(v) = want.as_ref().filter(|_| open) {
            self.hold = Some(self.model.update(cx, |m, cx| m.hold(&v.path, cx)));
            for dir in self.model.read(cx).open_folders(&v.path) {
                self.tree.set_open(&dir, true);
            }
        }
        self.vault = want;
        self.open = open;
        self.refresh(cx);
    }

    fn keep_open_folders(&self, cx: &mut App) {
        let Some(vault) = self.vault.as_ref().filter(|_| self.open) else {
            return;
        };
        let open: Vec<SharedString> = self
            .tree
            .expanded()
            .filter(|p| !p.is_empty())
            .cloned()
            .collect();
        self.model
            .update(cx, |m, _| m.keep_open_folders(&vault.path, open));
    }

    /// Drawn: Obsidian is on and lists a vault.
    pub fn shown(&self, cx: &App) -> bool {
        self.vault.is_some() && gitviber_ui::Settings::get(cx).obsidian
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens or closes the section, in every window.
    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        let collapsed = self.open;
        self.model
            .update(cx, |m, cx| m.set_collapsed(collapsed, cx));
    }

    /// Opens the section and moves the keyboard into it.
    pub fn open_and_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |m, cx| m.set_collapsed(false, cx));
        self.focus.focus(window, cx);
    }

    /// Back up into the files.
    fn focus_files(&self, window: &mut Window, cx: &mut Context<Self>) {
        _ = self
            .workspace
            .update(cx, |ws, cx| ws.focus_slot(Slot::Right, window, cx));
    }

    /// Lists every open folder again.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let open: Vec<SharedString> = self.tree.expanded().cloned().collect();
        for path in open {
            self.load(path, cx);
        }
        cx.notify();
    }

    fn load(&mut self, path: SharedString, cx: &mut Context<Self>) {
        let Some(vault) = self
            .vault
            .as_ref()
            .filter(|_| self.open)
            .map(|v| v.path.clone())
        else {
            return;
        };
        let id = self.tree.begin(&path);
        let (core, rel, key) = (self.core.clone(), path.to_string(), path.clone());
        let task = cx.spawn(async move |this, cx| {
            let listed = cx
                .background_spawn(async move {
                    api::obsidian::vault_list_dir(&core, vault, rel).map(|entries| {
                        entries
                            .into_iter()
                            .map(|e| Node {
                                name: e.name.into(),
                                path: e.path.into(),
                                is_dir: e.is_dir,
                                ignored: false,
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .await;
            _ = this.update(cx, |this, cx| {
                this.listings.remove(&key);
                match this.tree.listed(&key, id, listed) {
                    Listed::Changed | Listed::Closed => this.rebuild(cx),
                    Listed::RootFailed(e) => {
                        this.error = Some(e.into());
                        this.rebuild(cx);
                    }
                    Listed::Same | Listed::Stale => {}
                }
                if key.is_empty() && this.error.is_some() && this.tree.children("").is_some() {
                    this.error = None;
                }
            });
        });
        self.listings.insert(path, task);
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        self.rows = self.tree.rows();
        cx.notify();
    }

    fn row_index(&self, path: &str) -> Option<usize> {
        self.rows.iter().position(|r| r.node.path.as_ref() == path)
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.tree.collapse_all();
        self.keep_open_folders(cx);
        self.rebuild(cx);
    }

    fn set_open(&mut self, path: &str, open: bool, cx: &mut Context<Self>) {
        if self.tree.set_open(path, open) {
            self.load(path.to_string().into(), cx);
        }
        self.rebuild(cx);
    }

    fn activate(&mut self, node: &Node, pin: bool, window: &mut Window, cx: &mut Context<Self>) {
        if node.is_dir {
            let open = !self.tree.is_open(&node.path);
            return self.set_open(&node.path, open, cx);
        }
        let Some(vault) = self.vault.as_ref().map(|v| v.path.clone()) else {
            return;
        };
        open_in_tab(&self.workspace, &vault, &node.path, "", !pin, window, cx);
    }

    fn click(&mut self, ix: usize, e: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = self.rows.get(ix).map(|r| r.node.clone()) else {
            return;
        };
        self.focus.focus(window, cx);
        self.selected = Some(node.path.clone());
        self.activate(&node, e.click_count() >= 2, window, cx);
        cx.notify();
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &e.keystroke;
        // ⌥ and ⌘ arrows belong to the global shortcuts.
        if ks.modifiers.alt || ks.modifiers.platform || ks.modifiers.control {
            return;
        }
        let cur = self.selected.as_ref().and_then(|s| self.row_index(s));
        // ↑ from the header or the first row goes back up into the files.
        if ks.key == "up" && (!self.open || cur.is_none_or(|c| c == 0)) {
            cx.stop_propagation();
            return self.focus_files(window, cx);
        }
        if !self.open {
            if matches!(ks.key.as_str(), "enter" | "space" | "right" | "down") {
                cx.stop_propagation();
                self.toggle(cx);
            }
            return;
        }
        let tree = &self.tree;
        let Some(k) = tree_key(&ks.key, &self.rows, cur, |p| tree.is_open(p)) else {
            return;
        };
        cx.stop_propagation();
        match k {
            TreeKey::Move(to) => {
                if self.rows.is_empty() {
                    return;
                }
                let to = to.clamp(0, self.rows.len() as isize - 1) as usize;
                self.selected = Some(self.rows[to].node.path.clone());
                self.scroll.scroll_to_item(to, ScrollStrategy::Nearest);
                cx.notify();
            }
            TreeKey::Open(path) => self.set_open(&path, true, cx),
            TreeKey::Close(path) => self.set_open(&path, false, cx),
            TreeKey::Select(path) => {
                self.selected = Some(path);
                cx.notify();
            }
            TreeKey::Activate {
                node,
                pin,
                focus_code,
            } => {
                self.activate(&node, pin, window, cx);
                if focus_code {
                    _ = self
                        .workspace
                        .update(cx, |ws, cx| ws.focus_center(window, cx));
                }
            }
        }
    }

    fn toast(&self, toast: Toast, cx: &mut App) {
        _ = self.workspace.update(cx, |ws, cx| ws.toast(toast, cx));
    }

    fn open_in_obsidian(&self, path: String, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.as_ref().map(|v| v.path.clone()) else {
            return;
        };
        let (core, ws) = (self.core.clone(), self.workspace.clone());
        cx.spawn(async move |_, cx| {
            let opened = cx
                .background_spawn(async move {
                    api::obsidian::vault_open_in_obsidian(&core, vault, path)
                })
                .await;
            if let Err(e) = opened {
                _ = ws.update(cx, |ws, cx| {
                    ws.toast(Toast::error(e).title("Could not open Obsidian"), cx)
                });
            }
        })
        .detach();
    }

    fn reveal(&self, path: String, cx: &mut Context<Self>) {
        let Some(vault) = self.vault.as_ref().map(|v| v.path.clone()) else {
            return;
        };
        let core = self.core.clone();
        cx.background_spawn(async move {
            _ = api::obsidian::vault_reveal(&core, vault, path);
        })
        .detach();
    }

    fn copy(&self, text: String, what: &str, cx: &mut App) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.toast(Toast::success(what.to_string()), cx);
    }

    fn menu(&mut self, node: Option<Node>, menu: PopupMenu, cx: &mut Context<Self>) -> PopupMenu {
        let Some(vault) = self.vault.as_ref().map(|v| v.path.clone()) else {
            return menu;
        };
        if let Some(n) = &node {
            self.selected = Some(n.path.clone());
        }
        let me = cx.weak_entity();
        let item = |label: &str, f: MenuAction| {
            let me = me.clone();
            PopupMenuItem::new(label.to_string()).on_click(move |_, window, cx| {
                let f = f.clone();
                _ = me.update(cx, |this, cx| f(this, window, cx));
            })
        };
        let mut menu = menu;
        if let Some(n) = node.clone().filter(|n| !n.is_dir) {
            let (open, obsidian) = (n.clone(), n.path.to_string());
            menu = menu
                .item(item(
                    "Open",
                    Rc::new(move |this, w, cx| this.activate(&open, true, w, cx)),
                ))
                .item(item(
                    "Open in Obsidian",
                    Rc::new(move |this, _, cx| this.open_in_obsidian(obsidian.clone(), cx)),
                ))
                .separator();
        }
        let rel = node
            .as_ref()
            .map(|n| n.path.to_string())
            .unwrap_or_default();
        let (r1, r2) = (rel.clone(), rel.clone());
        let abs = if rel.is_empty() {
            vault.clone()
        } else {
            format!("{vault}/{rel}")
        };
        menu = menu
            .item(item(
                REVEAL_LABEL,
                Rc::new(move |this, _, cx| this.reveal(r1.clone(), cx)),
            ))
            .item(item(
                "Copy Path",
                Rc::new(move |this, _, cx| this.copy(abs.clone(), "Path copied", cx)),
            ));
        if node.is_some() {
            menu = menu.item(item(
                "Copy Relative Path",
                Rc::new(move |this, _, cx| this.copy(r2.clone(), "Relative path copied", cx)),
            ));
        }
        menu
    }

    fn render_row(
        &mut self,
        ix: usize,
        active: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let t = cx.app_theme();
        let Some(row) = self.rows.get(ix).cloned() else {
            return div().into_any_element();
        };
        let node = row.node;
        let open = node.is_dir && self.tree.is_open(&node.path);
        let is_active = !node.is_dir && active == Some(node.path.as_ref());
        let selected = self.selected.as_ref() == Some(&node.path);
        let focused = self.focus.contains_focused(window, cx);
        let name: SharedString = if node.is_dir {
            node.name.clone()
        } else {
            note_name(&node.path).to_string().into()
        };
        let menu_node = node.clone();
        let me = cx.weak_entity();
        div()
            .id(("vault-row", ix))
            .relative()
            .h(px(ROW_HEIGHT))
            .flex()
            .items_center()
            .gap_1()
            .pl(px(8. + row.depth as f32 * INDENT))
            .pr_2()
            .text_size(px(12.5))
            .cursor_pointer()
            .when(is_active, |d| d.bg(t.primary.opacity(0.15)))
            .when(!is_active, |d| d.hover(|s| s.bg(t.hover)))
            .when(selected && focused, |d| {
                d.bg(t.hover)
                    .border_1()
                    .border_color(t.primary.opacity(0.7))
            })
            .when(is_active, |d| {
                d.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(2.))
                        .bg(t.primary),
                )
            })
            .child(if node.is_dir {
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(px(12.))
                .text_color(t.subtle)
                .into_any_element()
            } else {
                div().w(px(12.)).flex_none().into_any_element()
            })
            .child(div().flex_none().child(if node.is_dir {
                FileIcon::folder(&node.name, open)
            } else {
                FileIcon::file(&node.path)
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(t.foreground.opacity(0.85))
                    .child(name),
            )
            .on_click(
                cx.listener(move |this, e: &ClickEvent, window, cx| this.click(ix, e, window, cx)),
            )
            .context_menu(move |menu, _, cx| match me.upgrade() {
                Some(this) => {
                    this.update(cx, |this, cx| this.menu(Some(menu_node.clone()), menu, cx))
                }
                None => menu,
            })
            .into_any_element()
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = cx.app_theme();
        let open = self.open;
        let vaults = self.model.read(cx).vaults(cx);
        let current = self.vault.clone();
        let me = cx.weak_entity();
        let name: SharedString = current
            .as_ref()
            .map(|v| v.name.clone())
            .unwrap_or_default()
            .into();
        let switcher = if vaults.len() > 1 {
            let model = self.model.downgrade();
            let picked = current.as_ref().map(|v| v.path.clone());
            Button::new("vault-switch")
                .ghost()
                .xsmall()
                .label(name)
                .icon(IconName::ChevronDown)
                .tooltip("Switch vault")
                .dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    for v in &vaults {
                        let (model, path) = (model.clone(), v.path.clone());
                        menu = menu.item(
                            PopupMenuItem::new(v.name.clone())
                                .checked(picked.as_ref() == Some(&v.path))
                                .on_click(move |_, _, cx| {
                                    _ = model.update(cx, |m, cx| m.pick(&path, cx));
                                }),
                        );
                    }
                    menu
                })
                .into_any_element()
        } else {
            div()
                .px_1()
                .min_w_0()
                .truncate()
                .text_size(px(11.))
                .text_color(t.subtle)
                .child(name)
                .into_any_element()
        };
        let toggle = div()
            .id("vault-toggle")
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .cursor_pointer()
            .text_size(px(11.))
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .text_color(t.subtle)
            .hover(|s| s.text_color(t.foreground))
            .child(
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(px(12.)),
            )
            .child("OBSIDIAN")
            .on_click(cx.listener(|this, _, _, cx| this.toggle(cx)));
        div()
            .flex_none()
            .h(px(28.))
            .flex()
            .items_center()
            .gap_1()
            .pl_2()
            .pr_1()
            .border_color(t.border)
            .when(open, |d| d.border_b_1())
            .when(!open, |d| d.border_t_1())
            .child(toggle)
            .child(div().min_w_0().child(switcher))
            .child(div().flex_1())
            .when(open, |d| {
                d.child(
                    Button::new("collapse-folders")
                        .ghost()
                        .xsmall()
                        .icon(IconName::ChevronsDownUp)
                        .tooltip("Collapse folders")
                        .on_click(move |_, _, cx| {
                            _ = me.update(cx, |this, cx| this.collapse_all(cx));
                        }),
                )
            })
    }

    #[cfg(test)]
    pub(crate) fn open_folder(&mut self, path: &str, cx: &mut Context<Self>) {
        self.set_open(path, true, cx);
    }

    #[cfg(test)]
    pub(crate) fn row_paths(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.node.path.to_string()).collect()
    }
}

fn notice(title: &str, detail: &str, cx: &App) -> impl IntoElement + use<> {
    let t = cx.app_theme();
    div()
        .px_4()
        .py_6()
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .text_center()
        .child(
            div()
                .text_size(px(12.))
                .text_color(t.muted_foreground)
                .child(title.to_string()),
        )
        .child(
            div()
                .text_size(px(11.5))
                .text_color(t.subtle)
                .child(detail.to_string()),
        )
}

impl Render for VaultSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        let base = div()
            .id("vault-section")
            .key_context("Explorer")
            .track_focus(&self.focus)
            .w_full()
            .flex()
            .flex_col()
            .text_color(t.foreground)
            .on_key_down(
                cx.listener(|this, e: &KeyDownEvent, window, cx| this.on_key(e, window, cx)),
            );
        // Nothing to show: the files have the whole explorer.
        let Some(vault) = self.vault.clone().filter(|_| self.shown(cx)) else {
            return base.flex_none();
        };
        if !self.open {
            return base.flex_none().child(self.header(cx));
        }
        // The open tab's file, when it's one of this vault's (selection_key's "vault:" form).
        let prefix = format!("vault:{}:", vault.path);
        let active: Option<SharedString> = self.workspace.upgrade().and_then(|ws| {
            let key = ws.read(cx).tabs().active_key()?;
            key.strip_prefix(&prefix)
                .map(|p| SharedString::from(p.to_string()))
        });
        let empty = self.tree.children("").is_some_and(|c| c.is_empty());
        let count = self.rows.len();
        let me = cx.weak_entity();
        let size = self.model.read(cx).size().unwrap_or(DEFAULT_SECTION);
        let handle = resize_handle::<VaultSplit, VaultSplit>("vault-split", Axis::Vertical)
            .inside(HandleEdge::Leading)
            .on_drag(VaultSplit, |d, _, _, cx| cx.new(|_| d.clone()));
        // Too short a window shrinks it first down to MIN_SECTION, then the files.
        base.relative()
            .h(px(size))
            .min_h(px(MIN_SECTION))
            .flex_shrink(1.)
            .on_drag_move(cx.listener(|this, e: &DragMoveEvent<VaultSplit>, _, cx| {
                let want = f32::from(e.bounds.bottom() - e.event.position.y);
                let most = f32::from(e.bounds.size.height + this.room.get()) - MIN_FILES;
                let size = want.min(most).max(MIN_SECTION).round();
                this.model.update(cx, |m, cx| m.set_size(size, cx));
            }))
            .child(self.header(cx))
            .when_some(self.error.clone(), |d, e| {
                d.child(notice(&format!("Could not list {}", vault.name), &e, cx))
            })
            .when(empty, |d| d.child(notice("This vault is empty", "", cx)))
            .child(
                uniform_list(
                    "vault-rows",
                    count,
                    cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                        range
                            .map(|ix| this.render_row(ix, active.as_deref(), window, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .flex_1()
                .py_1(),
            )
            // The space below the rows: the vault's own menu.
            .child(
                div()
                    .id("vault-blank")
                    .h(px(ROW_HEIGHT))
                    .flex_none()
                    .context_menu(move |menu, _, cx| match me.upgrade() {
                        Some(this) => this.update(cx, |this, cx| this.menu(None, menu, cx)),
                        None => menu,
                    }),
            )
            .child(handle)
    }
}
