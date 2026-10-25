//! A changed image's two versions: side by side, as a swipe (a divider between them) or as an
//! onion skin (after faded in over before), sharing one zoom so they stay lined up (`stage`).
//! An image's file is held by this view only and decoded off the main thread no bigger than it's
//! shown (`raster`); closing the view takes its textures out of every window's atlas. An SVG's
//! Preview is drawn here too, from the texts its diff has.

pub mod raster;
mod stage;

use crate::model::zoom::Zoom;
use crate::source::DiffSource;
use crate::style::{DiffOptions, DiffStyle, ImageMode};
use gitviber_core::Core;
use gitviber_ui::workspace::Toast;
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::{
    App, AppContext as _, Bounds, Context, DevicePixels, FocusHandle, ImageFormat, IntoElement,
    ParentElement as _, Pixels, Point, RenderImage, Styled as _, Subscription, SvgRenderer,
    SvgSize, Task, WeakEntity, Window, div, px, size,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

/// The image formats the compare view decodes.
pub fn image_format(path: &str) -> Option<ImageFormat> {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    Some(match ext.as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "gif" => ImageFormat::Gif,
        "webp" => ImageFormat::Webp,
        "bmp" => ImageFormat::Bmp,
        "ico" => ImageFormat::Ico,
        "tif" | "tiff" => ImageFormat::Tiff,
        _ => return None,
    })
}

struct Version {
    /// The whole picture as held: an SVG's drawn as it loads, an image's at the size its room
    /// fits it (None until the room is known), an animation's frame now.
    image: Option<Arc<RenderImage>>,
    bytes: usize,
    /// Of the file's bytes: the same again isn't decoded again.
    hash: u64,
    /// Its own size, which 1:1 shows it at.
    natural: (f32, f32),
    /// An image's file, decoded again for a zoomed part; an SVG's text is `texts`.
    raster: Option<Arc<[u8]>>,
    animated: bool,
}

/// What an SVG preview is drawn on: art in the theme's own color vanishes on the theme's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backdrop {
    #[default]
    Theme,
    Light,
    Dark,
}

/// An SVG's longer side is drawn this many device pixels long, whatever its own size: sharp
/// when fitted to a pane, and two of them stay under ~45 MB. Zoomed past it, only the part on
/// screen is drawn again (`stage`).
const SVG_MIN_SIDE: f32 = 1600.;
const SVG_MAX_SIDE: f32 = 2400.;

fn rasterize_svg(svg: &SvgRenderer, text: &str, hash: u64) -> Side {
    let Some((w, h)) = crate::model::svg::intrinsic_size(text) else {
        return Side::Failed("Not an SVG".into());
    };
    let long = w.max(h);
    let k = (long * 2.).clamp(SVG_MIN_SIDE, SVG_MAX_SIDE) / long;
    let px = |v: f32| DevicePixels((v * k).round().max(1.) as i32);
    let exact = SvgSize::ExactSize(size(px(w), px(h)));
    match svg
        .parse_svg(text.as_bytes())
        .and_then(|parsed| svg.render_parsed(&parsed, exact))
    {
        Ok(image) => Side::Ready(Version {
            image: Some(image),
            bytes: text.len(),
            hash,
            natural: (w, h),
            raster: None,
            animated: false,
        }),
        Err(e) => Side::Failed(format!("Could not draw this SVG: {e}")),
    }
}

/// Past this the compressed file isn't read in.
const MAX_BYTES: usize = 64 * 1024 * 1024;

fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

enum Side {
    Loading,
    Missing,
    Ready(Version),
    Failed(String),
    /// What a refresh found: the same bytes as the version shown.
    Same,
}

pub struct ImageCompare {
    core: Arc<Core>,
    source: DiffSource,
    sides: [Side; 2],
    /// Swipe's divider, and onion skin's opacity of After: kept apart, as a divider near the
    /// edge isn't a faint onion skin.
    divider: f32,
    opacity: f32,
    dragging: bool,
    stage: Rc<Cell<Bounds<Pixels>>>,
    /// Drawn from texts handed in (`set_svg_texts`), not read from git; kept to draw a zoomed
    /// part again.
    svg: bool,
    texts: [Option<Arc<str>>; 2],
    /// Both sides' zoom, so they stay lined up.
    zoom: Zoom,
    backdrop: Backdrop,
    /// Where each side was drawn last frame (one stage for both when laid over each other).
    rooms: [Rc<Cell<Bounds<Pixels>>>; 2],
    /// The part on screen of a zoomed side, drawn sharp, and what the last one asked for was.
    crops: [Option<stage::Crop>; 2],
    asked: [Option<stage::CropKey>; 2],
    /// The size each image side's whole picture was last asked at.
    sized: [Option<(u32, u32)>; 2],
    /// An animated side's frames, and when its next one is due.
    players: [Option<(raster::Player, Instant)>; 2],
    /// Set as a side paints: a hidden view's animations stop until it's drawn again.
    painted: Rc<Cell<bool>>,
    playing: bool,
    /// The pointer while a drag moves the drawing.
    pan: Option<Point<Pixels>>,
    /// Onion skin's fade track, where it was drawn.
    fade: Rc<Cell<Bounds<Pixels>>>,
    focus: FocusHandle,
    _load: Task<()>,
    _copy: Task<()>,
    _sized: [Task<()>; 2],
    _crop: Task<()>,
    _tick: Task<()>,
    _release: Subscription,
}

impl ImageCompare {
    pub fn new(source: DiffSource, cx: &mut Context<Self>) -> Self {
        let mut view = ImageCompare::empty(source, false, cx);
        view.load(cx);
        view
    }

    /// An SVG's preview, drawn from the texts `set_svg_texts` gives it.
    pub fn svg(source: DiffSource, cx: &mut Context<Self>) -> Self {
        ImageCompare::empty(source, true, cx)
    }

    fn empty(source: DiffSource, svg: bool, cx: &mut Context<Self>) -> Self {
        ImageCompare {
            core: crate::core(cx),
            source,
            sides: [Side::Loading, Side::Loading],
            divider: 0.5,
            opacity: 0.5,
            dragging: false,
            stage: Rc::default(),
            svg,
            texts: [None, None],
            zoom: Zoom::default(),
            backdrop: Backdrop::Theme,
            rooms: Default::default(),
            crops: [None, None],
            asked: [None, None],
            sized: [None, None],
            players: [None, None],
            painted: Rc::default(),
            playing: false,
            pan: None,
            fade: Rc::default(),
            focus: cx.focus_handle(),
            _load: Task::ready(()),
            _copy: Task::ready(()),
            _sized: [Task::ready(()), Task::ready(())],
            _crop: Task::ready(()),
            _tick: Task::ready(()),
            _release: cx.on_release(|this, cx| this.drop_images(cx)),
        }
    }

    /// Reads both versions again (the file changed on disk).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.svg {
            self.load(cx);
        }
    }

    /// The SVG's text before and after (None: not on that side); the same text isn't drawn again.
    pub fn set_svg_texts(&mut self, texts: [Option<String>; 2], cx: &mut Context<Self>) {
        let kept = texts.clone().map(|t| t.map(Arc::from));
        if kept != self.texts {
            self.texts = kept;
            self.drop_crops(cx);
        }
        let svg = cx.svg_renderer();
        let shown = self.shown_hashes();
        self._load = cx.spawn(async move |this, cx| {
            let sides = cx
                .background_spawn(async move {
                    let [before, after] = texts;
                    [(before, shown[0]), (after, shown[1])].map(|(text, shown)| match text {
                        None => Side::Missing,
                        Some(t) if shown == Some(hash(t.as_bytes())) => Side::Same,
                        Some(t) if t.len() > MAX_BYTES => {
                            Side::Failed("Too large to preview".into())
                        }
                        Some(t) => rasterize_svg(&svg, &t, hash(t.as_bytes())),
                    })
                })
                .await;
            this.update(cx, |this, cx| this.take_sides(sides, cx)).ok();
        });
    }

    pub fn zoom(&self) -> Zoom {
        self.zoom
    }

    pub fn set_zoom(&mut self, zoom: Zoom, cx: &mut Context<Self>) {
        self.zoom = zoom;
        cx.notify();
    }

    pub fn backdrop(&self) -> Backdrop {
        self.backdrop
    }

    pub fn set_backdrop(&mut self, backdrop: Backdrop, cx: &mut Context<Self>) {
        self.backdrop = backdrop;
        cx.notify();
    }

    /// Both versions are there to lay one over the other.
    pub fn has_both(&self) -> bool {
        !matches!(self.sides, [Side::Missing, _] | [_, Side::Missing])
    }

    fn shown_hashes(&self) -> [Option<u64>; 2] {
        [0, 1].map(|i| match &self.sides[i] {
            Side::Ready(v) => Some(v.hash),
            _ => None,
        })
    }

    fn take_sides(&mut self, sides: [Side; 2], cx: &mut Context<Self>) {
        for (i, side) in sides.into_iter().enumerate() {
            if matches!(side, Side::Same) {
                continue;
            }
            if let Side::Ready(old) = std::mem::replace(&mut self.sides[i], side)
                && let Some(image) = old.image
            {
                // Dropped once this update is over: the window it runs in is out of the app's list
                // until then, and would keep the picture.
                cx.defer(move |cx| cx.drop_image(image, None));
            }
            self.sized[i] = None;
            self._sized[i] = Task::ready(());
            self.players[i] = None;
            if let Some(crop) = self.crops[i].take() {
                cx.defer(move |cx| cx.drop_image(crop.image, None));
            }
            self.asked[i] = None;
        }
        cx.notify();
    }

    fn drop_images(&mut self, cx: &mut App) {
        self.players = [None, None];
        for side in &mut self.sides {
            if let Side::Ready(v) = std::mem::replace(side, Side::Loading)
                && let Some(image) = v.image
            {
                cx.defer(move |cx| cx.drop_image(image, None));
            }
        }
        self.drop_crops(cx);
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        if image_format(&self.source.path).is_none() {
            return;
        }
        let (core, source) = (self.core.clone(), self.source.clone());
        let shown = self.shown_hashes();
        self._load = cx.spawn(async move |this, cx| {
            let sides = cx
                .background_spawn(async move {
                    let c = &source.compare;
                    [(true, shown[0]), (false, shown[1])].map(|(original, shown)| {
                        let bytes = gitviber_core::api::files::media(
                            &core,
                            c.kind().to_string(),
                            source.path.clone(),
                            source.old_path.clone(),
                            c.sha().map(str::to_string),
                            c.base().map(str::to_string),
                            original,
                        );
                        match bytes {
                            Ok(b) if b.is_empty() => Side::Missing,
                            Ok(b) if shown == Some(hash(&b)) => Side::Same,
                            Ok(b) if b.len() > MAX_BYTES => {
                                Side::Failed("Too large to preview".into())
                            }
                            Ok(b) => match raster::probe(&b) {
                                Ok(p) => Side::Ready(Version {
                                    image: None,
                                    bytes: b.len(),
                                    hash: hash(&b),
                                    natural: (p.size.0 as f32, p.size.1 as f32),
                                    raster: Some(b.into()),
                                    animated: p.animated,
                                }),
                                Err(e) => Side::Failed(e),
                            },
                            // A side that isn't there (added, deleted) reads as an error.
                            Err(_) => Side::Missing,
                        }
                    })
                })
                .await;
            this.update(cx, |this, cx| this.take_sides(sides, cx)).ok();
        });
    }

    /// What a right-click on side `i` offers (None: the stage both are laid over): Copy Image
    /// for a side, each version's on the stage. Labels, and Before's (true) or After's. Only on
    /// macOS, where a file goes onto the pasteboard.
    #[doc(hidden)]
    pub fn copies(&self, i: Option<usize>) -> Vec<(&'static str, bool)> {
        if !cfg!(target_os = "macos") {
            return vec![];
        }
        let there = |i: usize| !matches!(self.sides[i], Side::Missing);
        match i {
            Some(i) if there(i) => vec![("Copy Image", i == 0)],
            Some(_) => vec![],
            None => [("Copy Before Image", 0), ("Copy After Image", 1)]
                .into_iter()
                .filter(|&(_, i)| there(i))
                .map(|(label, i)| (label, i == 0))
                .collect(),
        }
    }

    /// The file of one version to copy: the working tree's own, or a stored one saved first.
    #[doc(hidden)]
    pub fn copy_file(&self, before: bool) -> impl Future<Output = Result<String, String>> + use<> {
        let (core, source) = (self.core.clone(), self.source.clone());
        async move {
            let c = &source.compare;
            gitviber_core::api::files::media_file(
                &core,
                c.kind().to_string(),
                source.path.clone(),
                source.old_path.clone(),
                c.sha().map(str::to_string),
                c.base().map(str::to_string),
                before,
            )
        }
    }

    /// One version onto the pasteboard as Finder copies a file: it pastes into Finder, Slack or
    /// a terminal, and an app that only takes a picture gets its picture.
    fn copy_image(&mut self, before: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (file, core) = (self.copy_file(before), self.core.clone());
        self._copy = cx.spawn_in(window, async move |_, cx| {
            let copied = cx
                .background_spawn(async move {
                    gitviber_core::api::app::copy_files(&core, vec![file.await?])
                })
                .await;
            if let Err(e) = copied {
                _ = cx
                    .update(|window, cx| Toast::error(e).title("Could not copy").show(window, cx));
            }
        });
    }

    /// The right-click menu of side `i` (None: the stage).
    fn copy_menu(&self, i: Option<usize>, menu: PopupMenu, view: WeakEntity<Self>) -> PopupMenu {
        self.copies(i)
            .into_iter()
            .fold(menu, |menu, (label, before)| {
                let view = view.clone();
                menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    _ = view.update(cx, |this, cx| this.copy_image(before, window, cx));
                }))
            })
    }

    fn drag(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let b = self.stage.get();
        if b.size.width <= px(0.) {
            return;
        }
        let at = ((x - b.origin.x) / b.size.width).clamp(0., 1.);
        if DiffOptions::get(cx).image == ImageMode::Onion {
            self.opacity = at;
        } else {
            self.divider = at;
        }
        cx.notify();
    }
}

fn details(v: &Side) -> String {
    match v {
        Side::Ready(v) => {
            let (w, h) = v.natural;
            let size = raster::file_size(v.bytes as u64);
            format!("{}×{} · {size}", w.round(), h.round())
        }
        Side::Failed(e) => e.clone(),
        _ => String::new(),
    }
}

impl ImageCompare {
    /// 2-up until both versions are there to lay one over the other.
    fn mode(&self, cx: &App) -> ImageMode {
        if matches!(self.sides, [Side::Ready(_), Side::Ready(_)]) {
            DiffOptions::get(cx).image
        } else {
            ImageMode::Side
        }
    }

    /// Before's and After's sizes, and `middle` between them.
    fn bar(&self, style: &DiffStyle, middle: Option<impl IntoElement>) -> gpui_kit::Div {
        let c = &style.colors;
        let label = |text: &'static str, color, d: String| {
            h_flex()
                .gap_2()
                .child(div().text_color(color).child(text))
                .child(div().text_color(c.subtle).child(d))
        };
        h_flex()
            .h(px(28.))
            .flex_none()
            .px_3()
            .gap_2()
            .border_b_1()
            .border_color(c.border)
            .text_size(style.ui_font_size - px(0.5))
            .child(label("Before", c.removed, details(&self.sides[0])))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .justify_center()
                    .text_color(c.subtle)
                    .children(middle),
            )
            .child(label("After", c.added, details(&self.sides[1])))
    }
}

impl gpui_kit::Render for ImageCompare {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_stage(window, cx)
    }
}
