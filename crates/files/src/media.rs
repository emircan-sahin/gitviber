//! A file that isn't text: an image fitted to the view or zoomed (raster.rs), PDFs, audio and
//! video in macOS's own viewers (native_media.rs), elsewhere a card that opens them in the
//! default app.

use std::path::{Path, PathBuf};

use gitviber_ui::ActiveAppTheme as _;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::FileIcon;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{
    AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, Window, div, px,
};

use crate::raster::ImagePreview;

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
        "png" | "apng" | "jpg" | "jpeg" | "jfif" | "gif" | "webp" | "bmp" | "ico" | "tif"
        | "tiff" | "tga" | "qoi" | "pbm" | "pgm" | "ppm" | "pnm" | "exr" | "dds" | "svg" => {
            MediaKind::Image
        }
        "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" | "oga" | "opus" => MediaKind::Audio,
        "mp4" | "m4v" | "mov" | "webm" | "ogv" | "mkv" | "avi" => MediaKind::Video,
        "pdf" => MediaKind::Pdf,
        _ => return None,
    })
}

pub struct MediaView {
    /// The file on disk.
    path: PathBuf,
    kind: MediaKind,
    /// An image's preview, whose focus is this view's.
    image: Option<Entity<ImagePreview>>,
    focus: FocusHandle,
    /// A PDF's, a video's or a song's native viewer (macOS); None draws the open-elsewhere card.
    #[cfg(target_os = "macos")]
    native: Option<crate::native_media::NativeMedia>,
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
        let image = (kind == MediaKind::Image).then(|| {
            let path = path.clone();
            cx.new(|cx| ImagePreview::new(path, cx))
        });
        let focus = match &image {
            Some(image) => image.read(cx).focus_handle().clone(),
            None => cx.focus_handle(),
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
            image,
            focus,
            _subs: subs,
            _live: Live::new("MediaView"),
        }
    }

    #[cfg(test)]
    pub(crate) fn image(&self) -> Option<&Entity<ImagePreview>> {
        self.image.as_ref()
    }

    /// The file changed on disk: an image is read again.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if let Some(image) = &self.image {
            image.update(cx, |i, cx| i.reload(cx));
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
}

impl Render for MediaView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        if let Some(image) = &self.image {
            return div().id("media").size_full().child(image.clone());
        }
        let root = div().id("media").track_focus(&self.focus).size_full();
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
