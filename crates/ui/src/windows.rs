//! What every GitViber window shares: its options (our own title bar, the traffic lights in it),
//! the background launch, and the app commands that work from any window.

use gpui_kit::component::TitleBar;
use gpui_kit::{App, Bounds, Pixels, Size, WindowBounds, WindowOptions, point, px, size};

use crate::actions::{app, window as win};
use crate::state::WindowGeometry;
use crate::theme::ActiveAppTheme;

/// GITVIBER_BACKGROUND=1: the window opens behind the others without taking the keyboard, and
/// the app doesn't come to the front. For screenshots taken while someone types elsewhere.
pub fn background_launch() -> bool {
    std::env::var("GITVIBER_BACKGROUND").is_ok_and(|v| v == "1")
}

/// The top bar's height; the traffic lights sit in its middle.
pub const TOP_BAR_HEIGHT: Pixels = px(40.);

/// Options for a GitViber window of `default_size`, at `bounds` when given.
pub fn options(
    bounds: Option<WindowBounds>,
    default_size: Size<Pixels>,
    min_size: Size<Pixels>,
    cx: &App,
) -> WindowOptions {
    let background = background_launch();
    let mut titlebar = TitleBar::title_bar_options();
    // Centered in the top bar: macOS draws them 14px tall at this inset.
    titlebar.traffic_light_position = Some(point(px(14.), (TOP_BAR_HEIGHT - px(14.)) / 2.));
    WindowOptions {
        window_bounds: Some(
            bounds.unwrap_or_else(|| {
                WindowBounds::Windowed(Bounds::centered(None, default_size, cx))
            }),
        ),
        titlebar: Some(titlebar),
        app_owns_titlebar_drag: true,
        focus: !background,
        show: true,
        window_min_size: Some(min_size),
        window_background: cx.app_theme().window_background(),
        app_id: Some(crate::store::APP_ID.into()),
        ..Default::default()
    }
}

/// Saved geometry as window bounds, when it still lands on a connected display: fitted to the
/// one it's most on, so a window left big on an external display opens whole on the laptop's.
pub fn restore_bounds(
    saved: Option<&WindowGeometry>,
    min_size: Size<Pixels>,
    cx: &App,
) -> Option<WindowBounds> {
    let g = saved?;
    let bounds = Bounds::new(
        point(px(g.x), px(g.y)),
        size(
            px(g.width.max(f32::from(min_size.width))),
            px(g.height.max(f32::from(min_size.height))),
        ),
    );
    let on = |area: Bounds<Pixels>| {
        let b = area.intersect(&bounds);
        f32::from(b.size.width).max(0.) * f32::from(b.size.height).max(0.)
    };
    let display = cx
        .displays()
        .into_iter()
        .filter(|d| d.bounds().intersects(&bounds))
        .max_by(|a, b| on(a.bounds()).total_cmp(&on(b.bounds())))?;
    let bounds = fit(bounds, display.visible_bounds(), min_size);
    Some(if g.fullscreen {
        WindowBounds::Fullscreen(bounds)
    } else if g.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    })
}

/// `bounds` shrunk to fit `area` (a display's, less the menu bar and Dock) and moved into it;
/// never under `min`.
fn fit(bounds: Bounds<Pixels>, area: Bounds<Pixels>, min: Size<Pixels>) -> Bounds<Pixels> {
    let w = bounds.size.width.min(area.size.width).max(min.width);
    let h = bounds.size.height.min(area.size.height).max(min.height);
    let x = bounds.origin.x.min(area.right() - w).max(area.left());
    let y = bounds.origin.y.min(area.bottom() - h).max(area.top());
    Bounds::new(point(x, y), size(w, h))
}

/// Window bounds as they're saved.
pub fn geometry(bounds: WindowBounds) -> WindowGeometry {
    let (b, maximized, fullscreen) = match bounds {
        WindowBounds::Windowed(b) => (b, false, false),
        WindowBounds::Maximized(b) => (b, true, false),
        WindowBounds::Fullscreen(b) => (b, false, true),
    };
    WindowGeometry {
        x: f32::from(b.origin.x),
        y: f32::from(b.origin.y),
        width: f32::from(b.size.width),
        height: f32::from(b.size.height),
        maximized,
        fullscreen,
    }
}

/// Quit, hide and the window commands, from any window; the app quits with its last window.
pub fn install_app_actions(cx: &mut App) {
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            crate::state::flush(cx);
            cx.quit();
        }
    })
    .detach();
    cx.on_action(|_: &app::Quit, cx| {
        crate::state::flush(cx);
        cx.quit();
    });
    cx.on_action(|_: &app::Hide, cx| cx.hide());
    cx.on_action(|_: &app::HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &app::ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &win::Minimize, cx| {
        if let Some(w) = cx.active_window() {
            let _ = w.update(cx, |_, window, _| window.minimize_window());
        }
    });
    cx.on_action(|_: &win::Zoom, cx| {
        if let Some(w) = cx.active_window() {
            let _ = w.update(cx, |_, window, _| window.zoom_window());
        }
    });
    cx.on_action(|_: &win::ToggleFullScreen, cx| {
        if let Some(w) = cx.active_window() {
            let _ = w.update(cx, |_, window, _| window.toggle_fullscreen());
        }
    });
    cx.on_action(|_: &win::BringAllToFront, cx| cx.activate(true));
    cx.on_app_quit(|cx| {
        crate::state::flush(cx);
        async {}
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    #[test]
    fn a_window_bigger_than_its_display_is_shrunk_into_it() {
        let min = size(px(900.), px(560.));
        // The laptop's 1512×982, under a 33pt menu bar; the window left at 2400×1300.
        let laptop = b(0., 33., 1512., 949.);
        assert_eq!(fit(b(200., 100., 2400., 1300.), laptop, min), laptop);
        // One that fits stays where it was; one hanging off the edge comes in.
        assert_eq!(
            fit(b(100., 80., 1000., 700.), laptop, min),
            b(100., 80., 1000., 700.)
        );
        assert_eq!(
            fit(b(900., 600., 1000., 700.), laptop, min),
            b(512., 282., 1000., 700.)
        );
        // A display smaller than the window's minimum: the minimum, from its corner.
        assert_eq!(
            fit(b(0., 0., 1200., 800.), b(0., 0., 800., 500.), min),
            b(0., 0., 900., 560.)
        );
    }
}
