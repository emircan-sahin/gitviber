//! A markdown or SVG file's diff shows rendered with Preview: markdown as the new version reads
//! (the old one for a deleted file), an SVG as both versions. Reviewing markdown starts on the
//! diff; an SVG opens however the last one was left. A preview shows the file as typed.

use super::*;
use crate::image::Backdrop;
use crate::model::zoom::{ACTUAL, FIT, TIP, Zoom};
use gitviber_markdown::{MarkdownBase, MarkdownEvent, MarkdownView, ReadImage, is_markdown};
use gitviber_ui::Settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Markdown,
    Svg,
}

pub(super) enum Preview {
    Markdown {
        view: Entity<MarkdownView>,
        _links: Subscription,
        /// The diff's own focus goes on to the preview (a tab shown again focuses the diff).
        _focus: Subscription,
    },
    Svg(Entity<ImageCompare>),
}

pub(super) fn is_svg(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".svg")
}

impl DiffView {
    /// A file that reads rendered, with a text side to render.
    pub(super) fn preview_kind(&self) -> Option<Kind> {
        if self.image.is_some() {
            return None;
        }
        let data = self.data.as_ref()?;
        let text = |s: &SideText| s.exists && s.textual();
        if !text(&data.old) && !text(&data.new) {
            return None;
        }
        let path = &self.source.path;
        if is_markdown(path) {
            Some(Kind::Markdown)
        } else if is_svg(path) {
            Some(Kind::Svg)
        } else {
            None
        }
    }

    pub(super) fn previewing(&self) -> bool {
        self.preview.is_some()
    }

    /// A markdown or SVG file shows rendered (it shows once the next frame is drawn).
    pub fn is_previewing(&self) -> bool {
        self.previewing()
    }

    fn wants_preview(&self, kind: Kind, cx: &App) -> bool {
        match kind {
            Kind::Markdown => self.markdown_preview,
            Kind::Svg => cx
                .try_global::<Settings>()
                .map_or(self.svg_preview, |s| s.svg_preview),
        }
    }

    /// Diff or Preview, as the header's switch sets it; nothing for other files.
    pub fn set_preview(&mut self, on: bool, cx: &mut Context<Self>) {
        match self.preview_kind() {
            Some(Kind::Markdown) => self.markdown_preview = on,
            Some(Kind::Svg) => {
                self.svg_preview = on;
                // SVGs open however the last one was left, diffs and the file view alike.
                if cx.has_global::<Settings>() {
                    Settings::update(cx, |s| s.svg_preview = on);
                }
            }
            None => return,
        }
        self.preview_stale = true;
        cx.notify();
    }

    /// Before and after as the preview shows them: what's typed over the new side, if anything.
    fn preview_texts(&self, cx: &App) -> [Option<String>; 2] {
        let Some(data) = &self.data else {
            return [None, None];
        };
        let typed = self
            .edit
            .as_ref()
            .and_then(|e| e.read(cx).unsaved(cx))
            .map(|(text, _)| text);
        let side = |s: &SideText| (s.exists && s.textual()).then(|| s.text());
        [side(&data.old), typed.or_else(|| side(&data.new))]
    }

    /// Builds or drops the preview to match the switch, and hands it the texts once they changed.
    /// The keyboard, when it was in the diff, goes into what shows: a markdown preview takes it
    /// for its scroll keys.
    pub(super) fn sync_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let want = self.preview_kind().filter(|&k| self.wants_preview(k, cx));
        let has = match &self.preview {
            Some(Preview::Markdown { .. }) => Some(Kind::Markdown),
            Some(Preview::Svg(_)) => Some(Kind::Svg),
            None => None,
        };
        // Asked before the preview goes: its focus goes with it.
        let had_focus = want != has && self.focus.contains_focused(window, cx);
        self.fill_preview(want, has, window, cx);
        if had_focus {
            let shown = match &self.preview {
                Some(Preview::Markdown { view, .. }) => view.focus_handle(cx),
                _ => self.focus.clone(),
            };
            window.focus(&shown, cx);
        }
    }

    fn fill_preview(
        &mut self,
        want: Option<Kind>,
        has: Option<Kind>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if want != has {
            self.preview = None;
            self.preview_stale = true;
            if want.is_some() {
                self.find = None;
                self.overview = None;
                self.places = None;
            }
        }
        let Some(kind) = want else { return };
        if !std::mem::take(&mut self.preview_stale) && self.preview.is_some() {
            return;
        }
        let [before, after] = self.preview_texts(cx);
        match (&self.preview, kind) {
            (Some(Preview::Markdown { view, .. }), _) => {
                let text = after.or(before).unwrap_or_default();
                view.update(cx, |v, cx| v.set_source(&text, cx));
            }
            (Some(Preview::Svg(view)), _) => {
                view.update(cx, |v, cx| v.set_svg_texts([before, after], cx));
            }
            (None, Kind::Markdown) => {
                let images = self.version_images(after.is_none());
                let text = after.or(before).unwrap_or_default();
                let base = match self.core.repo() {
                    Ok(root) => MarkdownBase::file(root, self.source.path.clone()),
                    Err(_) => MarkdownBase::default(),
                };
                let view = cx.new(|cx| {
                    let mut v = MarkdownView::new(&text, base, window, cx);
                    if images.is_some() {
                        v.read_images(images, window, cx);
                    }
                    v
                });
                let sub = cx.subscribe_in(&view, window, |_, _, e: &MarkdownEvent, window, cx| {
                    let MarkdownEvent::OpenFile { path, line } = e;
                    links::open_file(path, line.map(|l| (l, 0)), window, cx);
                });
                let focus = cx.on_focus(&self.focus, window, |this, window, cx| {
                    if let Some(Preview::Markdown { view, .. }) = &this.preview {
                        window.focus(&view.focus_handle(cx), cx);
                    }
                });
                self.preview = Some(Preview::Markdown {
                    view,
                    _links: sub,
                    _focus: focus,
                });
            }
            (None, Kind::Svg) => {
                let source = self.source.clone();
                let view = cx.new(|cx| {
                    let mut v = ImageCompare::svg(source, cx);
                    v.set_svg_texts([before, after], cx);
                    v
                });
                self.preview = Some(Preview::Svg(view));
            }
        }
    }

    /// The markdown preview on show (tests).
    #[doc(hidden)]
    pub fn markdown_preview(&self) -> Option<&Entity<MarkdownView>> {
        match &self.preview {
            Some(Preview::Markdown { view, .. }) => Some(view),
            _ => None,
        }
    }

    /// What the markdown preview would read for the picture at `path`; None: it reads the disk
    /// (tests).
    #[doc(hidden)]
    pub fn preview_image(&self, path: &str) -> Option<Result<Vec<u8>, String>> {
        let new = self.data.as_ref().is_some_and(|d| d.new.exists);
        self.version_images(!new).map(|read| read(path))
    }

    /// A markdown preview's pictures, from the version it shows (`original`: the old side's);
    /// None for the working tree's, which the preview reads from disk itself.
    fn version_images(&self, original: bool) -> Option<ReadImage> {
        let compare = self.source.compare.clone();
        if !original && compare.new_side_on_disk() {
            return None;
        }
        let core = self.core.clone();
        Some(Arc::new(move |path: &str| {
            let bytes = gitviber_core::api::files::media(
                &core,
                compare.kind().to_string(),
                path.to_string(),
                None,
                compare.sha().map(str::to_string),
                compare.base().map(str::to_string),
                original,
            )?;
            if bytes.is_empty() {
                return Err(format!("{path} isn't in this version"));
            }
            Ok(bytes)
        }))
    }

    pub(super) fn preview_element(&self) -> Option<AnyElement> {
        match self.preview.as_ref()? {
            Preview::Markdown { view, .. } => Some(view.clone().into_any_element()),
            Preview::Svg(view) => Some(view.clone().into_any_element()),
        }
    }

    /// The SVG preview's or the image's Fit / 1:1 and background, before the Diff / Preview
    /// switch.
    pub(super) fn svg_controls(
        &self,
        style: &DiffStyle,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let view = match (&self.preview, &self.image) {
            (Some(Preview::Svg(view)), _) | (None, Some(view)) => view,
            _ => return vec![],
        };
        let (zoom, backdrop) = {
            let v = view.read(cx);
            (v.zoom(), v.backdrop())
        };
        let dark = style.colors.background.l < 0.5;
        let contrast = if dark {
            Backdrop::Light
        } else {
            Backdrop::Dark
        };
        let zoom_button = |id: &'static str, label: &'static str, to: Zoom| {
            let view = view.downgrade();
            Button::new(id)
                .ghost()
                .xsmall()
                .label(label)
                .tooltip(TIP)
                .toggle(zoom.scale == to.scale, cx)
                .on_click(move |_, _, cx| {
                    view.update(cx, |v, cx| v.set_zoom(to, cx)).ok();
                })
        };
        let tip = match (backdrop, dark) {
            (Backdrop::Theme, true) => "Light background",
            (Backdrop::Theme, false) => "Dark background",
            _ => "Theme background",
        };
        let back = view.downgrade();
        vec![
            segmented(
                [
                    zoom_button("svg-fit", "Fit", FIT),
                    zoom_button("svg-actual", "1:1", ACTUAL),
                ],
                cx,
            )
            .into_any_element(),
            icon_button("svg-contrast", Lucide::Contrast, tip)
                .toggle(backdrop != Backdrop::Theme, cx)
                .on_click(move |_, _, cx| {
                    back.update(cx, |v, cx| {
                        let next = if v.backdrop() == Backdrop::Theme {
                            contrast
                        } else {
                            Backdrop::Theme
                        };
                        v.set_backdrop(next, cx)
                    })
                    .ok();
                })
                .into_any_element(),
        ]
    }

    /// The header's last switch: Diff or Preview.
    pub(super) fn preview_switch(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.preview_kind()?;
        let on = self.previewing();
        let segments = [
            Button::new("show-diff")
                .ghost()
                .xsmall()
                .icon(Lucide::GitCompareArrows)
                .label("Diff")
                .toggle(!on, cx)
                .on_click(cx.listener(|this, _, _, cx| this.set_preview(false, cx))),
            Button::new("show-preview")
                .ghost()
                .xsmall()
                .icon(Lucide::Eye)
                .label("Preview")
                .toggle(on, cx)
                .on_click(cx.listener(|this, _, _, cx| this.set_preview(true, cx))),
        ];
        Some(segmented(segments, cx).ml_1().into_any_element())
    }
}
