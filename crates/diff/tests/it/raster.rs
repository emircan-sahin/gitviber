//! A changed image: each side held no bigger than it's shown, both zoomed and moved as one,
//! drawn crisp from the right part past 100%, and given back once closed.

use crate::review_fixes::{git, setup, window_with};
use gitviber_diff::{Compare, DiffOptions, DiffSource, ImageCompare, Layout};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, Modifiers, MouseButton, MouseDownEvent,
    ParentElement as _, Render, ScrollDelta, ScrollWheelEvent, Styled as _, TestAppContext,
    TouchPhase, VisualTestContext, Window, div, point, px,
};
use std::path::PathBuf;
use std::time::Duration;

/// A `w` × `h` PNG: `left` on its left half, `right` on its right.
fn png(w: u32, h: u32, left: [u8; 4], right: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(w, h, |x, _| {
        image::Rgba(if x < w / 2 { left } else { right })
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

/// A repo whose a.png changed from `before` to `after`, not staged.
fn repo(name: &str, before: &[u8], after: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gitviber-diff-raster-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.name", "Test"]);
    git(&dir, &["config", "user.email", "test@example.com"]);
    std::fs::write(dir.join("a.png"), before).unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    std::fs::write(dir.join("a.png"), after).unwrap();
    dir
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
    }
}

#[gpui::test]
fn both_sides_zoom_and_move_together_and_go_crisp(cx: &mut TestAppContext) {
    let dir = repo(
        "zoom",
        &png(1200, 800, RED, GREEN),
        &png(1200, 800, RED, BLUE),
    );
    setup(cx, &dir);
    cx.update(|cx| {
        DiffOptions::set(
            DiffOptions {
                layout: Layout::Split,
                ..DiffOptions::default()
            },
            cx,
        )
    });
    let (view, cx) = window_with(cx, |_, cx| {
        cx.new(|cx| ImageCompare::new(DiffSource::new(Compare::Unstaged, "a.png"), cx))
    });
    settle(cx);
    let (size, density) = cx.update(|window, _| (window.viewport_size(), window.scale_factor()));
    let (w, h): (f32, f32) = (size.width.into(), size.height.into());
    // After's cell, beside Before's: the bar on top, a 16px margin and gap around them.
    let (cell_w, cell_h) = ((w - 48.) / 2., h - 28. - 32.);
    let fit = ((cell_w - 32.) / 1200.).min((cell_h - 32.) / 800.).min(1.);
    // Fitted, each side is held at the size it's shown: never its 1200 pixels.
    let k = (fit * density).min(1.);
    let shown = ((1200. * k).round() as u32, (800. * k).round() as u32);
    assert_eq!(view.read_with(cx, |v, _| v.held()), [Some(shown); 2]);
    assert_eq!(view.read_with(cx, |v, _| v.crops()), [None, None]);

    // ⌘-scroll three quarters across After: in its blue half, Before's green one.
    let left = cell_w + 32. + (cell_w - 1200. * fit) / 2.;
    let at = point(px(left + 900. * fit), px(28. + 16. + cell_h / 2.));
    let wheel = |dx: f32, dy: f32, modifiers: Modifiers| ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
        modifiers,
        touch_phase: TouchPhase::Moved,
    };
    cx.simulate_event(wheel(0., 2000., Modifiers::command()));
    settle(cx);
    let zoom = view.read_with(cx, |v, _| v.zoom());
    let scale = zoom.scale.expect("zoomed");
    assert!(scale > 4., "{scale}");
    let crops = view.read_with(cx, |v, _| v.crops());
    let colors: Vec<[u8; 4]> = crops
        .iter()
        .map(|c| {
            let ((cw, ch), bgra) = c.expect("both sides zoomed, drawn sharp");
            // Only the part on screen, at the display's density.
            assert!(cw as f32 <= (cell_w + 1.) * density && ch as f32 <= (cell_h + 1.) * density);
            bgra
        })
        .collect();
    assert_eq!(colors, [[0, 255, 0, 255], [255, 0, 0, 255]]);

    // The wheel alone moves both, and no further than the picture's edge.
    cx.simulate_event(wheel(100_000., 0., Modifiers::none()));
    settle(cx);
    let moved = view.read_with(cx, |v, _| v.zoom());
    assert_eq!(moved.scale, zoom.scale);
    let half = cell_w / 2. / (1200. * scale);
    assert!((moved.u - half).abs() < 1e-4, "{} {half}", moved.u);
    let crops = view.read_with(cx, |v, _| v.crops());
    // Both now show their left, red halves.
    for c in crops {
        assert_eq!(c.expect("still sharp").1, [0, 0, 255, 255]);
    }

    // A double-click fits again, and the crops go; another goes to 1:1.
    let click = |count: usize| MouseDownEvent {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::none(),
        click_count: count,
        first_mouse: false,
    };
    cx.simulate_event(click(2));
    settle(cx);
    assert_eq!(view.read_with(cx, |v, _| v.zoom().scale), None);
    assert_eq!(view.read_with(cx, |v, _| v.crops()), [None, None]);
    cx.simulate_event(click(2));
    settle(cx);
    assert_eq!(view.read_with(cx, |v, _| v.zoom().scale), Some(1.));
}

struct Host(Option<Entity<ImageCompare>>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().children(self.0.clone())
    }
}

#[gpui::test]
fn closing_an_image_compare_frees_it(cx: &mut TestAppContext) {
    let dir = repo(
        "close",
        &png(1600, 1000, RED, GREEN),
        &png(1600, 1000, RED, BLUE),
    );
    setup(cx, &dir);
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
    for _ in 0..3 {
        let view = cx.update(|_, cx| {
            cx.new(|cx| ImageCompare::new(DiffSource::new(Compare::Unstaged, "a.png"), cx))
        });
        host.update(cx, |h, cx| {
            h.0 = Some(view.clone());
            cx.notify();
        });
        settle(cx);
        assert!(view.read_with(cx, |v, _| v.held().iter().all(Option::is_some)));
        host.update(cx, |h, cx| {
            h.0 = None;
            cx.notify();
        });
        drop(view);
        settle(cx);
    }
    cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
}
