//! A markdown or SVG file's diff shows rendered with Preview: markdown as the new version reads
//! (the old one for a deleted file), an SVG as both versions. Reviewing markdown starts on the
//! diff; an SVG opens however the last one was left. A preview shows the file as typed.

use super::*;
use crate::image::Backdrop;
use crate::model::zoom::{ACTUAL, FIT, Zoom};
use gitviber_markdown::{MarkdownBase, MarkdownEvent, MarkdownView, is_markdown};
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
    pub(super) fn sync_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let want = self.preview_kind().filter(|&k| self.wants_preview(k, cx));
        let has = match &self.preview {
            Some(Preview::Markdown { .. }) => Some(Kind::Markdown),
            Some(Preview::Svg(_)) => Some(Kind::Svg),
            None => None,
        };
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
                let text = after.or(before).unwrap_or_default();
                let base = match self.core.repo() {
                    Ok(root) => MarkdownBase::file(root, self.source.path.clone()),
                    Err(_) => MarkdownBase::default(),
                };
                let view = cx.new(|cx| MarkdownView::new(&text, base, window, cx));
                let sub = cx.subscribe_in(&view, window, |_, _, e: &MarkdownEvent, window, cx| {
                    let MarkdownEvent::OpenFile { path, line } = e;
                    links::open_file(path, line.map(|l| (l, 0)), window, cx);
                });
                self.preview = Some(Preview::Markdown { view, _links: sub });
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

    pub(super) fn preview_element(&self) -> Option<AnyElement> {
        match self.preview.as_ref()? {
            Preview::Markdown { view, .. } => Some(view.clone().into_any_element()),
            Preview::Svg(view) => Some(view.clone().into_any_element()),
        }
    }

    /// The SVG preview's Fit / 1:1 and background, before the Diff / Preview switch.
    pub(super) fn svg_controls(
        &self,
        style: &DiffStyle,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(Preview::Svg(view)) = &self.preview else {
            return vec![];
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
                .tooltip("Scroll to zoom, drag to pan, double-click to fit")
                .toggled(zoom.scale == to.scale)
                .on_click(move |_, _, cx| {
                    view.update(cx, |v, cx| v.set_zoom(to, cx)).ok();
                })
                .into_any_element()
        };
        let tip = match (backdrop, dark) {
            (Backdrop::Theme, true) => "Light background",
            (Backdrop::Theme, false) => "Dark background",
            _ => "Theme background",
        };
        let back = view.downgrade();
        vec![
            zoom_button("svg-fit", "Fit", FIT),
            zoom_button("svg-actual", "1:1", ACTUAL),
            icon_button(
                "svg-contrast",
                Lucide::Contrast,
                tip,
                backdrop != Backdrop::Theme,
            )
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
        Some(
            h_flex()
                .flex_none()
                .ml_1()
                .child(
                    Button::new("show-diff")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::GitCompareArrows)
                        .label("Diff")
                        .toggled(!on)
                        .on_click(cx.listener(|this, _, _, cx| this.set_preview(false, cx))),
                )
                .child(
                    Button::new("show-preview")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Eye)
                        .label("Preview")
                        .toggled(on)
                        .on_click(cx.listener(|this, _, _, cx| this.set_preview(true, cx))),
                )
                .into_any_element(),
        )
    }
}
