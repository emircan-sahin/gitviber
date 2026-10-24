//! A file that isn't text: an image fit to the view or zoomed (= − 0), decoded scaled to the
//! display through a cache the view owns; PDFs, audio and video in macOS's own viewers
//! (native_media.rs), elsewhere a card that opens them in the default app.

use std::path::{Path, PathBuf};

use gitviber_markdown::BoundedImageCache;
use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::keymap::media;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::{FileIcon, Toggle as _};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{
    AppContext as _, Context, Entity, FocusHandle, ImageSource, InteractiveElement as _,
    IntoElement, ObjectFit, ParentElement as _, Render, Resource, StatefulInteractiveElement as _,
    Styled as _, StyledImage as _, Task, Window, div, image_cache, img, px,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Audio,
    Video,
    Pdf,
}

/// What a file's extension says it is, when it's media.
pub fn media_kind(path: &str) -> Option<MediaKind> {
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    Some(match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff" | "tga"
        | "qoi" | "svg" => MediaKind::Image,
        "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" | "oga" | "opus" => MediaKind::Audio,
        "mp4" | "m4v" | "mov" | "webm" | "ogv" | "mkv" | "avi" => MediaKind::Video,
        "pdf" => MediaKind::Pdf,
        _ => return None,
    })
}

const ZOOM_STEP: f32 = 1.25;

pub struct MediaView {
    /// The file on disk.
    path: PathBuf,
    kind: MediaKind,
    images: Entity<BoundedImageCache>,
    /// None: fit to the view.
    zoom: Option<f32>,
    /// The image's own size, read from its header.
    natural: Option<(u32, u32)>,
    focus: FocusHandle,
    /// A PDF's, a video's or a song's native viewer (macOS); None draws the open-elsewhere card.
    #[cfg(target_os = "macos")]
    native: Option<crate::native_media::NativeMedia>,
    _size: Task<()>,
    _subs: Vec<gpui_kit::Subscription>,
    _live: Live,
}

impl MediaView {
    pub fn new(
        path: PathBuf,
        kind: MediaKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let images = BoundedImageCache::new(BoundedImageCache::display_side(window), cx);
        let header = path.clone();
        let size = if kind == MediaKind::Image {
            cx.spawn(async move |this, cx| {
                let natural = cx
                    .background_spawn(async move { image::image_dimensions(&header).ok() })
                    .await;
                _ = this.update(cx, |this, cx| {
                    this.natural = natural;
                    cx.notify();
                });
            })
        } else {
            Task::ready(())
        };
        // A tab switch may leave a native viewer out of sight with nothing painted to say so.
        let mut subs = Vec::new();
        if kind != MediaKind::Image
            && let Some(ws) = gitviber_ui::workspace::Workspace::for_window(window, cx)
        {
            subs.push(cx.subscribe_in(
                &ws,
                window,
                |_, _, e: &gitviber_ui::workspace::WorkspaceEvent, window, cx| {
                    use gitviber_ui::workspace::WorkspaceEvent::*;
                    if matches!(e, TabsChanged | LayoutChanged | ProjectChanged) {
                        #[cfg(target_os = "macos")]
                        crate::native_media::check_soon(window, cx);
                        #[cfg(not(target_os = "macos"))]
                        let _ = (window, cx);
                    }
                },
            ));
        }
        MediaView {
            #[cfg(target_os = "macos")]
            native: crate::native_media::NativeMedia::open(&path, kind),
            path,
            kind,
            images,
            zoom: None,
            natural: None,
            focus: cx.focus_handle(),
            _size: size,
            _subs: subs,
            _live: Live::new("MediaView"),
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn native(&self) -> Option<&crate::native_media::NativeMedia> {
        self.native.as_ref()
    }

    /// The area a native viewer lies over, reported to the placer as it paints.
    #[cfg(target_os = "macos")]
    fn native_area(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let (id, view) = (cx.entity_id(), cx.weak_entity());
        gpui_kit::canvas(
            |bounds, window, _| window.insert_hitbox(bounds, gpui_kit::HitboxBehavior::Normal),
            move |bounds, hitbox, window, cx| {
                crate::native_media::painted(
                    crate::native_media::Painted {
                        id,
                        bounds,
                        hitbox,
                        view,
                    },
                    window,
                    cx,
                )
            },
        )
        .size_full()
        .into_any_element()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        // Zooming from fit starts at the size fit showed, roughly: 100%.
        let now = self.zoom.unwrap_or(1.0);
        self.zoom = Some((now * factor).clamp(0.05, 32.0));
        cx.notify();
    }

    fn render_image(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme();
        let source = ImageSource::Resource(Resource::Path(self.path.as_path().into()));
        let image = img(source).object_fit(ObjectFit::ScaleDown);
        let body = match (self.zoom, self.natural) {
            (Some(z), Some((w, h))) => div()
                .id("media-scroll")
                .size_full()
                .overflow_scroll()
                .flex()
                .items_center()
                .justify_center()
                .child(image.w(px(w as f32 * z)).h(px(h as f32 * z)).flex_none()),
            _ => div()
                .id("media-fit")
                .size_full()
                .p_4()
                .flex()
                .items_center()
                .justify_center()
                .child(image.max_w_full().max_h_full().size_full()),
        };
        let label = match (self.zoom, self.natural) {
            (Some(z), _) => format!("{}%", (z * 100.).round()),
            (None, _) => "Fit".to_string(),
        };
        let dims = self.natural.map(|(w, h)| format!("{w} × {h}"));
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                image_cache(self.images.clone())
                    .flex_1()
                    .min_h_0()
                    .bg(t.code(t.background))
                    .child(body),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .h(px(28.))
                    .border_t_1()
                    .border_color(t.border)
                    .text_size(px(11.5))
                    .text_color(t.muted_foreground)
                    .children(dims)
                    .child(div().flex_1())
                    .child(
                        Button::new("zoom-out")
                            .ghost()
                            .xsmall()
                            .label("−")
                            .on_click(
                                cx.listener(|this, _, _, cx| this.zoom_by(1. / ZOOM_STEP, cx)),
                            ),
                    )
                    .child(div().min_w(px(40.)).text_center().child(label))
                    .child(
                        Button::new("zoom-in")
                            .ghost()
                            .xsmall()
                            .label("+")
                            .on_click(cx.listener(|this, _, _, cx| this.zoom_by(ZOOM_STEP, cx))),
                    )
                    .child(
                        Button::new("fit")
                            .ghost()
                            .xsmall()
                            .label("Fit")
                            .toggle(self.zoom.is_none(), cx)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.zoom = None;
                                cx.notify();
                            })),
                    ),
            )
    }
}

impl Render for MediaView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let root = div()
            .id("media")
            .key_context("ImageView")
            .track_focus(&self.focus)
            .size_full()
            .on_action(cx.listener(|this, _: &media::ZoomIn, _, cx| this.zoom_by(ZOOM_STEP, cx)))
            .on_action(
                cx.listener(|this, _: &media::ZoomOut, _, cx| this.zoom_by(1. / ZOOM_STEP, cx)),
            )
            .on_action(cx.listener(|this, _: &media::Fit, _, cx| {
                this.zoom = None;
                cx.notify();
            }));
        if self.kind == MediaKind::Image {
            return root.child(self.render_image(cx));
        }
        let what = match self.kind {
            MediaKind::Audio => "Audio",
            MediaKind::Video => "Video",
            MediaKind::Pdf => "PDF",
            MediaKind::Image => unreachable!(),
        };
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let path = self.path.clone();
        #[cfg(target_os = "macos")]
        if self.native.is_some() {
            let open = path.clone();
            return root.child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .bg(t.code(t.background))
                            .child(self.native_area(cx)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .h(px(28.))
                            .border_t_1()
                            .border_color(t.border)
                            .text_size(px(11.5))
                            .text_color(t.muted_foreground)
                            .child(what)
                            .child(div().flex_1())
                            .child(
                                Button::new("open-default")
                                    .ghost()
                                    .xsmall()
                                    .label("Open in Default App")
                                    .on_click(move |_, _, cx| cx.open_with_system(&open)),
                            ),
                    ),
            );
        }
        root.child(
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .bg(t.code(t.background))
                .child(FileIcon::file(&name).size(px(40.)))
                .child(div().text_color(t.foreground).child(name.clone()))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(t.subtle)
                        .child(format!("{what} opens in its own app here.")),
                )
                .child(
                    Button::new("open-default")
                        .small()
                        .label("Open in Default App")
                        .on_click(move |_, _, cx| cx.open_with_system(&path)),
                ),
        )
    }
}
