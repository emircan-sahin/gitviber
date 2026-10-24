//! A file tab: its text in the code view, a markdown or SVG file rendered (Code / Preview),
//! media in the media view, or why it can't show (binary, too large, gone, an LFS pointer). It
//! reads the file and HEAD's version in one go off the main thread, and again only when the
//! file's modification time moved or git's state changed.

use std::path::PathBuf;
use std::sync::Arc;

use gitviber_core::git::{DiffPair, FileText};
use gitviber_core::{Core, api};
use gitviber_markdown::{MarkdownBase, MarkdownEvent, MarkdownView, is_markdown};
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::file;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{Project, ProjectEvent, TabIcon, TabItem, Workspace};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, StyledImage as _, Subscription, Task,
    WeakEntity, Window, div, prelude::FluentBuilder as _, px,
};

use crate::code::{CodeEvent, CodeSource, CodeView};
use crate::media::{MediaKind, MediaView, media_kind};
use crate::text::{self, Bar};
use crate::{OpenOptions, Reveal, backend, open_file};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileSource {
    /// A file in the open repo, by its repo-relative path.
    Repo(String),
    /// A file outside it, by its absolute path: shown read-only.
    External(String),
}

impl FileSource {
    pub fn path(&self) -> &str {
        match self {
            FileSource::Repo(p) | FileSource::External(p) => p,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Code,
    Preview,
}

enum Content {
    Loading,
    Notice {
        title: SharedString,
        detail: SharedString,
        open: bool,
    },
    Media(Entity<MediaView>),
    Text {
        code: Entity<CodeView>,
        preview: Option<Entity<MarkdownView>>,
        _subs: Vec<Subscription>,
    },
}

pub struct FileView {
    workspace: WeakEntity<Workspace>,
    core: Arc<Core>,
    source: FileSource,
    root: Option<PathBuf>,
    content: Content,
    mode: Mode,
    /// A line asked for before the text was there.
    reveal: Option<Reveal>,
    /// The file's modification time when read, to skip re-reading it on others' changes.
    mtime: Option<f64>,
    focus: FocusHandle,
    _load: Task<()>,
    _subs: Vec<Subscription>,
    _live: Live,
}

/// What reading the file found.
enum Loaded {
    Pair(DiffPair),
    External(FileText),
    Failed(String),
}

impl FileView {
    /// A file tab of `workspace`'s project. Openers run while the workspace is being updated, so
    /// the project is read a moment later, before the file is.
    pub fn new(
        workspace: WeakEntity<Workspace>,
        source: FileSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path = source.path();
        let mode = if (is_markdown(path) && gitviber_ui::Settings::get(cx).markdown_preview)
            || (path.to_ascii_lowercase().ends_with(".svg")
                && gitviber_ui::Settings::get(cx).svg_preview)
        {
            Mode::Preview
        } else {
            Mode::Code
        };
        cx.defer_in(window, |this: &mut Self, window, cx| {
            this.attach(window, cx)
        });
        FileView {
            workspace,
            core: backend::core(cx),
            source,
            root: None,
            content: Content::Loading,
            mode,
            reveal: None,
            mtime: None,
            focus: cx.focus_handle(),
            _load: Task::ready(()),
            _subs: Vec::new(),
            _live: Live::new("FileView"),
        }
    }

    /// Follows the project (its root, its changes) and reads the file.
    fn attach(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let project = self
            .workspace
            .upgrade()
            .and_then(|ws| ws.read(cx).project().cloned());
        if let (Some(project), FileSource::Repo(_)) = (&project, &self.source) {
            self.root = Some(project.read(cx).root().to_path_buf());
            self._subs.push(cx.subscribe_in(
                project,
                window,
                |this, project: &Entity<Project>, e: &ProjectEvent, window, cx| match e {
                    ProjectEvent::RepoChanged(change) => this.repo_changed(change.git, window, cx),
                    ProjectEvent::Opened | ProjectEvent::Switched { .. } => {
                        this.root = Some(project.read(cx).root().to_path_buf());
                        this.load(window, cx);
                    }
                    ProjectEvent::Failed(_) => {}
                },
            ));
        }
        self.load(window, cx);
    }

    #[cfg(test)]
    pub(crate) fn code(&self) -> Option<Entity<CodeView>> {
        match &self.content {
            Content::Text { code, .. } => Some(code.clone()),
            _ => None,
        }
    }

    pub fn source(&self) -> &FileSource {
        &self.source
    }

    fn abs_path(&self) -> PathBuf {
        match (&self.source, &self.root) {
            (FileSource::Repo(p), Some(root)) => root.join(p),
            (s, _) => PathBuf::from(s.path()),
        }
    }

    fn previewable(&self) -> bool {
        let p = self.source.path().to_ascii_lowercase();
        is_markdown(&p) || p.ends_with(".svg")
    }

    /// Shows `reveal` once the text is there (in the code: a rendered preview switches to it).
    pub fn reveal(&mut self, reveal: Reveal, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Preview {
            self.set_mode(Mode::Code, window, cx);
        }
        match &self.content {
            Content::Text { code, .. } => code.update(cx, |c, cx| c.reveal(reveal, window, cx)),
            _ => self.reveal = Some(reveal),
        }
    }

    fn read(core: Arc<Core>, source: FileSource) -> (Loaded, Option<f64>) {
        match source {
            FileSource::Repo(path) => {
                let mtime = api::files::file_mtimes(&core, vec![path.clone()])
                    .ok()
                    .and_then(|m| m.into_iter().next().flatten());
                let pair =
                    api::files::diff_pair(&core, "worktree".into(), path, None, None, None, None);
                (pair.map_or_else(Loaded::Failed, Loaded::Pair), mtime)
            }
            FileSource::External(path) => (
                api::files::external_read_file(&core, path)
                    .map_or_else(Loaded::Failed, Loaded::External),
                None,
            ),
        }
    }

    pub fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (core, source) = (self.core.clone(), self.source.clone());
        self._load = cx.spawn_in(window, async move |this, cx| {
            let (loaded, mtime) = cx
                .background_spawn(async move { Self::read(core, source) })
                .await;
            _ = this.update_in(cx, |this, window, cx| {
                this.mtime = mtime;
                this.show(loaded, window, cx);
            });
        });
    }

    /// Something in the repo changed: the file is read again if it did, or HEAD moved.
    fn repo_changed(&mut self, git: bool, window: &mut Window, cx: &mut Context<Self>) {
        let FileSource::Repo(path) = self.source.clone() else {
            return;
        };
        let (core, was) = (self.core.clone(), self.mtime);
        self._load = cx.spawn_in(window, async move |this, cx| {
            let now = {
                let (core, path) = (core.clone(), path.clone());
                cx.background_spawn(async move {
                    api::files::file_mtimes(&core, vec![path])
                        .ok()
                        .and_then(|m| m.into_iter().next().flatten())
                })
                .await
            };
            if now == was && !git {
                return;
            }
            let (loaded, mtime) = cx
                .background_spawn(async move { Self::read(core, FileSource::Repo(path)) })
                .await;
            _ = this.update_in(cx, |this, window, cx| {
                this.mtime = mtime;
                this.show(loaded, window, cx);
            });
        });
    }

    fn notice(
        &mut self,
        title: &str,
        detail: impl Into<SharedString>,
        open: bool,
        cx: &mut Context<Self>,
    ) {
        self.content = Content::Notice {
            title: title.to_string().into(),
            detail: detail.into(),
            open,
        };
        cx.notify();
    }

    fn show(&mut self, loaded: Loaded, window: &mut Window, cx: &mut Context<Self>) {
        let (head, file, rows) = match loaded {
            Loaded::Failed(e) => return self.notice("Could not read the file", e, false, cx),
            Loaded::External(f) => (None, f, vec![]),
            Loaded::Pair(DiffPair {
                original,
                modified,
                rows,
                ..
            }) => {
                let head = original.exists.then_some(original.text);
                (head, modified, rows)
            }
        };
        let path = self.source.path().to_string();
        let kind = media_kind(&path);
        if !file.exists {
            return self.notice(
                "This file is not on disk",
                "It was deleted or moved since it opened.",
                false,
                cx,
            );
        }
        if let Some(size) = file.lfs_missing {
            return self.notice(
                "Git LFS file not downloaded",
                format!("Its object ({size}) isn't here; run `git lfs pull` to get it."),
                false,
                cx,
            );
        }
        if let Some(kind) =
            kind.filter(|k| *k != MediaKind::Image || !path.to_ascii_lowercase().ends_with(".svg"))
        {
            if !matches!(self.content, Content::Media(_)) {
                let abs = self.abs_path();
                self.content = Content::Media(cx.new(|cx| MediaView::new(abs, kind, window, cx)));
            }
            cx.notify();
            return;
        }
        if file.too_large {
            return self.notice(
                "This file is too large to show",
                "Files over 8 MB open in another app.",
                true,
                cx,
            );
        }
        if file.binary {
            return self.notice("Binary file", "It has no text to show.", true, cx);
        }
        let bars: Vec<Bar> = if head.is_some() {
            text::change_bars(&rows)
        } else {
            vec![]
        };
        drop(rows);
        let readonly = !matches!(self.source, FileSource::Repo(_))
            || file.lossy
            || !text::keeps_line_endings(&file.text);
        if let Content::Text { code, preview, .. } = &self.content {
            let (code, preview) = (code.clone(), preview.clone());
            if let Some(preview) = &preview {
                preview.update(cx, |p, cx| p.set_source(&file.text, cx));
            }
            code.update(cx, |c, cx| {
                c.file_changed(file.text, head, bars, window, cx)
            });
            return;
        }
        let source = CodeSource {
            path: path.clone(),
            root: self.root.clone(),
        };
        let ws = self.workspace.clone();
        let code =
            cx.new(|cx| CodeView::new(ws, source, file.text, head, bars, readonly, window, cx));
        let sub = cx.subscribe(&code, |_, _, e: &CodeEvent, cx| match e {
            CodeEvent::DirtyChanged | CodeEvent::Saved => cx.notify(),
        });
        self.content = Content::Text {
            code: code.clone(),
            preview: None,
            _subs: vec![sub],
        };
        if let Some(reveal) = self.reveal.take() {
            self.mode = Mode::Code;
            code.update(cx, |c, cx| c.reveal(reveal, window, cx));
        }
        if self.mode == Mode::Preview {
            self.build_preview(window, cx);
        }
        cx.notify();
    }

    fn build_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Content::Text {
            code,
            preview,
            _subs,
        } = &mut self.content
        else {
            return;
        };
        if preview.is_some() || !is_markdown(self.source.path()) {
            return;
        }
        let text = code.read(cx).text(cx).to_string();
        let base = match (&self.root, &self.source) {
            (Some(root), FileSource::Repo(path)) => MarkdownBase::file(root.clone(), path.clone()),
            _ => MarkdownBase::default(),
        };
        let view = cx.new(|cx| MarkdownView::new(&text, base, window, cx));
        let ws = self.workspace.clone();
        _subs.push(cx.subscribe_in(
            &view,
            window,
            move |_, _, e: &MarkdownEvent, window, cx| match e {
                MarkdownEvent::OpenFile { path, line } => {
                    let reveal = line.map(|line| Reveal::Line { line, column: 0 });
                    open_file(
                        &ws,
                        path,
                        OpenOptions {
                            preview: false,
                            reveal,
                            focus: true,
                        },
                        window,
                        cx,
                    );
                }
            },
        ));
        *preview = Some(view);
    }

    fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        let lower = self.source.path().to_ascii_lowercase();
        gitviber_ui::Settings::update(cx, |s| {
            if lower.ends_with(".svg") {
                s.svg_preview = mode == Mode::Preview;
            } else {
                s.markdown_preview = mode == Mode::Preview;
            }
        });
        match mode {
            Mode::Preview => self.build_preview(window, cx),
            // The rendered copy goes; the code keeps the text (and any typing).
            Mode::Code => {
                if let Content::Text { preview, .. } = &mut self.content {
                    *preview = None;
                }
            }
        }
        cx.notify();
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        let path = self.source.path().to_string();
        let (dir, name) = path
            .rsplit_once('/')
            .map(|(d, n)| (d.to_string(), n.to_string()))
            .unwrap_or((String::new(), path.clone()));
        let readonly = match &self.content {
            Content::Text { code, .. } => code.read(cx).is_readonly(),
            _ => false,
        };
        let svg = path.to_ascii_lowercase().ends_with(".svg");
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
            .child(div().min_w_0().truncate().text_color(t.subtle).child(dir))
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
            // Who last changed each line (View → Blame), for the file in the repo as code.
            .when(
                matches!(self.source, FileSource::Repo(_))
                    && matches!(self.content, Content::Text { .. })
                    && self.mode == Mode::Code,
                |d| {
                    let on = gitviber_ui::Settings::get(cx).blame;
                    d.child(
                        Button::new("blame")
                            .ghost()
                            .xsmall()
                            .label("Blame")
                            .toggled(on)
                            .on_click(|_, _, cx| {
                                gitviber_ui::Settings::update(cx, |s| s.blame = !s.blame)
                            }),
                    )
                },
            )
            .when(
                self.previewable() && matches!(self.content, Content::Text { .. }) || svg,
                |d| {
                    d.child(
                        Button::new("mode-code")
                            .ghost()
                            .xsmall()
                            .label("Code")
                            .toggled(self.mode == Mode::Code)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_mode(Mode::Code, window, cx)
                            })),
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
                },
            )
    }
}

impl TabItem for FileView {
    fn tab_title(&self, _: &App) -> SharedString {
        let path = self.source.path();
        path.rsplit(['/', '\\'])
            .next()
            .unwrap_or(path)
            .to_string()
            .into()
    }

    fn tab_icon(&self, _: &App) -> TabIcon {
        TabIcon::File(self.source.path().to_string().into())
    }

    fn tab_tooltip(&self, _: &App) -> Option<SharedString> {
        Some(self.source.path().to_string().into())
    }

    fn tab_detail(&self, _: &App) -> Option<SharedString> {
        matches!(self.source, FileSource::External(_)).then(|| "read-only".into())
    }

    fn is_dirty(&self, cx: &App) -> bool {
        matches!(&self.content, Content::Text { code, .. } if code.read(cx).is_dirty())
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Task<bool>> {
        match &self.content {
            Content::Text { code, .. } => code.clone().update(cx, |c, cx| c.save_task(window, cx)),
            _ => None,
        }
    }

    /// An open repo file's text; never one outside the repo, which is read-only.
    fn can_save(&self, _: &App) -> bool {
        matches!(self.content, Content::Text { .. }) && matches!(self.source, FileSource::Repo(_))
    }

    fn tab_focus(&self, cx: &App) -> Option<FocusHandle> {
        Some(match &self.content {
            Content::Text {
                preview: Some(p), ..
            } if self.mode == Mode::Preview => gpui_kit::Focusable::focus_handle(p.read(cx), cx),
            Content::Text { code, .. } => code.read(cx).editor_focus(cx),
            Content::Media(m) => m.read(cx).focus_handle().clone(),
            _ => self.focus.clone(),
        })
    }
}

impl Render for FileView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        let svg_preview =
            self.mode == Mode::Preview && self.source.path().to_ascii_lowercase().ends_with(".svg");
        let body = match &self.content {
            Content::Loading => div().size_full().into_any_element(),
            Content::Notice {
                title,
                detail,
                open,
            } => {
                let abs = self.abs_path();
                div()
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
                    .when(*open, |d| {
                        d.child(
                            Button::new("open-default")
                                .small()
                                .label("Open in Default App")
                                .on_click(move |_, _, cx| cx.open_with_system(&abs)),
                        )
                    })
                    .into_any_element()
            }
            Content::Media(m) => m.clone().into_any_element(),
            Content::Text {
                preview: Some(p), ..
            } if self.mode == Mode::Preview => p.clone().into_any_element(),
            Content::Text { .. } if svg_preview => {
                let abs = self.abs_path();
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_4()
                    .child(
                        gpui_kit::img(abs)
                            .max_w_full()
                            .max_h_full()
                            .size_full()
                            .object_fit(gpui_kit::ObjectFit::ScaleDown),
                    )
                    .into_any_element()
            }
            Content::Text { code, .. } => code.clone().into_any_element(),
        };
        let path = self.source.path().to_string();
        let ws = self.workspace.clone();
        div()
            .id("file-view")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(t.code(t.background))
            .on_action(
                cx.listener(move |this, _: &file::RevealInExplorer, window, cx| {
                    if let FileSource::Repo(path) = &this.source {
                        crate::reveal_in_explorer(&ws, path, window, cx);
                    }
                }),
            )
            .on_action(cx.listener(move |this, _: &file::Reveal, _, cx| {
                let abs = this.abs_path();
                let core = this.core.clone();
                let rel = path.clone();
                let external = matches!(this.source, FileSource::External(_));
                cx.background_spawn(async move {
                    _ = if external {
                        api::files::external_reveal(&core, abs.to_string_lossy().into_owned())
                    } else {
                        api::files::reveal_path(&core, rel)
                    };
                })
                .detach();
            }))
            .child(self.header(cx))
            .child(div().flex_1().min_h_0().child(body))
    }
}
