//! A rendered SVG file in the zoom stage the image preview shares (`zoom`): fitted to the view
//! or zoomed and moved, on a checkerboard of the theme's background or a light or dark one for
//! art drawn in a color that vanishes on the theme's. Drawn from the text, so typing not saved
//! yet shows.
//!
//! Only the part on screen is rasterized, at the display's density, off the main thread: the
//! root element's viewBox is rewritten to that part, so a zoomed drawing stays sharp and its
//! pixels never outgrow the view. The image is this view's own, dropped when replaced.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use gitviber_diff::model::zoom::Frame;
use gitviber_ui::memory::Live;
use gpui_kit::{
    App, AppContext as _, Context, DevicePixels, FocusHandle, IntoElement, ParentElement as _,
    Render, SharedString, SvgSize, Task, Window, size,
};

use crate::zoom::{self, Drawn, Zoomable, Zoomer};

/// Zooming or panning settles this long before the sharp picture is drawn again.
const SETTLE: Duration = Duration::from_millis(90);

/// The root `<svg …>` start tag's byte range, past the prolog, comments and doctype.
fn root_tag(text: &str) -> Option<Range<usize>> {
    let mut at = 0;
    loop {
        let rest = &text[at..];
        let start = at + rest.find('<')?;
        let tail = &text[start..];
        let skip = if tail.starts_with("<?") {
            tail.find("?>").map(|e| e + 2)
        } else if tail.starts_with("<!--") {
            tail.find("-->").map(|e| e + 3)
        } else if tail.starts_with("<!") {
            // A doctype's internal subset may hold '>'.
            let close = tail
                .find('[')
                .filter(|b| tail.find('>').is_some_and(|g| *b < g));
            match close {
                Some(_) => tail.find("]>").map(|e| e + 2),
                None => tail.find('>').map(|e| e + 1),
            }
        } else {
            None
        };
        if let Some(skip) = skip {
            at = start + skip;
            continue;
        }
        let name_end = tail[1..]
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .map_or(tail.len(), |e| e + 1);
        let name = &tail[1..name_end];
        if name != "svg" && !name.ends_with(":svg") {
            return None;
        }
        let mut quote = None;
        for (i, c) in tail.char_indices() {
            match (quote, c) {
                (None, '"' | '\'') => quote = Some(c),
                (Some(q), c) if c == q => quote = None,
                (None, '>') => return Some(start..start + i + 1),
                _ => {}
            }
        }
        return None;
    }
}

/// A start tag's attributes: name, value, and the byte range of the whole `name="value"`.
fn attributes(tag: &str) -> Vec<(&str, &str, Range<usize>)> {
    static ATTR: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"([A-Za-z_:][\w:.-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap()
    });
    ATTR.captures_iter(tag)
        .filter_map(|c| {
            let all = c.get(0)?;
            let value = c.get(2).or(c.get(3))?;
            Some((c.get(1)?.as_str(), value.as_str(), all.range()))
        })
        .collect()
}

/// A length in CSS pixels; None for a percentage or what doesn't parse.
fn length(text: &str) -> Option<f32> {
    let t = text.trim();
    let split = t
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(t.len());
    let n: f32 = t[..split].trim().parse().ok()?;
    let per = match t[split..].trim() {
        "" | "px" => 1.,
        "pt" => 4. / 3.,
        "pc" => 16.,
        "in" => 96.,
        "cm" => 96. / 2.54,
        "mm" => 96. / 25.4,
        "em" => 16.,
        "ex" => 8.,
        _ => return None,
    };
    (n.is_finite() && n > 0.).then_some(n * per)
}

/// What the root element says about its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Root {
    /// The size it draws at on its own, in CSS pixels.
    pub natural: (f32, f32),
    /// Its viewBox: user space shown in `natural`.
    pub view_box: Option<[f32; 4]>,
    /// preserveAspectRatio="none": the viewBox stretches to the size.
    pub stretch: bool,
}

/// The root's size the way the renderer reads it: width and height, else the viewBox's (one
/// missing follows its aspect), else 100 × 100.
pub(crate) fn root_size(text: &str) -> Option<Root> {
    let range = root_tag(text)?;
    let attrs = attributes(&text[range]);
    let get = |name: &str| attrs.iter().find(|a| a.0 == name).map(|a| a.1);
    let view_box = get("viewBox").and_then(|v| {
        let n: Vec<f32> = v
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        (n.len() == 4 && n[2] > 0. && n[3] > 0.).then(|| [n[0], n[1], n[2], n[3]])
    });
    let (w, h) = (
        get("width").and_then(length),
        get("height").and_then(length),
    );
    let natural = match (w, h, view_box) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some(b)) => (w, w * b[3] / b[2]),
        (None, Some(h), Some(b)) => (h * b[2] / b[3], h),
        (None, None, Some(b)) => (b[2], b[3]),
        (w, h, None) => (w.unwrap_or(100.), h.unwrap_or(100.)),
    };
    let stretch = get("preserveAspectRatio").is_some_and(|p| p.trim() == "none");
    Some(Root {
        natural,
        view_box,
        stretch,
    })
}

/// `text` with its root showing only `part` (CSS pixels of the drawing at its natural size),
/// `device` pixels big.
pub(crate) fn cropped(
    text: &str,
    root: &Root,
    part: [f32; 4],
    device: (u32, u32),
) -> Option<String> {
    let range = root_tag(text)?;
    let tag = &text[range.clone()];
    let (nw, nh) = root.natural;
    // From the drawing's pixels to the root's user space, as the renderer maps it (centered meet).
    let to_user = |x: f32, y: f32| -> (f32, f32, f32, f32) {
        let Some([bx, by, bw, bh]) = root.view_box else {
            return (x, y, 1., 1.);
        };
        if root.stretch {
            return (bx + x * bw / nw, by + y * bh / nh, bw / nw, bh / nh);
        }
        let s = (nw / bw).min(nh / bh);
        let (tx, ty) = ((nw - bw * s) / 2., (nh - bh * s) / 2.);
        (bx + (x - tx) / s, by + (y - ty) / s, 1. / s, 1. / s)
    };
    let (ux, uy, sx, sy) = to_user(part[0], part[1]);
    let view_box = format!("{ux} {uy} {} {}", part[2] * sx, part[3] * sy);
    let mut out = String::with_capacity(text.len() + 128);
    out.push_str(&text[..range.start]);
    let mut last = 0;
    for (name, _, at) in attributes(tag) {
        if matches!(name, "width" | "height" | "viewBox" | "preserveAspectRatio") {
            out.push_str(&tag[last..at.start]);
            last = at.end;
        }
    }
    let close = if tag.ends_with("/>") { 2 } else { 1 };
    out.push_str(&tag[last..tag.len() - close]);
    out.push_str(&format!(
        " width=\"{}\" height=\"{}\" viewBox=\"{view_box}\" preserveAspectRatio=\"none\"",
        device.0, device.1
    ));
    out.push_str(&tag[tag.len() - close..]);
    out.push_str(&text[range.end..]);
    Some(out)
}

fn no_root(root: Option<Root>) -> Option<SharedString> {
    root.is_none()
        .then(|| "It has no <svg> element at its root".into())
}

pub struct SvgPreview {
    source: Arc<str>,
    root: Option<Root>,
    z: Zoomer,
    /// The part on screen, drawn last.
    drawn: Option<Drawn>,
    /// The part and device size asked of the renderer last.
    asked: Option<([f32; 4], (u32, u32))>,
    broken: Option<SharedString>,
    _render: Task<()>,
    _live: Live,
}

impl Zoomable for SvgPreview {
    const GROW: bool = true;

    fn zoomer(&self) -> &Zoomer {
        &self.z
    }

    fn zoomer_mut(&mut self) -> &mut Zoomer {
        &mut self.z
    }

    fn natural(&self) -> Option<(f32, f32)> {
        self.root.map(|r| r.natural)
    }
}

impl SvgPreview {
    pub fn new(source: &str, cx: &mut Context<Self>) -> Self {
        cx.on_release(|this: &mut Self, cx| this.drop_image(cx))
            .detach();
        let root = root_size(source);
        SvgPreview {
            source: source.into(),
            broken: no_root(root),
            root,
            z: Zoomer::new(cx),
            drawn: None,
            asked: None,
            _render: Task::ready(()),
            _live: Live::new("SvgPreview"),
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.z.focus
    }

    /// Draws other text (typed into, or changed on disk); the zoom stays.
    pub fn set_source(&mut self, source: &str, cx: &mut Context<Self>) {
        if *self.source == *source {
            return;
        }
        self.source = source.into();
        self.root = root_size(source);
        self.asked = None;
        self.broken = no_root(self.root);
        cx.notify();
    }

    fn drop_image(&mut self, cx: &mut App) {
        if let Some(d) = self.drawn.take() {
            cx.drop_image(d.image, None);
        }
    }

    /// Asks for the part on screen at the display's density, once the view settles.
    fn draw_visible(&mut self, f: &Frame, window: &Window, cx: &mut Context<Self>) {
        let Some(root) = self.root else { return };
        let s = f.scale(self.z.zoom);
        let [x, y, w, h] = f.drawn(self.z.zoom);
        let (x0, y0) = (x.max(0.), y.max(0.));
        let (x1, y1) = ((x + w).min(f.room.0), (y + h).min(f.room.1));
        if x1 <= x0 || y1 <= y0 || s <= 0. {
            return;
        }
        let sf = window.scale_factor();
        let device = (
            ((x1 - x0) * sf).round().max(1.) as u32,
            ((y1 - y0) * sf).round().max(1.) as u32,
        );
        let part = [(x0 - x) / s, (y0 - y) / s, (x1 - x0) / s, (y1 - y0) / s];
        if self.asked == Some((part, device)) {
            return;
        }
        self.asked = Some((part, device));
        let (renderer, source, wait) =
            (cx.svg_renderer(), self.source.clone(), self.drawn.is_some());
        self._render = cx.spawn(async move |this, cx| {
            if wait {
                cx.background_executor().timer(SETTLE).await;
            }
            let drawn = cx
                .background_spawn(async move {
                    let text = cropped(&source, &root, part, device).unwrap_or_default();
                    let parsed = renderer.parse_svg(text.as_bytes())?;
                    let px = |n: u32| DevicePixels(n as i32);
                    renderer.render_parsed(
                        &parsed,
                        SvgSize::ExactSize(size(px(device.0), px(device.1))),
                    )
                })
                .await;
            _ = this.update(cx, |this, cx| {
                this.drop_image(cx);
                match drawn {
                    Ok(image) => {
                        this.broken = None;
                        this.drawn = Some(Drawn { image, part });
                    }
                    Err(e) => this.broken = Some(e.to_string().into()),
                }
                cx.notify();
            });
        });
    }
}

impl Render for SvgPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(f) = self.frame() {
            self.draw_visible(&f, window, cx);
        }
        let over = self.broken.clone().map(|problem| {
            let detail = format!("{problem}. Switch to Code to see it.");
            zoom::problem("This SVG can't be rendered", detail, cx)
        });
        let details = self
            .root
            .map(|r| format!("{} × {}", r.natural.0.round(), r.natural.1.round()));
        zoom::root(self, cx)
            .child(zoom::stage(self, &self.drawn, over, cx))
            .child(zoom::bar(self, details, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Styled as _, div};

    #[test]
    fn the_root_size_follows_width_height_and_view_box() {
        let size = |s: &str| root_size(s).map(|r| r.natural);
        let doc = "<?xml version=\"1.0\"?>\n<!-- x > y -->\n<!DOCTYPE svg [<!ENTITY a \"b>\">]>\n";
        assert_eq!(
            size(&format!("{doc}<svg width=\"24\" height=\"12pt\">")),
            Some((24., 16.))
        );
        assert_eq!(
            size("<svg viewBox='0 0 48 24' width='100%'/>"),
            Some((48., 24.))
        );
        assert_eq!(
            size("<svg viewBox=\"0,0,48,24\" width=\"24\">"),
            Some((24., 12.))
        );
        assert_eq!(
            size("<svg xmlns='http://www.w3.org/2000/svg'>"),
            Some((100., 100.))
        );
        assert_eq!(size("<html><svg width='5'>"), None);
        assert_eq!(size("<svg:svg width='5' height='6'>"), Some((5., 6.)));
    }

    #[test]
    fn a_crop_rewrites_only_the_roots_size() {
        let text = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"50\" viewBox=\"0 0 200 100\"><rect width=\"9\"/></svg>";
        let root = root_size(text).unwrap();
        let out = cropped(text, &root, [50., 0., 50., 25.], (200, 100)).unwrap();
        assert_eq!(
            out,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"    width=\"200\" height=\"100\" viewBox=\"100 0 100 50\" preserveAspectRatio=\"none\"><rect width=\"9\"/></svg>"
        );
        // No viewBox: user space is the drawing's pixels. A letterboxed one is centered.
        let plain = root_size("<svg width='10' height='10'/>").unwrap();
        let out = cropped(
            "<svg width='10' height='10'/>",
            &plain,
            [2., 3., 4., 5.],
            (8, 10),
        )
        .unwrap();
        assert!(
            out.ends_with("viewBox=\"2 3 4 5\" preserveAspectRatio=\"none\"/>"),
            "{out}"
        );
        let wide = "<svg width='200' height='100' viewBox='0 0 100 100'>";
        let out = cropped(
            wide,
            &root_size(wide).unwrap(),
            [50., 0., 100., 100.],
            (1, 1),
        )
        .unwrap();
        assert!(out.contains("viewBox=\"0 0 100 100\""), "{out}");
    }

    struct Host(Option<gpui_kit::Entity<SvgPreview>>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().children(self.0.clone())
        }
    }

    #[gpui_kit::test]
    fn the_part_on_screen_is_drawn_and_given_back(cx: &mut gpui_kit::TestAppContext) {
        let dir = std::env::temp_dir().join(format!("gitviber-files-test-{}", std::process::id()));
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(gitviber_ui::init);
        let (host, cx) = cx.add_window_view(|_, _| Host(None));
        let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
        let svg = "<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'><rect width='40' height='20' fill='red'/></svg>";
        for _ in 0..2 {
            let view = cx.update(|_, cx| cx.new(|cx| SvgPreview::new(svg, cx)));
            host.update(cx, |h, cx| {
                h.0 = Some(view.clone());
                cx.notify();
            });
            for _ in 0..3 {
                cx.update(|window, cx| window.draw(cx).clear(cx));
                cx.run_until_parked();
            }
            let (drawn, scale, broken) = view.read_with(cx, |v, _| {
                (
                    v.drawn.as_ref().map(|d| (d.image.size(0), d.part)),
                    v.frame().map(|f| f.scale(v.z.zoom)),
                    v.broken.clone(),
                )
            });
            assert_eq!(broken, None);
            let (pixels, part) = drawn.expect("drawn");
            // Fit: the whole drawing, at its size on screen.
            assert_eq!(part, [0., 0., 40., 20.]);
            let sf = cx.update(|window, _| window.scale_factor());
            assert_eq!(
                i32::from(pixels.width),
                (40. * scale.unwrap() * sf).round() as i32
            );
            // Broken markup says so.
            view.update(cx, |v, cx| v.set_source("<svg><g></svg>", cx));
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.executor().advance_clock(SETTLE * 2);
            cx.run_until_parked();
            assert!(view.read_with(cx, |v, _| v.broken.is_some()));
            host.update(cx, |h, cx| {
                h.0 = None;
                cx.notify();
            });
            drop(view);
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        }
        cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
    }
}
