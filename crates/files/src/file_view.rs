//! A file tab: its text in the code view, a markdown or SVG file rendered (Code / Preview),
//! media in the media view, or why it can't show (binary, too large, gone, an LFS pointer). It
//! reads the file and HEAD's version in one go off the main thread, and again only when the
//! file's modification time moved or git's state changed.

use std::path::PathBuf;
use std::sync::Arc;

use gitviber_core::git::{DiffPair, FileText};
use gitviber_core::{Core, api};
use gitviber_markdown::{
    FindBar, FindBarEvent, FindChanged, FindQuery, MarkdownBase, MarkdownEvent, MarkdownView,
    is_markdown,
};
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::{editor, file};
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::{
    Project, ProjectEvent, Selection, TabIcon, TabItem, Toast, Workspace,
};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, WeakEntity, Window,
    div, prelude::FluentBuilder as _, px,
};

use crate::code::{CodeEvent, CodeSource, CodeView};
use crate::media::{MediaKind, MediaView, media_kind};
use crate::svg::SvgPreview;
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
        svg: Option<Entity<SvgPreview>>,
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
    /// It differs from HEAD (or is new): the header offers its diff.
    changed: bool,
    /// Its Git LFS object isn't here: the notice offers to download it.
    lfs: bool,
    /// ⌘F's box over the rendered preview, made the first time; its query stays for the next.
    find: Option<PreviewFind>,
    find_open: bool,
    focus: FocusHandle,
    _load: Task<()>,
    _subs: Vec<Subscription>,
    _live: Live,
}

struct PreviewFind {
    bar: Entity<FindBar>,
    _sub: Subscription,
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
            changed: false,
            lfs: false,
            find: None,
            find_open: false,
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

    /// The language of the code on screen, for the status bar; None while a preview, media or
    /// a notice shows.
    pub fn shown_language(&self, cx: &App) -> Option<&'static str> {
        match &self.content {
            Content::Text { code, .. } if self.mode == Mode::Code => Some(code.read(cx).language()),
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
            Content::Text { code, .. } => {
                let code = code.clone();
                self.reveal_in(&code, reveal, window, cx)
            }
            _ => self.reveal = Some(reveal),
        }
    }

    /// Reveals in the code, which focuses the editor: focus outside this tab (the Search
    /// panel's results, stepped through) goes back where it was.
    fn reveal_in(
        &self,
        code: &Entity<CodeView>,
        reveal: Reveal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = window
            .focused(cx)
            .filter(|f| *f != self.focus && !self.focus.contains(f, window));
        code.update(cx, |c, cx| c.reveal(reveal, window, cx));
        if let Some(before) = before {
            before.focus(window, cx);
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

    /// This file's uncommitted diff: the unstaged one, else the staged one.
    fn open_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let FileSource::Repo(path) = self.source.clone() else {
            return;
        };
        let core = self.core.clone();
        self._load = cx.spawn_in(window, async move |this, cx| {
            let status = cx
                .background_spawn(async move { api::changes::status(&core) })
                .await;
            _ = this.update_in(cx, |this, window, cx| {
                let selection = status.ok().and_then(|st| {
                    let find = |list: &[gitviber_core::git::FileChange]| {
                        list.iter().find(|f| f.path == path).cloned().map(Arc::new)
                    };
                    find(&st.unstaged)
                        .map(|file| Selection::Unstaged { file })
                        .or_else(|| find(&st.staged).map(|file| Selection::Staged { file }))
                });
                match selection {
                    Some(selection) => {
                        _ = this
                            .workspace
                            .update(cx, |ws, cx| ws.open(selection, false, window, cx));
                    }
                    None => {
                        _ = this.workspace.update(cx, |ws, cx| {
                            ws.toast(Toast::info("No uncommitted changes."), cx)
                        });
                    }
                }
            });
        });
    }

    /// Fetches the file's Git LFS object, then shows the file.
    fn download_lfs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let FileSource::Repo(path) = self.source.clone() else {
            return;
        };
        let core = self.core.clone();
        self._load = cx.spawn_in(window, async move |this, cx| {
            let op = format!("lfs-pull:{path}");
            let shown = path.clone();
            let pulled = cx
                .background_spawn(async move { api::sync::lfs_pull(&core, path, op, |_| {}) })
                .await;
            _ = this.update_in(cx, |this, window, cx| {
                let toast = match pulled {
                    Ok(()) => Toast::success(format!("Downloaded {shown}")),
                    Err(e) => Toast::error(e).title("Could not download the LFS file"),
                };
                _ = this.workspace.update(cx, |ws, cx| ws.toast(toast, cx));
                this.load(window, cx);
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
        self.lfs = false;
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
            self.notice(
                "Git LFS file not downloaded",
                format!("Its object ({size}) isn't here."),
                false,
                cx,
            );
            self.lfs = true;
            return;
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
        // Differs from HEAD, or isn't in it: there's an uncommitted change to open.
        self.changed = matches!(self.source, FileSource::Repo(_))
            && (head.is_none() || rows.iter().any(|r| r.k != 0));
        drop(rows);
        let readonly = !matches!(self.source, FileSource::Repo(_))
            || file.lossy
            || !text::keeps_line_endings(&file.text);
        if let Content::Text {
            code, preview, svg, ..
        } = &self.content
        {
            let (code, preview, svg) = (code.clone(), preview.clone(), svg.clone());
            if let Some(preview) = &preview {
                preview.update(cx, |p, cx| p.set_source(&file.text, cx));
            }
            if let Some(svg) = &svg {
                svg.update(cx, |p, cx| p.set_source(&file.text, cx));
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
            svg: None,
            _subs: vec![sub],
        };
        if let Some(reveal) = self.reveal.take() {
            self.mode = Mode::Code;
            self.reveal_in(&code, reveal, window, cx);
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
            svg,
            _subs,
        } = &mut self.content
        else {
            return;
        };
        // From the code's text: typing not saved yet shows.
        if svg.is_none() && self.source.path().to_ascii_lowercase().ends_with(".svg") {
            let text = code.read(cx).text(cx).to_string();
            *svg = Some(cx.new(|cx| SvgPreview::new(&text, cx)));
            return;
        }
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
        // The file changed on disk: the find's count follows.
        _subs.push(cx.subscribe(&view, |this, _, _: &FindChanged, cx| this.show_found(cx)));
        *preview = Some(view);
    }

    fn shown_preview(&self) -> Option<Entity<MarkdownView>> {
        match &self.content {
            Content::Text {
                preview: Some(p), ..
            } if self.mode == Mode::Preview => Some(p.clone()),
            _ => None,
        }
    }

    /// ⌘F on the rendered preview: the find box, focused with its text selected.
    pub(crate) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shown_preview().is_none() {
            return;
        }
        let find = self.find.get_or_insert_with(|| {
            let bar = cx.new(|cx| FindBar::new(window, cx));
            let sub = cx.subscribe_in(
                &bar,
                window,
                |this, _, e: &FindBarEvent, window, cx| match e {
                    FindBarEvent::Changed => this.find_again(cx),
                    FindBarEvent::Step(dir) => {
                        if let Some(preview) = this.shown_preview() {
                            preview.update(cx, |p, cx| p.step_match(*dir, cx));
                            this.show_found(cx);
                        }
                    }
                    FindBarEvent::Close => this.close_find(window, cx),
                },
            );
            PreviewFind { bar, _sub: sub }
        });
        let bar = find.bar.clone();
        if !self.find_open {
            self.find_open = true;
            // The last query, marked again.
            self.find_again(cx);
        }
        bar.update(cx, |b, cx| b.focus(window, cx));
        cx.notify();
    }

    /// Runs the box's query on the preview, from its first match.
    fn find_again(&mut self, cx: &mut Context<Self>) {
        let (Some(f), Some(preview)) = (&self.find, self.shown_preview()) else {
            return;
        };
        if !self.find_open {
            return;
        }
        let query = f.bar.read(cx).query().clone();
        let found = preview.update(cx, |p, cx| {
            let found = p.find(&query, cx);
            p.select_match(Some(0), true, cx);
            found.map(|_| p.matches())
        });
        f.bar.update(cx, |b, cx| b.set_found(found, cx));
    }

    /// The box's count, from the preview (stepped, or its text changed).
    fn show_found(&mut self, cx: &mut Context<Self>) {
        if let (Some(f), Some(preview)) = (&self.find, self.shown_preview()) {
            let found = Ok(preview.read(cx).matches());
            f.bar.update(cx, |b, cx| b.set_found(found, cx));
        }
    }

    /// The preview find's current match and count; None while its box is closed.
    #[cfg(test)]
    pub(crate) fn preview_find(&self, cx: &App) -> Option<(Option<usize>, usize)> {
        self.find_open.then_some(())?;
        Some(self.shown_preview()?.read(cx).matches())
    }

    pub(crate) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.find_open) {
            return;
        }
        if let Some(preview) = self.shown_preview() {
            preview.update(cx, |p, cx| {
                _ = p.find(&FindQuery::default(), cx);
            });
            gpui_kit::Focusable::focus_handle(preview.read(cx), cx).focus(window, cx);
        }
        cx.notify();
    }

    fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        // The box searches the rendered copy, which goes or comes anew.
        self.find_open = false;
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
                if let Content::Text { preview, svg, .. } = &mut self.content {
                    *preview = None;
                    *svg = None;
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
            .when(self.changed, |d| {
                d.child(
                    Button::new("changes")
                        .ghost()
                        .xsmall()
                        .label("Changes")
                        .tooltip("This file's uncommitted changes")
                        .on_click(cx.listener(|this, _, window, cx| this.open_changes(window, cx))),
                )
            })
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
            Content::Text { svg: Some(s), .. } if self.mode == Mode::Preview => {
                s.read(cx).focus_handle().clone()
            }
            Content::Text { code, .. } => code.read(cx).editor_focus(cx),
            Content::Media(m) => m.read(cx).focus_handle().clone(),
            _ => self.focus.clone(),
        })
    }
}

impl Render for FileView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        let body =
            match &self.content {
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
                        .when(self.lfs, |d| {
                            d.child(
                                Button::new("lfs-pull")
                                    .small()
                                    .label("Download with Git LFS")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.download_lfs(window, cx)
                                    })),
                            )
                        })
                        .into_any_element()
                }
                Content::Media(m) => m.clone().into_any_element(),
                Content::Text {
                    preview: Some(p), ..
                } if self.mode == Mode::Preview => p.clone().into_any_element(),
                Content::Text { svg: Some(s), .. } if self.mode == Mode::Preview => {
                    s.clone().into_any_element()
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
            .on_action(cx.listener(|this, _: &editor::Find, window, cx| {
                if this.shown_preview().is_some() {
                    this.open_find(window, cx);
                } else {
                    cx.propagate();
                }
            }))
            .child(self.header(cx))
            .child(
                div().relative().flex_1().min_h_0().child(body).children(
                    self.find
                        .as_ref()
                        .filter(|_| self.find_open && self.shown_preview().is_some())
                        .map(|f| div().absolute().top_2().right_4().child(f.bar.clone())),
                ),
            )
    }
}
