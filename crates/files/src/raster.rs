//! An image file (PNG, JPEG, GIF, WebP, BMP, ICO, TIFF, …) in the zoom stage the SVG preview
//! shares (`zoom`), decoded off the main thread no bigger than it's shown
//! (`gitviber_diff::raster`): the whole image at the size fitting shows it and, zoomed past
//! that, the part on screen at the display's density, crisp above 100%. An animation plays a
//! frame at a time while it's drawn. Its textures are this view's own, given back as they're
//! replaced and when it closes; nothing else of the file is kept.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use gitviber_diff::model::zoom::{FIT, Frame, Zoom};
use gitviber_diff::raster::{self, Next, Player, Probe};
use gitviber_ui::memory::Live;
use gpui_kit::{
    App, AppContext as _, Context, FocusHandle, IntoElement, ParentElement as _, Render,
    RenderImage, SharedString, Task, Window,
};

use crate::zoom::{self, Drawn, Zoomable, Zoomer};

/// Zooming or moving settles this long before the part on screen is decoded again.
const SETTLE: Duration = Duration::from_millis(90);
/// How soon to look again for an animation's frame that isn't decoded yet.
const RETRY: Duration = Duration::from_millis(15);
/// Past this a file isn't read in to be shown.
const MAX_BYTES: u64 = 256 << 20;

/// The file's length and modification time, as read: a refresh that finds them the same
/// decodes nothing.
type Stamp = (u64, Option<SystemTime>);

/// What a whole-image request gives back.
enum Whole {
    Still(Arc<RenderImage>),
    Playing(Option<Player>),
}

pub struct ImagePreview {
    path: PathBuf,
    probe: Option<Probe>,
    stamp: Option<Stamp>,
    z: Zoomer,
    /// The whole image at the size fitting shows it (an animation's frame now).
    base: Option<Drawn>,
    /// The part on screen, sharp, once zoomed past what `base` holds.
    crop: Option<Drawn>,
    /// The size `base` was last asked at, and the crop last asked for.
    sized: Option<(u32, u32)>,
    asked: Option<[i32; 6]>,
    player: Option<Player>,
    playing: bool,
    broken: Option<SharedString>,
    _probe: Task<()>,
    _base: Task<()>,
    _crop: Task<()>,
    _tick: Task<()>,
    _live: Live,
}

impl Zoomable for ImagePreview {
    const GROW: bool = false;

    fn zoomer(&self) -> &Zoomer {
        &self.z
    }

    fn zoomer_mut(&mut self) -> &mut Zoomer {
        &mut self.z
    }

    fn natural(&self) -> Option<(f32, f32)> {
        self.probe.map(|p| (p.size.0 as f32, p.size.1 as f32))
    }
}

/// The file's stamp and what its header says; None while it's still `known`.
fn read(path: &Path, known: Option<Stamp>) -> Option<(Option<Stamp>, Result<Probe, String>)> {
    let meta = std::fs::metadata(path);
    let stamp = meta.as_ref().ok().map(|m| (m.len(), m.modified().ok()));
    if stamp.is_some() && stamp == known {
        return None;
    }
    let probe = match meta {
        Err(e) => Err(e.to_string()),
        Ok(m) if m.len() > MAX_BYTES => Err("Too large to preview".into()),
        Ok(_) => std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|b| raster::probe(&b)),
    };
    Some((stamp, probe))
}

impl ImagePreview {
    pub fn new(path: PathBuf, cx: &mut Context<Self>) -> Self {
        cx.on_release(|this: &mut Self, cx| this.drop_images(cx))
            .detach();
        let mut view = ImagePreview {
            path,
            probe: None,
            stamp: None,
            z: Zoomer::new(cx),
            base: None,
            crop: None,
            sized: None,
            asked: None,
            player: None,
            playing: false,
            broken: None,
            _probe: Task::ready(()),
            _base: Task::ready(()),
            _crop: Task::ready(()),
            _tick: Task::ready(()),
            _live: Live::new("ImagePreview"),
        };
        view.reload(cx);
        view
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.z.focus
    }

    /// The whole image's pixels as held, and whether a crop is (tests).
    #[cfg(test)]
    pub(crate) fn held(&self) -> (Option<(i32, i32)>, bool) {
        let size = self.base.as_ref().map(|b| b.image.size(0));
        (size.map(|s| (s.width.0, s.height.0)), self.crop.is_some())
    }

    /// Fitted (None), or `scale` view pixels to one of the image's, centered.
    pub fn zoom_to(&mut self, scale: Option<f32>, cx: &mut Context<Self>) {
        self.set_zoom(Zoom { scale, ..FIT }, cx);
    }

    /// Reads the file again if it changed on disk; the zoom stays.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let (path, known) = (self.path.clone(), self.stamp);
        self._probe = cx.spawn(async move |this, cx| {
            let read = cx.background_spawn(async move { read(&path, known) }).await;
            if let Some(read) = read {
                this.update(cx, |this, cx| this.probed(read, cx)).ok();
            }
        });
    }

    fn probed(
        &mut self,
        (stamp, probe): (Option<Stamp>, Result<Probe, String>),
        cx: &mut Context<Self>,
    ) {
        self.drop_images(cx);
        self.stamp = stamp;
        self.sized = None;
        self.asked = None;
        self._base = Task::ready(());
        self._crop = Task::ready(());
        match probe {
            Ok(p) => {
                self.probe = Some(p);
                self.broken = None;
            }
            Err(e) => {
                self.probe = None;
                self.broken = Some(e.into());
            }
        }
        cx.notify();
    }

    fn drop_images(&mut self, cx: &mut App) {
        self.player = None;
        for d in [self.base.take(), self.crop.take()].into_iter().flatten() {
            // Dropped once this update is over: the window it runs in is out of the app's list
            // until then, and would keep the picture.
            cx.defer(move |cx| cx.drop_image(d.image, None));
        }
    }

    fn set_base(&mut self, image: Arc<RenderImage>, cx: &mut Context<Self>) {
        let Some(p) = self.probe else { return };
        let part = [0., 0., p.size.0 as f32, p.size.1 as f32];
        if let Some(old) = self.base.replace(Drawn { image, part }) {
            cx.defer(move |cx| cx.drop_image(old.image, None));
        }
        cx.notify();
    }

    /// Asks for the whole image `want` big: at once the first time, once the view settles
    /// after; an animation's player starts at that size.
    fn ask_base(&mut self, want: (u32, u32), animated: bool, cx: &mut Context<Self>) {
        self.sized = Some(want);
        let (path, wait) = (self.path.clone(), self.base.is_some());
        self._base = cx.spawn(async move |this, cx| {
            if wait {
                cx.background_executor().timer(SETTLE).await;
            }
            let whole = cx
                .background_spawn(async move {
                    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                    if animated {
                        Ok(Whole::Playing(Player::start(bytes.into(), want)))
                    } else {
                        raster::whole(&bytes, want).map(Whole::Still)
                    }
                })
                .await;
            this.update(cx, |this, cx| match whole {
                Ok(Whole::Still(image)) => this.set_base(image, cx),
                Ok(Whole::Playing(player)) => {
                    this.player = player;
                    this.playing = false;
                    this.play(cx);
                }
                Err(e) => {
                    this.drop_images(cx);
                    this.broken = Some(e.into());
                    cx.notify();
                }
            })
            .ok();
        });
    }

    /// The whole image at the size fitting shows it, asked again when that changes by more
    /// than a tenth; zoomed past it, the part on screen once the view settles.
    fn sync(&mut self, f: &Frame, window: &Window, cx: &mut Context<Self>) {
        let Some(p) = self.probe else { return };
        let density = window.scale_factor();
        let k = (f.fit() * density).min(1.);
        let side = |n: u32| ((n as f32 * k).round() as u32).max(1);
        let want = (side(p.size.0), side(p.size.1));
        let close = self
            .sized
            .is_some_and(|h| (h.0 as f32 - want.0 as f32).abs() <= (want.0 as f32 * 0.1).max(2.));
        if !close {
            self.ask_base(want, p.animated, cx);
        }
        let zoom = self.z.zoom;
        let s = f.scale(zoom);
        let [x, y, ..] = f.drawn(zoom);
        let (x0, y0) = ((-x / s).max(0.), (-y / s).max(0.));
        let (x1, y1) = (
            ((f.room.0 - x) / s).min(p.size.0 as f32),
            ((f.room.1 - y) / s).min(p.size.1 as f32),
        );
        // An animation is drawn whole; a still image is cropped once its whole picture is in.
        let crop = self
            .base
            .as_ref()
            .filter(|_| !p.animated && x1 > x0 && y1 > y0)
            .and_then(|b| {
                let held = b.image.size(0).width.0 as u32;
                raster::crop_for(p.size, held, s, density, [x0, y0, x1 - x0, y1 - y0])
            });
        let Some(crop) = crop else {
            if let Some(old) = self.crop.take() {
                cx.defer(move |cx| cx.drop_image(old.image, None));
            }
            self.asked = None;
            self._crop = Task::ready(());
            return;
        };
        if self.asked == Some(crop.key()) {
            return;
        }
        self.asked = Some(crop.key());
        let path = self.path.clone();
        self._crop = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let drawn = cx
                .background_spawn(async move {
                    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                    raster::draw(&bytes, &crop)
                })
                .await;
            this.update(cx, |this, cx| {
                if let Ok(image) = drawn {
                    let part = crop.part;
                    if let Some(old) = this.crop.replace(Drawn { image, part }) {
                        cx.defer(move |cx| cx.drop_image(old.image, None));
                    }
                    cx.notify();
                }
            })
            .ok();
        });
    }

    /// Plays the animation while it's drawn.
    fn play(&mut self, cx: &mut Context<Self>) {
        if self.playing || self.player.is_none() {
            return;
        }
        self.playing = true;
        self._tick = cx.spawn(async move |this, cx| {
            loop {
                let Ok(Some(wait)) = this.update(cx, |this, cx| this.advance(cx)) else {
                    break;
                };
                cx.background_executor().timer(wait).await;
            }
        });
    }

    /// Shows the next frame and says how long it stays. None stops: the animation ended, or
    /// its last frame wasn't drawn (the tab is hidden; the next render plays on).
    fn advance(&mut self, cx: &mut Context<Self>) -> Option<Duration> {
        let next = self
            .player
            .as_ref()
            .filter(|_| self.z.painted.get())
            .map(Player::next);
        match next {
            Some(Next::Frame(image, delay)) => {
                self.z.painted.set(false);
                self.set_base(image, cx);
                Some(delay)
            }
            Some(Next::Wait) => Some(RETRY),
            Some(Next::Done) | None => {
                if matches!(next, Some(Next::Done)) {
                    self.player = None;
                }
                self.playing = false;
                None
            }
        }
    }
}

impl Render for ImagePreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(f) = self.frame() {
            self.sync(&f, window, cx);
        }
        self.play(cx);
        let over = self
            .broken
            .clone()
            .map(|e| zoom::problem("This image can't be shown", format!("{e}."), cx));
        let details = self.probe.map(|p| {
            let (w, h) = p.size;
            match self.stamp {
                Some((len, _)) => format!("{w} × {h} · {}", raster::file_size(len)),
                None => format!("{w} × {h}"),
            }
        });
        zoom::root(self, cx)
            .child(zoom::stage(
                self,
                self.base.iter().chain(&self.crop),
                over,
                cx,
            ))
            .child(zoom::bar(self, details, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        Entity, Modifiers, MouseButton, MouseDownEvent, Styled as _, TestAppContext,
        VisualTestContext, div, point,
    };

    struct Host(Option<Entity<ImagePreview>>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().children(self.0.clone())
        }
    }

    /// A `w` × `h` PNG, red on its left half and blue on its right, in a temporary file.
    pub(crate) fn halves(name: &str, w: u32, h: u32) -> PathBuf {
        let path = gitviber_ui::test_support::temp_dir("files-png").join(format!("{name}.png"));
        image::RgbaImage::from_fn(w, h, |x, _| {
            image::Rgba(if x < w / 2 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            })
        })
        .save(&path)
        .unwrap();
        path
    }

    fn settle(cx: &mut VisualTestContext) {
        for _ in 0..3 {
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
            cx.executor().advance_clock(SETTLE * 2);
            cx.run_until_parked();
        }
    }

    /// The BGRA at (x, y) of a drawn picture.
    fn pixel(d: &Drawn, x: usize, y: usize) -> [u8; 4] {
        let w = d.image.size(0).width.0 as usize;
        let b = d.image.as_bytes(0).unwrap();
        let i = (y * w + x) * 4;
        [b[i], b[i + 1], b[i + 2], b[i + 3]]
    }

    /// An animation plays frame after frame while it's drawn, stops while its tab is hidden,
    /// plays on once shown again, and its frames' thread ends with it.
    #[gpui_kit::test]
    fn an_animation_plays_only_while_shown(cx: &mut TestAppContext) {
        use image::codecs::gif::GifEncoder;
        gitviber_ui::test_support::config_dir("files-test");
        cx.update(gitviber_ui::init);
        let path = gitviber_ui::test_support::temp_dir("files-png").join("anim.gif");
        {
            let mut enc = GifEncoder::new(std::fs::File::create(&path).unwrap());
            for c in [[255, 0, 0, 255], [0, 0, 255, 255]] {
                let frame = image::Frame::from_parts(
                    image::RgbaImage::from_pixel(32, 32, image::Rgba(c)),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(50, 1),
                );
                enc.encode_frame(frame).unwrap();
            }
        }
        let (host, cx) = cx.add_window_view(|_, _| Host(None));
        let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
        let view = cx.update(|_, cx| cx.new(|cx| ImagePreview::new(path.clone(), cx)));
        let show = |on: bool, cx: &mut VisualTestContext| {
            let v = view.clone();
            host.update(cx, |h, cx| {
                h.0 = on.then_some(v);
                cx.notify();
            });
        };
        // Frames come from a thread in real time; the view's clock is the test's.
        let run = |cx: &mut VisualTestContext| {
            let mut seen = Vec::new();
            for _ in 0..40 {
                cx.update(|window, cx| window.draw(cx).clear(cx));
                cx.run_until_parked();
                cx.executor().advance_clock(Duration::from_millis(60));
                cx.run_until_parked();
                std::thread::sleep(Duration::from_millis(5));
                let px = view.read_with(cx, |v, _| v.base.as_ref().map(|b| pixel(b, 1, 1)));
                if let Some(px) = px
                    && seen.last() != Some(&px)
                {
                    seen.push(px);
                }
            }
            seen
        };
        show(true, cx);
        let seen = run(cx);
        assert!(seen.len() >= 3, "{seen:?}");
        assert!(view.read_with(cx, |v, _| v.playing));
        // Hidden: the next frame isn't drawn, so the player waits.
        show(false, cx);
        run(cx);
        assert!(!view.read_with(cx, |v, _| v.playing));
        let still = view.read_with(cx, |v, _| v.base.as_ref().map(|b| b.image.id));
        run(cx);
        assert_eq!(
            view.read_with(cx, |v, _| v.base.as_ref().map(|b| b.image.id)),
            still
        );
        show(true, cx);
        assert!(run(cx).len() >= 2);
        show(false, cx);
        drop(view);
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
        std::fs::remove_file(path).ok();
    }

    /// ⌘= / ⌘− / ⌘0 zoom the picture where focus is on it, not the window.
    #[gpui_kit::test]
    fn the_window_zoom_keys_zoom_the_picture(cx: &mut TestAppContext) {
        gitviber_ui::test_support::config_dir("files-test");
        cx.update(gitviber_ui::init);
        let path = halves("keys", 1600, 1200);
        let (host, cx) = cx.add_window_view(|_, _| Host(None));
        let view = cx.update(|_, cx| cx.new(|cx| ImagePreview::new(path.clone(), cx)));
        host.update(cx, |h, cx| {
            h.0 = Some(view.clone());
            cx.notify();
        });
        settle(cx);
        cx.update(|window, cx| {
            let focus = view.read(cx).focus_handle().clone();
            focus.focus(window, cx);
        });
        let scale = |cx: &mut VisualTestContext| view.read_with(cx, |v, _| v.z.zoom.scale);
        let cmd = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        cx.simulate_keystrokes(&format!("{cmd}-="));
        let once = scale(cx).expect("zoomed in");
        cx.simulate_keystrokes(&format!("{cmd}-="));
        let twice = scale(cx).unwrap();
        assert!(twice > once, "{once} {twice}");
        cx.simulate_keystrokes(&format!("{cmd}--"));
        assert!(scale(cx).unwrap() < twice);
        cx.simulate_keystrokes(&format!("{cmd}-0"));
        assert_eq!(view.read_with(cx, |v, _| v.z.zoom), FIT);
        std::fs::remove_file(path).ok();
    }

    #[gpui_kit::test]
    fn held_at_the_size_shown_and_drawn_crisp_zoomed_in(cx: &mut TestAppContext) {
        gitviber_ui::test_support::config_dir("files-test");
        cx.update(gitviber_ui::init);
        let path = halves("zoom", 1600, 1200);
        let (host, cx) = cx.add_window_view(|_, _| Host(None));
        let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
        let view = cx.update(|_, cx| cx.new(|cx| ImagePreview::new(path.clone(), cx)));
        host.update(cx, |h, cx| {
            h.0 = Some(view.clone());
            cx.notify();
        });
        settle(cx);
        let sf = cx.update(|window, _| window.scale_factor());
        let frame = view.read_with(cx, |v, _| v.frame()).expect("laid out");
        assert!(frame.fit() < 1.);
        // Fitted: held at the size it's shown, never its 1600 pixels; nothing more.
        let k = (frame.fit() * sf).min(1.);
        let held = view.read_with(cx, |v, _| v.held());
        let side = |n: f32| (n * k).round() as i32;
        assert_eq!(held, (Some((side(1600.), side(1200.))), false));

        // 400%, centered: the part on screen at the display's density, red then blue.
        view.update(cx, |v, cx| v.zoom_to(Some(4.), cx));
        settle(cx);
        view.read_with(cx, |v, _| {
            let crop = v.crop.as_ref().expect("zoomed in, drawn sharp");
            let s = crop.image.size(0);
            let (w, h) = (s.width.0 as f32, s.height.0 as f32);
            assert_eq!(w, (crop.part[2] * 4. * sf).round());
            assert!(w <= frame.room.0 * sf + 1. && h <= frame.room.1 * sf + 1.);
            let (cw, ch) = (w as usize, h as usize);
            assert_eq!(pixel(crop, cw / 4, ch / 2), [0, 0, 255, 255]);
            assert_eq!(pixel(crop, cw * 3 / 4, ch / 2), [255, 0, 0, 255]);
        });

        // Moving stops at the image's edge.
        view.update(cx, |v, cx| v.pan_by((1e6, 0.), cx));
        let left = view.read_with(cx, |v, _| {
            let f = v.frame().unwrap();
            f.drawn(v.z.zoom)[0]
        });
        assert!(left.abs() < 0.01, "{left}");

        // A double-click fits again and gives the crop back, out of the window's atlas too: it
        // goes while the window draws.
        let crop = view.read_with(cx, |v, _| v.crop.as_ref().unwrap().image.clone());
        assert!(cx.update(|window, _| window.has_image_atlas_entry(&crop)));
        let middle = cx.update(|_, cx| {
            let b = view.read(cx).z.stage.get();
            b.origin + point(b.size.width / 2., b.size.height / 2.)
        });
        cx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: middle,
            modifiers: Modifiers::none(),
            click_count: 2,
            first_mouse: false,
        });
        settle(cx);
        assert_eq!(view.read_with(cx, |v, _| v.z.zoom), FIT);
        assert!(view.read_with(cx, |v, _| v.crop.is_none()));
        assert!(!cx.update(|window, _| window.has_image_atlas_entry(&crop)));
        drop(crop);

        host.update(cx, |h, cx| {
            h.0 = None;
            cx.notify();
        });
        drop(view);
        settle(cx);
        cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
        std::fs::remove_file(path).ok();
    }
}
