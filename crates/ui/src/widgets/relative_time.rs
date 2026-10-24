//! "5m ago", kept current. One timer for the whole app, armed only while some window shows a
//! time: it fires at the next minute and repaints those windows, which re-arm it as they draw.

use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui_kit::{
    AnyWindowHandle, App, Global, IntoElement, ParentElement, RenderOnce, SharedString, Task,
    Window, div,
};

/// "3h ago", from a Unix time in seconds, as of `now`.
pub fn relative_time(unix_seconds: i64, now: i64) -> String {
    let s = (now - unix_seconds).max(0);
    const STEPS: [(i64, &str); 6] = [
        (31_557_600, "y"),
        (2_629_800, "mo"),
        (604_800, "w"),
        (86_400, "d"),
        (3_600, "h"),
        (60, "m"),
    ];
    for (secs, unit) in STEPS {
        if s >= secs {
            return format!("{}{unit} ago", s / secs);
        }
    }
    "just now".into()
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A Unix time in the user's local time with the time of day, for a tooltip.
pub fn full_date(unix_seconds: i64) -> String {
    use chrono::TimeZone as _;
    match chrono::Local.timestamp_opt(unix_seconds, 0) {
        chrono::LocalResult::Single(t) => t.format("%x %X").to_string(),
        _ => String::new(),
    }
}

#[derive(Default)]
struct MinuteTicker {
    windows: HashSet<AnyWindowHandle>,
    task: Option<Task<()>>,
}

impl Global for MinuteTicker {}

/// Repaints `window` at the next minute boundary.
pub fn repaint_next_minute(window: &Window, cx: &mut App) {
    let ticker = cx.default_global::<MinuteTicker>();
    ticker.windows.insert(window.window_handle());
    if ticker.task.is_some() {
        return;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let wait = Duration::from_secs(60 - now.as_secs() % 60) + Duration::from_millis(50);
    let timer = cx.background_executor().timer(wait);
    let task = cx.spawn(async move |cx| {
        timer.await;
        cx.update(|cx| {
            let ticker = cx.global_mut::<MinuteTicker>();
            ticker.task = None;
            let windows: Vec<_> = ticker.windows.drain().collect();
            for handle in windows {
                let _ = handle.update(cx, |_, window, _| window.refresh());
            }
        });
    });
    cx.global_mut::<MinuteTicker>().task = Some(task);
}

/// "5m ago" for a Unix time in seconds, kept current while on screen.
#[derive(IntoElement)]
pub struct RelativeTime {
    unix: i64,
}

impl RelativeTime {
    pub fn new(unix_seconds: i64) -> Self {
        Self { unix: unix_seconds }
    }
}

impl RenderOnce for RelativeTime {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        repaint_next_minute(window, cx);
        div().child(SharedString::from(relative_time(self.unix, now_unix())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_down_to_minutes() {
        assert_eq!(relative_time(1000, 1030), "just now");
        assert_eq!(relative_time(0, 60 * 5), "5m ago");
        assert_eq!(relative_time(0, 3600 * 3), "3h ago");
        assert_eq!(relative_time(0, 86_400 * 8), "1w ago");
        assert_eq!(relative_time(100, 0), "just now");
    }
}
