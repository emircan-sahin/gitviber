//! Each window opens where and as large as it was left (tauri-plugin-window-state). A position on a
//! monitor that's gone is left to the OS; a size bigger than the monitor shrinks to it. Never its
//! visibility: the page shows the window once its theme applies.

use tauri::plugin::TauriPlugin;
use tauri::{PhysicalSize, Runtime, WebviewWindow};
use tauri_plugin_window_state::StateFlags;

const FLAGS: StateFlags = StateFlags::SIZE
    .union(StateFlags::POSITION)
    .union(StateFlags::MAXIMIZED);

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    let builder = tauri_plugin_window_state::Builder::new().with_state_flags(FLAGS);
    // Windows' maximize (SW_MAXIMIZE) shows the window, unstyled: restored() and shown() do it in two.
    #[cfg(windows)]
    let builder = builder
        .skip_initial_state("main")
        .skip_initial_state(crate::settings_window::LABEL);
    builder.build()
}

/// A window just built, still hidden.
pub fn restored<R: Runtime>(window: &WebviewWindow<R>) {
    #[cfg(windows)]
    {
        use tauri_plugin_window_state::WindowExt;
        let _ = window.restore_state(StateFlags::SIZE | StateFlags::POSITION);
        maximize_pending()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(window.label().to_string());
    }
    let _ = fit(window);
}

/// Its page showed it: on Windows, it's maximized now if it was left so.
pub fn shown<R: Runtime>(window: &WebviewWindow<R>) {
    #[cfg(windows)]
    {
        use tauri_plugin_window_state::WindowExt;
        let pending = maximize_pending()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(window.label());
        if pending {
            let _ = window.restore_state(StateFlags::MAXIMIZED);
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

#[cfg(windows)]
fn maximize_pending() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static PENDING: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    PENDING.get_or_init(Default::default)
}

/// Shrinks a window left on a bigger monitor (a 5K display, then the laptop) to its monitor's work area.
fn fit<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    let Some(monitor) = window.current_monitor()?.or(window.primary_monitor()?) else {
        return Ok(());
    };
    let (outer, inner) = (window.outer_size()?, window.inner_size()?);
    if let Some(size) = fitted(outer, inner, monitor.work_area().size) {
        window.set_size(size)?;
    }
    Ok(())
}

/// The inner size that makes `outer` fit in `area`, or None when it fits already.
fn fitted(
    outer: PhysicalSize<u32>,
    inner: PhysicalSize<u32>,
    area: PhysicalSize<u32>,
) -> Option<PhysicalSize<u32>> {
    if outer.width <= area.width && outer.height <= area.height {
        return None;
    }
    // The frame (title bar, borders) is outside the inner size.
    let frame_w = outer.width.saturating_sub(inner.width);
    let frame_h = outer.height.saturating_sub(inner.height);
    Some(PhysicalSize::new(
        outer.width.min(area.width).saturating_sub(frame_w),
        outer.height.min(area.height).saturating_sub(frame_h),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_bigger_than_its_monitor_shrinks_to_it() {
        let size = PhysicalSize::new;
        // Fits: left alone.
        assert_eq!(
            fitted(size(1480, 948), size(1480, 920), size(2880, 1750)),
            None
        );
        // From a 5K display onto a laptop: each side no bigger than the work area, frame included.
        assert_eq!(
            fitted(size(5000, 2800), size(5000, 2772), size(2880, 1750)),
            Some(size(2880, 1722))
        );
        // Only too wide: the height stays.
        assert_eq!(
            fitted(size(3000, 1000), size(2990, 972), size(2880, 1750)),
            Some(size(2870, 972))
        );
    }
}
