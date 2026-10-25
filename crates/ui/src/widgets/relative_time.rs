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

    struct Times {
        show: bool,
    }

    impl gpui_kit::Render for Times {
        fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
            use gpui_kit::prelude::FluentBuilder as _;
            div().when(self.show, |d| {
                d.child(RelativeTime::new(0)).child(RelativeTime::new(60))
            })
        }
    }

    /// Whether the timer is armed, and for how many windows.
    fn ticker(cx: &mut gpui_kit::TestAppContext) -> (bool, usize) {
        cx.update(|cx| {
            cx.try_global::<MinuteTicker>()
                .map_or((false, 0), |t| (t.task.is_some(), t.windows.len()))
        })
    }

    /// One timer for every window showing times, armed only while one does: at the minute it
    /// repaints them and they arm it again as they draw; once none shows a time it stays off.
    #[gpui_kit::test]
    fn one_minute_timer_runs_only_while_a_time_is_shown(cx: &mut gpui_kit::TestAppContext) {
        let minute = || Duration::from_secs(61);
        assert_eq!(ticker(cx), (false, 0));
        let (none, _) = cx.add_window_view(|_, _| Times { show: false });
        cx.run_until_parked();
        assert_eq!(ticker(cx), (false, 0), "no time shown, no timer");
        let (a, _) = cx.add_window_view(|_, _| Times { show: true });
        let (b, _) = cx.add_window_view(|_, _| Times { show: true });
        cx.run_until_parked();
        // Four times in two windows: one timer.
        assert_eq!(ticker(cx), (true, 2));
        cx.executor().advance_clock(minute());
        cx.run_until_parked();
        assert_eq!(ticker(cx), (true, 2), "drawn again, armed again");
        for view in [&a, &b] {
            view.update(cx, |t, cx| {
                t.show = false;
                cx.notify();
            });
        }
        cx.run_until_parked();
        cx.executor().advance_clock(minute());
        cx.run_until_parked();
        assert_eq!(ticker(cx), (false, 0), "the last time left");
        cx.executor().advance_clock(minute());
        cx.run_until_parked();
        assert_eq!(ticker(cx), (false, 0));
        // Shown again later: armed again, from that draw.
        none.update(cx, |t, cx| {
            t.show = true;
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(ticker(cx), (true, 1));
    }
}
