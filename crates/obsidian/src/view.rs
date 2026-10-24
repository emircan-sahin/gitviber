//! A vault file in a tab: a note rendered (or its source, typed into and saved), a canvas drawn,
//! an SVG drawn, media in the media view, anything else as code. The file is read again when
//! the vault's watcher says it changed.

use std::path::PathBuf;
use std::sync::Arc;

use gitviber_core::git::FileText;
use gitviber_core::{Core, api, obsidian};
use gitviber_files::MediaView;
use gitviber_files::media::{MediaKind, media_kind};
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::file;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{TabIcon, TabItem, Toast, Workspace, save_signal};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, WeakEntity, Window,
    div, prelude::FluentBuilder as _, px,
};

use crate::canvas_view::CanvasView;
use crate::editor::{EditorEvent, NoteEditor};
use crate::edits;
use crate::note::NoteView;
use crate::vaults::{Vaults, VaultsEvent, hold_for};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Note,
    Canvas,
    Svg,
    Media(MediaKind),
    Text,
}

impl Kind {
    fn of(path: &str) -> Kind {
        let lower = path.to_ascii_lowercase();
        if gitviber_markdown::is_markdown(&lower) {
            Kind::Note
        } else if lower.ends_with(".canvas") {
            Kind::Canvas
        } else if lower.ends_with(".svg") {
            Kind::Svg
        } else {
            media_kind(&lower).map_or(Kind::Text, Kind::Media)
        }
    }

    /// Shown rendered, or as its source (Code / Preview).
    fn switchable(self) -> bool {
        matches!(self, Kind::Note | Kind::Canvas | Kind::Svg)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Code,
    Preview,
}

enum Content {
    Loading,
    Notice {
        title: SharedString,
        detail: SharedString,
    },
    Note(Entity<NoteView>),
    Canvas(Entity<CanvasView>),
    Media(Entity<MediaView>),
    Code,
}

/// What reading the file found.
enum Loaded {
    Text(FileText),
    /// Media is read by its view, from its place on disk.
    Media(PathBuf),
    Failed(String),
}

pub struct VaultTab {
    workspace: WeakEntity<Workspace>,
    core: Arc<Core>,
    vault: String,
    path: String,
    kind: Kind,
    mode: Mode,
    content: Content,
    /// The code view: while in Code, and while it has typing to keep.
    editor: Option<(Entity<NoteEditor>, Subscription)>,
    /// The last read wasn't the file's own bytes: a code view of it is never written back.
    lossy: bool,
    /// A #heading or #^block to show once the note is drawn.
    pending: Option<String>,
    focus: FocusHandle,
    _load: Task<()>,
    _subs: Vec<Subscription>,
    _live: Live,
}

impl VaultTab {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        vault: String,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let kind = Kind::of(&path);
        let s = gitviber_ui::Settings::get(cx);
        let mode = match kind {
            Kind::Note if !s.markdown_preview => Mode::Code,
            Kind::Svg if !s.svg_preview => Mode::Code,
            Kind::Note | Kind::Canvas | Kind::Svg | Kind::Media(_) => Mode::Preview,
            Kind::Text => Mode::Code,
        };
        hold_for(&vault, cx);
        let model = Vaults::global(cx);
        let sub = cx.subscribe_in(&model, window, |this, _, e: &VaultsEvent, window, cx| {
            if e.touches(&this.vault, &this.path) {
                this.load(window, cx);
            }
        });
        let mut this = VaultTab {
            workspace,
            core: gitviber_ui::core(cx),
            vault,
            path,
            kind,
            mode,
            content: Content::Loading,
            editor: None,
            lossy: false,
            pending: None,
            focus: cx.focus_handle(),
            _load: Task::ready(()),
            _subs: vec![sub],
            _live: Live::new("VaultTab"),
        };
        this.load(window, cx);
        this
    }

    pub fn vault(&self) -> &str {
        &self.vault
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Shows the part of the note a #heading or #^block names.
    pub fn reveal(&mut self, anchor: &str, cx: &mut Context<Self>) {
        match &self.content {
            Content::Note(note) => note.update(cx, |n, cx| n.reveal(anchor, cx)),
            _ => self.pending = Some(anchor.to_string()),
        }
    }

    fn read(core: Arc<Core>, vault: String, path: String, kind: Kind) -> Loaded {
        if let Kind::Media(_) = kind {
            let abs = obsidian::root(&vault).and_then(|root| obsidian::resolve(&root, &path));
            return match abs {
                Ok(p) if p.is_file() => Loaded::Media(p),
                Ok(_) => Loaded::Text(FileText::default()),
                Err(e) => Loaded::Failed(e),
            };
        }
        match api::obsidian::vault_read_file(&core, vault, path) {
            Ok(t) => Loaded::Text(t),
            Err(e) => Loaded::Failed(e),
        }
    }

    /// Reads the file (again) off the main thread.
    pub fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (core, vault, path, kind) = (
            self.core.clone(),
            self.vault.clone(),
            self.path.clone(),
            self.kind,
        );
        self._load = cx.spawn_in(window, async move |this, cx| {
            let loaded = cx
                .background_spawn(async move { Self::read(core, vault, path, kind) })
                .await;
            _ = this.update_in(cx, |this, window, cx| this.show(loaded, window, cx));
        });
    }

    fn notice(&mut self, title: &str, detail: &str, cx: &mut Context<Self>) {
        self.content = Content::Notice {
            title: title.to_string().into(),
            detail: detail.to_string().into(),
        };
        cx.notify();
    }

    fn show(&mut self, loaded: Loaded, window: &mut Window, cx: &mut Context<Self>) {
        let file = match loaded {
            Loaded::Failed(e) => return self.notice("Could not read the file", &e, cx),
            Loaded::Media(abs) => {
                let Kind::Media(kind) = self.kind else { return };
                self.content = Content::Media(cx.new(|cx| MediaView::new(abs, kind, window, cx)));
                return cx.notify();
            }
            Loaded::Text(file) => file,
        };
        if !file.exists {
            return self.notice(
                "This file is not in the vault",
                "It was deleted or moved since it opened.",
                cx,
            );
        }
        if file.too_large {
            return self.notice(
                "This file is too large to show",
                "Files over 8 MB open in another app.",
                cx,
            );
        }
        if file.binary {
            return self.notice("Binary file", "It has no text to show.", cx);
        }
        self.lossy = file.lossy;
        if let Some((editor, _)) = &self.editor {
            editor.update(cx, |e, cx| {
                e.file_changed(&file.text, file.lossy, window, cx)
            });
        } else if !file.lossy && edits::get(&self.vault, &self.path, cx).is_some() {
            // Typing kept from before: its code view comes back with it, so the tab is dirty
            // and the rendered view shows it.
            self.ensure_editor(file.text.clone(), window, cx);
        }
        // Typing not saved yet is what the rendered view shows.
        let typed = self
            .editor
            .as_ref()
            .filter(|(e, _)| e.read(cx).is_dirty())
            .map(|(e, _)| e.read(cx).content(cx));
        match self.mode {
            Mode::Code => self.show_code(file.text, window, cx),
            Mode::Preview => self.show_preview(typed.unwrap_or(file.text), window, cx),
        }
        cx.notify();
    }

    fn show_code(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_editor(text, window, cx);
        self.content = Content::Code;
    }

    /// The code view, made from `text` (the file as last read) if there's none; read-only when
    /// that read was lossy, whichever way the tab got to Code.
    fn ensure_editor(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.is_some() {
            return;
        }
        let (ws, vault, path, lossy) = (
            self.workspace.clone(),
            self.vault.clone(),
            self.path.clone(),
            self.lossy,
        );
        let editor = cx.new(|cx| NoteEditor::new(ws, vault, path, text, lossy, window, cx));
        let sub = cx.subscribe(&editor, |this, _, e: &EditorEvent, cx| match e {
            EditorEvent::DirtyChanged => cx.notify(),
            // A vault no watcher covers wouldn't be read again.
            EditorEvent::Saved(text) => this.text_saved(text, cx),
        });
        self.editor = Some((editor, sub));
    }

    fn show_preview(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        match (&self.content, self.kind) {
            (Content::Note(note), _) => note.update(cx, |n, cx| n.set_source(text, cx)),
            (Content::Canvas(canvas), _) => canvas.update(cx, |c, cx| c.set_source(text, cx)),
            (Content::Media(_), Kind::Svg) => {}
            (_, Kind::Note) => {
                let (ws, vault, path) = (
                    self.workspace.clone(),
                    self.vault.clone(),
                    self.path.clone(),
                );
                let note = cx.new(|cx| NoteView::new(ws, vault, path, text, window, cx));
                if let Some(anchor) = self.pending.take() {
                    note.update(cx, |n, cx| n.reveal(&anchor, cx));
                }
                self.content = Content::Note(note);
            }
            (_, Kind::Canvas) => {
                let (ws, vault, path) = (
                    self.workspace.clone(),
                    self.vault.clone(),
                    self.path.clone(),
                );
                self.content = Content::Canvas(
                    cx.new(|cx| CanvasView::new(ws, vault, path, text, window, cx)),
                );
            }
            (_, Kind::Svg) => {
                let abs = PathBuf::from(&self.vault).join(&self.path);
                self.content =
                    Content::Media(cx.new(|cx| MediaView::new(abs, MediaKind::Image, window, cx)));
            }
            _ => self.show_code(text, window, cx),
        }
    }

    fn text_saved(&mut self, text: &Arc<str>, cx: &mut Context<Self>) {
        match &self.content {
            Content::Note(note) => note.update(cx, |n, cx| n.set_source(text.to_string(), cx)),
            Content::Canvas(canvas) => {
                canvas.update(cx, |c, cx| c.set_source(text.to_string(), cx))
            }
            _ => {}
        }
        cx.notify();
    }

    /// The text as shown now: the code view's (typing included), else the rendered view's.
    fn current_text(&self, cx: &App) -> Option<String> {
        if let Some((editor, _)) = &self.editor {
            return Some(editor.read(cx).content(cx));
        }
        match &self.content {
            Content::Note(n) => Some(n.read(cx).source().to_string()),
            Content::Canvas(c) => Some(c.read(cx).source().to_string()),
            _ => None,
        }
    }

    pub fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == mode || !self.kind.switchable() {
            return;
        }
        self.mode = mode;
        let kind = self.kind;
        gitviber_ui::Settings::update(cx, |s| match kind {
            Kind::Note => s.markdown_preview = mode == Mode::Preview,
            Kind::Svg => s.svg_preview = mode == Mode::Preview,
            _ => {}
        });
        let text = self.current_text(cx);
        match mode {
            Mode::Preview => {
                // The code view goes unless it has typing to keep.
                if self
                    .editor
                    .as_ref()
                    .is_some_and(|(e, _)| !e.read(cx).is_dirty())
                {
                    self.editor = None;
                }
                self.content = Content::Loading;
                match text {
                    Some(text) => self.show_preview(text, window, cx),
                    None => self.load(window, cx),
                }
            }
            Mode::Code => {
                // The rendered copy goes; the code keeps the text.
                self.content = Content::Loading;
                match text {
                    Some(text) => self.show_code(text, window, cx),
                    None => self.load(window, cx),
                }
            }
        }
        cx.notify();
    }

    /// Zooms the canvas on show by `factor`, about its middle.
    pub fn zoom_canvas(&mut self, factor: f64, window: &mut Window, cx: &mut Context<Self>) {
        if let Content::Canvas(c) = &self.content {
            c.update(cx, |c, cx| c.zoom_by(factor, window, cx));
        }
    }

    fn open_in_obsidian(&self, cx: &mut Context<Self>) {
        let (core, vault, path, ws) = (
            self.core.clone(),
            self.vault.clone(),
            self.path.clone(),
            self.workspace.clone(),
        );
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

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let vault_name = Vaults::global(cx).read(cx).name_of(&self.vault);
        let t = cx.app_theme();
        let name = self
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&self.path)
            .to_string();
        let dir = self
            .path
            .rsplit_once('/')
            .map(|(d, _)| d.to_string())
            .unwrap_or_default();
        let shown_dir = if dir.is_empty() {
            vault_name.to_string()
        } else {
            format!("{vault_name}/{dir}")
        };
        let readonly = self
            .editor
            .as_ref()
            .is_some_and(|(e, _)| self.mode == Mode::Code && e.read(cx).is_readonly());
        let switch = self.kind.switchable();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .h(px(30.))
            .px_3()
            .border_b_1()
            .border_color(t.border)
            .text_size(px(12.))
            .child(div().text_color(t.foreground).child(name))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(t.subtle)
                    .child(shown_dir),
            )
            .when(readonly, |d| {
                d.child(
                    div()
                        .px_1()
                        .rounded(px(3.))
                        .bg(t.elevated)
                        .text_size(px(10.5))
                        .text_color(t.muted_foreground)
                        .child("Read-only"),
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("open-in-obsidian")
                    .ghost()
                    .xsmall()
                    .icon(gpui_kit::assets::IconName::ExternalLink)
                    .label("Open in Obsidian")
                    .on_click(cx.listener(|this, _, _, cx| this.open_in_obsidian(cx))),
            )
            .when(switch, |d| {
                d.child(
                    Button::new("mode-code")
                        .ghost()
                        .xsmall()
                        .label("Code")
                        .toggled(self.mode == Mode::Code)
                        .on_click(
                            cx.listener(|this, _, window, cx| {
                                this.set_mode(Mode::Code, window, cx)
                            }),
                        ),
                )
                .child(
                    Button::new("mode-preview")
                        .ghost()
                        .xsmall()
                        .label("Preview")
                        .toggled(self.mode == Mode::Preview)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.set_mode(Mode::Preview, window, cx)
                        })),
                )
            })
    }

    #[cfg(test)]
    pub(crate) fn note(&self) -> Option<Entity<NoteView>> {
        match &self.content {
            Content::Note(n) => Some(n.clone()),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn canvas(&self) -> Option<Entity<CanvasView>> {
        match &self.content {
            Content::Canvas(c) => Some(c.clone()),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn editor(&self) -> Option<Entity<NoteEditor>> {
        self.editor.as_ref().map(|(e, _)| e.clone())
    }
}

impl TabItem for VaultTab {
    fn tab_title(&self, _: &App) -> SharedString {
        self.path
            .rsplit('/')
            .next()
            .unwrap_or(&self.path)
            .to_string()
            .into()
    }

    fn tab_icon(&self, _: &App) -> TabIcon {
        TabIcon::File(self.path.clone().into())
    }

    fn tab_tooltip(&self, _: &App) -> Option<SharedString> {
        Some(format!("{}/{}", self.vault, self.path).into())
    }

    fn is_dirty(&self, cx: &App) -> bool {
        self.editor
            .as_ref()
            .is_some_and(|(e, _)| e.read(cx).is_dirty())
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Task<bool>> {
        let (editor, _) = self.editor.as_ref()?;
        if editor.read(cx).is_readonly() {
            return None;
        }
        let (done, task) = save_signal(cx);
        editor.update(cx, |e, cx| e.save_then(Some(done), window, cx));
        Some(task)
    }

    fn discard(&mut self, cx: &mut Context<Self>) {
        if let Some((editor, _)) = &self.editor {
            editor.update(cx, |e, cx| e.forget(cx));
        }
    }

    /// Unsaved typing is kept in the vault edits store when the tab goes.
    fn keeps_edits(&self, _: &App) -> bool {
        true
    }

    fn tab_focus(&self, cx: &App) -> Option<FocusHandle> {
        Some(match &self.content {
            Content::Code => match &self.editor {
                Some((e, _)) => e.read(cx).editor_focus(cx),
                None => self.focus.clone(),
            },
            Content::Note(n) => n.read(cx).focus_handle().clone(),
            Content::Canvas(c) => c.read(cx).focus_handle().clone(),
            Content::Media(m) => m.read(cx).focus_handle().clone(),
            _ => self.focus.clone(),
        })
    }
}

impl Render for VaultTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let body = match &self.content {
            Content::Loading => div().size_full().into_any_element(),
            Content::Notice { title, detail } => div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .child(div().text_color(t.foreground).child(title.clone()))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(t.subtle)
                        .child(detail.clone()),
                )
                .into_any_element(),
            Content::Note(n) => n.clone().into_any_element(),
            Content::Canvas(c) => c.clone().into_any_element(),
            Content::Media(m) => m.clone().into_any_element(),
            Content::Code => match &self.editor {
                Some((e, _)) => e.clone().into_any_element(),
                None => div().into_any_element(),
            },
        };
        div()
            .id("vault-tab")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(t.code(t.background))
            .on_action(cx.listener(|this, _: &file::Reveal, _, cx| {
                let (core, vault, path) =
                    (this.core.clone(), this.vault.clone(), this.path.clone());
                cx.background_spawn(async move {
                    _ = api::obsidian::vault_reveal(&core, vault, path);
                })
                .detach();
            }))
            .child(self.header(cx))
            .child(div().flex_1().min_h_0().child(body))
    }
}
