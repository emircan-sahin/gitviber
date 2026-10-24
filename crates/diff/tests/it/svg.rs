//! A zoomed SVG preview draws the part on screen sharp, from the right part of the drawing and
//! no bigger than the screen, and gives it back once zoomed out.

use crate::review_fixes::window_with;
use gitviber_diff::{Compare, DiffOptions, DiffSource, ImageCompare, Layout};
use gpui_kit::{
    AppContext as _, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, point,
    px,
};
use std::sync::Arc;
use std::time::Duration;

#[gpui::test]
fn zooming_in_draws_the_part_on_screen(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        gitviber_diff::set_core(Arc::new(gitviber_core::Core::new(Arc::new(|_| {}))), cx);
        DiffOptions::set(
            DiffOptions {
                layout: Layout::Split,
                ..DiffOptions::default()
            },
            cx,
        );
    });
    // Red on the left half, blue on the right.
    let text = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="10" height="10" fill="#f00"/><rect x="10" width="10" height="10" fill="#00f"/></svg>"##;
    let (view, cx) = window_with(cx, |_, cx| {
        cx.new(|cx| {
            let mut v = ImageCompare::svg(DiffSource::new(Compare::Unstaged, "a.svg"), cx);
            v.set_svg_texts([Some(text.into()), Some(text.into())], cx);
            v
        })
    });
    cx.run_until_parked();
    let (size, density) = cx.update(|window, _| (window.viewport_size(), window.scale_factor()));
    let (w, h): (f32, f32) = (size.width.into(), size.height.into());
    // After's cell, beside Before's: the bar on top, a 16px margin and gap around them.
    let (cell_w, cell_h) = ((w - 48.) / 2., h - 28. - 32.);
    let fit = ((cell_w - 32.) / 20.).min((cell_h - 32.) / 10.);
    let left = cell_w + 32. + (cell_w - 20. * fit) / 2.;
    // Three quarters across the drawing: in its blue half.
    let at = point(px(left + 15. * fit), px(28. + 16. + cell_h / 2.));
    let wheel = |dy: f32| ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    };
    assert_eq!(view.read_with(cx, |v, _| v.crops()), [None, None]);
    cx.simulate_event(wheel(2000.));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    let crops = view.read_with(cx, |v, _| v.crops());
    for crop in crops {
        let ((cw, ch), bgra) = crop.expect("both sides zoomed, drawn sharp");
        // Only the part on screen: no bigger than the cell.
        assert!(cw as f32 <= (cell_w + 1.) * density && ch as f32 <= (cell_h + 1.) * density);
        assert_eq!(bgra, [255, 0, 0, 255], "the blue half");
    }
    // Zoomed back out, the whole drawing's pixels are enough: the parts go.
    cx.simulate_event(wheel(-4000.));
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |v, _| v.crops()), [None, None]);
}
