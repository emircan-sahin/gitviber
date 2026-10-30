//! Updates as the app shows them: a newer release's notes, its download and the restart, the
//! Settings page's status and the top bar's badge. The updater itself comes with packaging
//! and plugs in with `set_updater`; until one does (and in a development build) updates are off,
//! Check for Updates is greyed out, and the page says so.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use futures::channel::oneshot;
use gitviber_ui::actions::app;
use gitviber_ui::memory::Live;
use gitviber_ui::widgets::relative_time;
use gitviber_ui::workspace::Toast;
use gitviber_ui::{ActiveAppTheme, Settings};
use gpui_kit::assets::IconName;
use gpui_kit::base::text::TextView;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonRounded, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::progress::ProgressCircle;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::{
    App, AppContext as _, Context, Global, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Task,
    Window, div, prelude::FluentBuilder as _, px, relative, rems,
};

use crate::help::{REPO, open_url, toast};

/// How often a running app looks again, besides at launch and when the Updates page opens: by
/// the wall clock, in seconds.
const EVERY: i64 = 4 * 60 * 60;
/// Timers don't run while the Mac sleeps: a short tick finds a check overdue soon after a wake.
const TICK: Duration = Duration::from_secs(30 * 60);
/// After a check that got no answer (the network not up yet at launch, a wake from sleep): again
/// this soon, twice as long after each one that fails, up to TICK.
const RETRY: Duration = Duration::from_secs(10);
/// Opening the Updates page checks, but not again within this many seconds of the last check.
const PAGE_AGAIN: i64 = 60;

/// The wait before the next tick: TICK once a check answered, else the retry's.
fn next_wait(answered: bool, failed_before: u32) -> Duration {
    if answered {
        return TICK;
    }
    RETRY.saturating_mul(1 << failed_before.min(16)).min(TICK)
}

/// Whether a timed check is due at `now`: none answered yet, or the last answer is EVERY old. A
/// clock set back counts as due, or it would wait out the difference.
fn due(checked_at: Option<i64>, now: i64) -> bool {
    checked_at.is_none_or(|at| now < at || now - at >= EVERY)
}

/// Whether opening the Updates page checks: no check started in the last PAGE_AGAIN seconds.
fn page_due(asked_at: Option<i64>, now: i64) -> bool {
    asked_at.is_none_or(|at| now < at || now - at >= PAGE_AGAIN)
}

/// Unix seconds, the wall clock's: it goes on while the Mac sleeps.
fn now() -> i64 {
    #[cfg(test)]
    if let Some(now) = tests::NOW.get() {
        return now;
    }
    gitviber_ui::widgets::now_unix()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    /// RFC 3339, as GitHub dates a release.
    pub date: Option<String>,
    /// Markdown.
    pub notes: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// It downloads the update and restarts into it.
    Install,
    /// This copy can't replace itself (a package manager's): the Releases page has it.
    Download,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Download {
    /// Going, at a percent when the size is known.
    Running(Option<u8>),
    /// Downloaded: a restart puts it in.
    Ready,
}

/// Told a download's percent as it goes, None while the size isn't known.
pub type Progress = Rc<dyn Fn(Option<u8>, &mut App)>;

/// What packaging provides: GitHub Releases asked, a download, a restart into it.
pub trait Updater {
    fn mode(&self) -> Mode;
    /// The newer release, or None when this one is the newest.
    fn check(&self, cx: &mut App) -> Task<Result<Option<Release>, String>>;
    /// Downloads `release`, telling `progress` as it goes.
    fn download(
        &self,
        release: &Release,
        progress: Progress,
        cx: &mut App,
    ) -> Task<Result<(), String>>;
    /// Quits and starts the downloaded version.
    fn restart(&self, cx: &mut App);
}

#[derive(Default)]
pub struct Updates {
    updater: Option<Rc<dyn Updater>>,
    release: Option<Release>,
    checking: bool,
    /// Unix seconds of the last check that answered.
    checked_at: Option<i64>,
    /// Unix seconds of the last check started, answered or not.
    asked_at: Option<i64>,
    download: Option<Download>,
    /// Why the last download failed; the badge offers to try again.
    failed: Option<String>,
    restarting: bool,
    _check: Option<Task<()>>,
    _download: Option<Task<()>>,
    _timer: Option<Task<()>>,
}

impl Global for Updates {}

impl Updates {
    fn get(cx: &mut App) -> &mut Updates {
        cx.default_global::<Updates>()
    }

    /// What a check found. A release being downloaded or waiting for its restart stays: it's
    /// what the restart installs, and the button that restarts goes with it.
    fn checked(&mut self, found: Option<Release>, at: i64) {
        self.checked_at = Some(at);
        if self.download.is_none() || self.release.is_none() {
            if self.release.as_ref().map(|r| &r.version) != found.as_ref().map(|r| &r.version) {
                self.failed = None;
            }
            self.release = found;
        }
    }
}

/// Plugs the updater in: Check for Updates applies from then on, the app checks at launch and
/// every few hours while Settings allows, on ticks that find a check due by the wall clock.
pub fn set_updater(updater: Rc<dyn Updater>, cx: &mut App) {
    let first = cx.default_global::<Updates>().updater.is_none();
    cx.global_mut::<Updates>().updater = Some(updater);
    if first {
        cx.on_action(|_: &app::CheckForUpdates, cx| check(true, cx));
        let timer = cx.spawn(async move |cx| {
            let mut failed = 0;
            loop {
                let answer = cx.update(|cx| {
                    let due = due(Updates::get(cx).checked_at, now());
                    (due && Settings::get(cx).auto_update).then(|| {
                        let (tx, rx) = oneshot::channel();
                        run_check(false, Some(tx), cx);
                        rx
                    })
                });
                // Not asked (not due, off, or one already running): taken as answered.
                let answered = match answer {
                    Some(answer) => answer.await.unwrap_or(true),
                    None => true,
                };
                let wait = next_wait(answered, failed);
                failed = if answered { 0 } else { failed + 1 };
                cx.background_executor().timer(wait).await;
            }
        });
        cx.global_mut::<Updates>()._timer = Some(timer);
    }
}

/// Whether this build updates itself at all.
pub fn enabled(cx: &App) -> bool {
    cx.try_global::<Updates>()
        .is_some_and(|u| u.updater.is_some())
}

/// A newer release is out.
pub fn available(cx: &App) -> bool {
    cx.try_global::<Updates>()
        .is_some_and(|u| u.release.is_some())
}

/// A check is out and hasn't answered yet.
pub fn checking(cx: &App) -> bool {
    cx.try_global::<Updates>().is_some_and(|u| u.checking)
}

pub fn release(cx: &App) -> Option<Release> {
    cx.try_global::<Updates>().and_then(|u| u.release.clone())
}

/// The Updates page's line under the version.
pub fn status_line(cx: &App) -> String {
    let Some(u) = cx.try_global::<Updates>().filter(|u| u.updater.is_some()) else {
        return "Updates are off in development builds.".into();
    };
    if u.checking {
        "Checking…".into()
    } else if let Some(r) = &u.release {
        format!("GitViber {} is available.", r.version)
    } else if let Some(at) = u.checked_at {
        format!("Up to date, checked {}.", relative_time(at, now()))
    } else {
        "Not checked yet.".into()
    }
}

/// The Updates page opened: a check right away, unless one started in the last minute or
/// Settings turned automatic checks off (Check Now still asks).
pub fn page_opened(cx: &mut App) {
    let u = Updates::get(cx);
    let due = u.updater.is_some() && page_due(u.asked_at, now());
    if due && Settings::get(cx).auto_update {
        run_check(false, None, cx);
    }
}

/// Asks GitHub Releases; `told`: the user asked, so how it went shows either way.
pub fn check(told: bool, cx: &mut App) {
    run_check(told, None, cx);
}

/// `check`, telling `answered` whether GitHub did; dropped unsent when no check started (no
/// updater, or one already running).
fn run_check(told: bool, answered: Option<oneshot::Sender<bool>>, cx: &mut App) {
    let Some(updater) = Updates::get(cx).updater.clone() else {
        return;
    };
    if Updates::get(cx).checking {
        return;
    }
    let u = Updates::get(cx);
    u.checking = true;
    u.asked_at = Some(now());
    // The Updates page says "Checking…" meanwhile.
    cx.refresh_windows();
    let asked = updater.check(cx);
    let task = cx.spawn(async move |cx| {
        let answer = asked.await;
        if let Some(tx) = answered {
            let _ = tx.send(answer.is_ok());
        }
        cx.update(|cx| {
            let u = Updates::get(cx);
            u.checking = false;
            match answer {
                Ok(release) => {
                    u.checked(release, now());
                    if told && u.release.is_some() {
                        show(cx);
                    } else if told {
                        toast(
                            Toast::success("You have the newest version.")
                                .title("GitViber is up to date"),
                            cx,
                        );
                    }
                }
                Err(err) if told => {
                    toast(Toast::error(err).title("Couldn't check for updates"), cx)
                }
                Err(_) => {}
            }
            cx.refresh_windows();
        });
    });
    Updates::get(cx)._check = Some(task);
}

fn download(cx: &mut App) {
    let u = Updates::get(cx);
    let (Some(updater), Some(release)) = (u.updater.clone(), u.release.clone()) else {
        return;
    };
    if u.download.is_some() {
        return;
    }
    u.download = Some(Download::Running(None));
    u.failed = None;
    let progress: Progress = Rc::new(|p, cx| {
        Updates::get(cx).download = Some(Download::Running(p));
        cx.refresh_windows();
    });
    let going = updater.download(&release, progress, cx);
    let task = cx.spawn(async move |cx| {
        let done = going.await;
        cx.update(|cx| {
            let u = Updates::get(cx);
            match done {
                Ok(()) => u.download = Some(Download::Ready),
                Err(err) => {
                    u.download = None;
                    u.failed = Some(err.clone());
                    toast(Toast::error(err).title("Couldn't download the update"), cx);
                }
            }
            cx.refresh_windows();
        });
    });
    Updates::get(cx)._download = Some(task);
}

fn restart(cx: &mut App) {
    let Some(updater) = Updates::get(cx).updater.clone() else {
        return;
    };
    Updates::get(cx).restarting = true;
    updater.restart(cx);
}

/// The update's dialog, in the window in front (Settings' What's New… asks from its own).
pub fn show(cx: &mut App) {
    if !available(cx) {
        return;
    }
    cx.defer(|cx| {
        let Some(handle) = cx
            .active_window()
            .or_else(|| cx.windows().into_iter().next())
        else {
            return;
        };
        let _ = handle.update(cx, |_, window, cx| open_dialog(window, cx));
    });
}

pub struct UpdateBody {
    _live: Live,
}

impl Render for UpdateBody {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let release = release(cx);
        let notes = release
            .as_ref()
            .map(|r| r.notes.trim().to_string())
            .unwrap_or_default();
        let download = cx.try_global::<Updates>().and_then(|u| u.download);
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .id("release-notes")
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.code(t.background))
                    .px_3()
                    .py_2()
                    .text_size(rems(0.78125))
                    .child(if notes.is_empty() {
                        div()
                            .italic()
                            .text_color(t.subtle)
                            .child("No release notes.")
                            .into_any_element()
                    } else {
                        TextView::markdown("release-notes-text", notes).into_any_element()
                    }),
            )
            .when_some(
                match download {
                    Some(Download::Running(p)) => Some(p),
                    _ => None,
                },
                |d, p| {
                    d.child(
                        div().h(px(4.)).w_full().rounded_full().bg(t.border).child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(t.primary)
                                .w(relative(p.map_or(1., |p| p as f32 / 100.))),
                        ),
                    )
                },
            )
    }
}

fn open_dialog(window: &mut Window, cx: &mut App) {
    let Some(release) = release(cx) else { return };
    let body = cx.new(|_| UpdateBody {
        _live: Live::new("UpdateDialog"),
    });
    let mode = cx
        .try_global::<Updates>()
        .and_then(|u| u.updater.as_ref().map(|up| up.mode()))
        .unwrap_or(Mode::Download);
    let date = release.date.as_deref().and_then(|d| {
        chrono::DateTime::parse_from_rfc3339(d)
            .ok()
            .map(|d| d.format("%b %-d, %Y").to_string())
    });
    let description: SharedString = format!(
        "{}{}",
        date.map(|d| format!("Released {d}. ")).unwrap_or_default(),
        match mode {
            Mode::Download =>
                "This copy of GitViber can't replace itself: get the new version from the Releases page.",
            Mode::Install => "Nothing changes until you restart.",
        }
    )
    .into();
    let title: SharedString = format!("GitViber {} is available", release.version).into();
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, _, cx| {
        first.set(false);
        let u = cx.try_global::<Updates>();
        let state = u.and_then(|u| u.download);
        let restarting = u.is_some_and(|u| u.restarting);
        let t = cx.app_theme();
        let main = match (mode, state) {
            (Mode::Download, _) => Button::new("update-releases")
                .primary()
                .label("Open Releases Page")
                .on_click(|_, _, cx| open_url(format!("{REPO}/releases"), cx)),
            (Mode::Install, Some(Download::Ready)) => Button::new("update-restart")
                .primary()
                .label(if restarting {
                    "Restarting…"
                } else {
                    "Restart to Update"
                })
                .disabled(restarting)
                .on_click(|_, _, cx| restart(cx)),
            (Mode::Install, Some(Download::Running(p))) => Button::new("update-download")
                .primary()
                .label(match p {
                    Some(p) => format!("Downloading… {p}%"),
                    None => "Downloading…".into(),
                })
                .disabled(true),
            (Mode::Install, None) => Button::new("update-download")
                .primary()
                .label("Download Update")
                .on_click(|_, _, cx| download(cx)),
        };
        dialog
            .title(title.clone())
            .w(px(512.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_color(t.muted_foreground)
                            .child(description.clone()),
                    )
                    .child(body.clone()),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("update-later")
                            .label("Later")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(main),
            )
    });
}

/// Where an update stands, as the top bar's badge shows it.
#[derive(Clone, Debug, PartialEq)]
pub enum Badge {
    Available(String),
    Downloading(String, Option<u8>),
    Ready {
        version: String,
        restarting: bool,
    },
    /// The download failed: the badge, quieter, tries again.
    Failed {
        version: String,
        error: String,
    },
}

impl Badge {
    fn label(&self) -> String {
        match self {
            Badge::Available(v) => format!("Update Available: {v}"),
            Badge::Downloading(v, Some(p)) => format!("Downloading {v}… {p}%"),
            Badge::Downloading(v, None) => format!("Downloading {v}…"),
            Badge::Ready {
                restarting: true, ..
            } => "Restarting…".into(),
            Badge::Ready { .. } => "Restart to Update".into(),
            Badge::Failed { .. } => "Retry Update".into(),
        }
    }

    /// The tooltip, and what a screen reader hears: the version even where the label hasn't it.
    fn tip(&self) -> String {
        match self {
            Badge::Available(v) => format!("GitViber {v} is available: see what's new"),
            Badge::Downloading(v, _) => format!("Downloading GitViber {v}"),
            Badge::Ready { version, .. } => {
                format!("GitViber {version} is downloaded: restart to install it")
            }
            Badge::Failed { version, error } => {
                format!("Couldn't download GitViber {version}: {error}. Click to try again.")
            }
        }
    }
}

/// None while no newer release is out.
pub fn badge(cx: &App) -> Option<Badge> {
    let u = cx.try_global::<Updates>()?;
    let version = u.release.as_ref()?.version.clone();
    Some(match (u.download, &u.failed) {
        (Some(Download::Ready), _) => Badge::Ready {
            version,
            restarting: u.restarting,
        },
        (Some(Download::Running(p)), _) => Badge::Downloading(version, p),
        (None, Some(error)) => Badge::Failed {
            version,
            error: error.clone(),
        },
        (None, None) => Badge::Available(version),
    })
}

/// The top bar's badge once a newer release is out: filled with the accent until something
/// went wrong, then an outlined retry.
pub fn badge_button(cx: &App) -> Option<Button> {
    let badge = badge(cx)?;
    let t = cx.app_theme();
    let button = Button::new("update-badge")
        .small()
        .rounded(ButtonRounded::Size(px(999.)))
        .label(badge.label())
        .tooltip(badge.tip())
        .accessibility_label(format!("{}. {}", badge.label(), badge.tip()));
    Some(match badge {
        Badge::Available(_) => button
            .primary()
            .icon(IconName::Download)
            .on_click(|_, _, cx| show(cx)),
        Badge::Downloading(_, p) => button
            .primary()
            .icon(
                ProgressCircle::new("update-progress")
                    .color(t.primary_foreground)
                    .loading(p.is_none())
                    .value(p.unwrap_or_default().into()),
            )
            .on_click(|_, _, cx| show(cx)),
        Badge::Ready { restarting, .. } => button
            .primary()
            .icon(IconName::RotateCw)
            .disabled(restarting)
            .on_click(|_, _, cx| restart(cx)),
        Badge::Failed { .. } => button
            .outline()
            .icon(IconName::RotateCw)
            .on_click(|_, _, cx| download(cx)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use gpui_kit::TestAppContext;

    thread_local! {
        /// The wall clock as a test sets it; the real one while None.
        pub(super) static NOW: Cell<Option<i64>> = const { Cell::new(None) };
    }

    /// 2026-10-01, a wall clock to move by hand.
    const T0: i64 = 1_790_000_000;

    fn named(version: &str) -> Release {
        Release {
            version: version.into(),
            date: Some("2026-10-01T00:00:00Z".into()),
            notes: "- **New** things".into(),
        }
    }

    /// Answers its checks in turn (None: up to date; past the last, up to date), and counts them.
    #[derive(Default)]
    struct Answers {
        answers: RefCell<VecDeque<Result<Option<&'static str>, &'static str>>>,
        asked: Cell<u32>,
        /// The next download fails with this.
        fail_download: RefCell<Option<&'static str>>,
    }

    impl Answers {
        fn new(
            answers: impl IntoIterator<Item = Result<Option<&'static str>, &'static str>>,
        ) -> Rc<Self> {
            Rc::new(Answers {
                answers: RefCell::new(answers.into_iter().collect()),
                asked: Cell::new(0),
                fail_download: RefCell::default(),
            })
        }
    }

    impl Updater for Answers {
        fn mode(&self) -> Mode {
            Mode::Install
        }
        fn check(&self, _: &mut App) -> Task<Result<Option<Release>, String>> {
            self.asked.set(self.asked.get() + 1);
            let answer = self.answers.borrow_mut().pop_front().unwrap_or(Ok(None));
            Task::ready(answer.map(|v| v.map(named)).map_err(String::from))
        }
        fn download(
            &self,
            _: &Release,
            progress: Progress,
            cx: &mut App,
        ) -> Task<Result<(), String>> {
            if let Some(err) = self.fail_download.borrow_mut().take() {
                return Task::ready(Err(err.into()));
            }
            progress(Some(50), cx);
            Task::ready(Ok(()))
        }
        fn restart(&self, _: &mut App) {}
    }

    /// The updater plugged in as packaging does, its timer running, the wall clock at T0.
    fn plugged(updater: Rc<Answers>, cx: &mut TestAppContext) {
        NOW.set(Some(T0));
        cx.update(|cx| {
            cx.set_global(gitviber_ui::Settings::default());
            set_updater(updater, cx);
        });
        cx.run_until_parked();
    }

    /// Only the given updater, no timer.
    fn with(updater: Rc<Answers>, cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(gitviber_ui::Settings::default());
            Updates::get(cx).updater = Some(updater);
        });
    }

    fn version(cx: &mut TestAppContext) -> Option<String> {
        cx.update(|cx| release(cx).map(|r| r.version))
    }

    fn check_now(cx: &mut TestAppContext) {
        cx.update(|cx| check(false, cx));
        cx.run_until_parked();
    }

    #[test]
    fn failed_checks_retry_soon_then_less_often() {
        assert_eq!(next_wait(false, 0), RETRY);
        assert_eq!(next_wait(false, 1), RETRY * 2);
        assert_eq!(next_wait(false, 40), TICK);
        assert_eq!(next_wait(true, 3), TICK);
    }

    #[test]
    fn a_check_is_due_when_none_answered_or_the_last_is_old_enough() {
        assert!(due(None, T0));
        assert!(!due(Some(T0), T0 + EVERY - 1));
        assert!(due(Some(T0), T0 + EVERY));
        assert!(due(Some(T0), T0 - 60), "the clock set back");
    }

    #[gpui_kit::test]
    fn a_launch_check_that_failed_is_tried_again_in_seconds(cx: &mut TestAppContext) {
        let updater = Answers::new([Err("offline")]);
        plugged(updater.clone(), cx);
        assert_eq!(updater.asked.get(), 1);
        cx.executor().advance_clock(RETRY);
        cx.run_until_parked();
        assert_eq!(updater.asked.get(), 2, "not hours later");
        // Answered: the ticks find nothing due until the usual few hours have passed.
        cx.executor().advance_clock(TICK * 3);
        cx.run_until_parked();
        assert_eq!(updater.asked.get(), 2);
        NOW.set(Some(T0 + EVERY));
        cx.executor().advance_clock(TICK);
        cx.run_until_parked();
        assert_eq!(updater.asked.get(), 3);
    }

    /// Timers stop while the Mac sleeps, the wall clock doesn't: a night asleep, and the first
    /// tick after the wake checks.
    #[gpui_kit::test]
    fn a_wake_after_hours_asleep_checks_within_a_tick(cx: &mut TestAppContext) {
        let updater = Answers::new([]);
        plugged(updater.clone(), cx);
        assert_eq!(updater.asked.get(), 1);
        NOW.set(Some(T0 + 8 * 60 * 60));
        cx.executor().advance_clock(TICK);
        cx.run_until_parked();
        assert_eq!(updater.asked.get(), 2, "awake for only a tick");
        // A check by hand counts too: the timer waits from it.
        NOW.set(Some(T0 + 10 * 60 * 60));
        check_now(cx);
        assert_eq!(updater.asked.get(), 3);
        cx.executor().advance_clock(TICK);
        cx.run_until_parked();
        assert_eq!(updater.asked.get(), 3);
    }

    #[gpui_kit::test]
    fn timed_checks_stay_off_while_settings_say_so(cx: &mut TestAppContext) {
        let updater = Answers::new([]);
        NOW.set(Some(T0));
        cx.update(|cx| {
            cx.set_global(gitviber_ui::Settings {
                auto_update: false,
                ..Default::default()
            });
            set_updater(updater.clone(), cx);
        });
        cx.run_until_parked();
        NOW.set(Some(T0 + 2 * EVERY));
        cx.executor().advance_clock(TICK);
        cx.run_until_parked();
        assert_eq!(updater.asked.get(), 0);
    }

    #[gpui_kit::test]
    fn without_an_updater_updates_are_off(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert!(!enabled(cx));
            assert_eq!(status_line(cx), "Updates are off in development builds.");
            check(true, cx);
            assert!(!available(cx));
        });
    }

    #[gpui_kit::test]
    fn a_check_shows_what_it_found_and_an_empty_answer_clears_it(cx: &mut TestAppContext) {
        with(
            Answers::new([Ok(Some("0.2.0")), Ok(Some("0.3.0")), Ok(None)]),
            cx,
        );
        check_now(cx);
        assert_eq!(version(cx).as_deref(), Some("0.2.0"));
        cx.update(|cx| assert_eq!(status_line(cx), "GitViber 0.2.0 is available."));
        check_now(cx);
        assert_eq!(
            version(cx).as_deref(),
            Some("0.3.0"),
            "a newer one replaces it"
        );
        check_now(cx);
        assert_eq!(version(cx), None);
        cx.update(|cx| {
            assert!(badge(cx).is_none());
            assert!(status_line(cx).starts_with("Up to date, checked"));
        });
    }

    /// A release downloading or waiting for its restart outlives newer checks: the UI shows what
    /// a restart installs, and an empty answer doesn't take the Restart button away.
    #[gpui_kit::test]
    fn a_newer_check_never_replaces_a_downloaded_release(cx: &mut TestAppContext) {
        let answers = [
            Ok(Some("0.2.0")),
            Ok(Some("0.3.0")),
            Ok(None),
            Ok(Some("0.4.0")),
        ];
        with(Answers::new(answers), cx);
        check_now(cx);
        cx.update(download);
        cx.run_until_parked();
        cx.update(|cx| assert_eq!(Updates::get(cx).download, Some(Download::Ready)));
        check_now(cx);
        assert_eq!(version(cx).as_deref(), Some("0.2.0"));
        check_now(cx);
        assert_eq!(version(cx).as_deref(), Some("0.2.0"));
        cx.update(|cx| {
            assert!(
                matches!(badge(cx), Some(Badge::Ready { .. })),
                "Restart to Update stays"
            );
            assert_eq!(status_line(cx), "GitViber 0.2.0 is available.");
            // Still downloading counts the same.
            Updates::get(cx).download = Some(Download::Running(Some(10)));
        });
        check_now(cx);
        assert_eq!(version(cx).as_deref(), Some("0.2.0"));
    }

    #[gpui_kit::test]
    fn a_found_release_downloads_then_waits_for_the_restart(cx: &mut TestAppContext) {
        with(Answers::new([Ok(Some("9.9.9"))]), cx);
        check_now(cx);
        cx.update(|cx| {
            assert_eq!(status_line(cx), "GitViber 9.9.9 is available.");
            download(cx);
        });
        cx.run_until_parked();
        cx.update(|cx| assert_eq!(Updates::get(cx).download, Some(Download::Ready)));
    }

    #[test]
    fn the_page_checks_again_only_after_a_minute() {
        assert!(page_due(None, T0));
        assert!(!page_due(Some(T0), T0 + PAGE_AGAIN - 1));
        assert!(page_due(Some(T0), T0 + PAGE_AGAIN));
        assert!(page_due(Some(T0), T0 - 1), "the clock set back");
    }

    fn open_page(cx: &mut TestAppContext) {
        cx.update(page_opened);
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn opening_the_updates_page_checks_at_most_once_a_minute(cx: &mut TestAppContext) {
        let updater = Answers::new([Ok(None), Ok(Some("0.2.0"))]);
        NOW.set(Some(T0));
        with(updater.clone(), cx);
        cx.update(|cx| {
            page_opened(cx);
            assert_eq!(status_line(cx), "Checking…");
            assert!(checking(cx));
        });
        cx.run_until_parked();
        assert_eq!(updater.asked.get(), 1);
        NOW.set(Some(T0 + 30));
        open_page(cx);
        assert_eq!(updater.asked.get(), 1, "reopened within the minute");
        NOW.set(Some(T0 + PAGE_AGAIN));
        open_page(cx);
        assert_eq!(updater.asked.get(), 2);
        assert_eq!(version(cx).as_deref(), Some("0.2.0"));
        // Check Now isn't held back by the minute.
        check_now(cx);
        assert_eq!(updater.asked.get(), 3);
    }

    /// A failed check counts too: reopening the page doesn't hammer a server that's down.
    #[gpui_kit::test]
    fn a_failed_check_holds_the_page_back_as_well(cx: &mut TestAppContext) {
        let updater = Answers::new([Err("offline")]);
        NOW.set(Some(T0));
        with(updater.clone(), cx);
        open_page(cx);
        NOW.set(Some(T0 + 10));
        open_page(cx);
        assert_eq!(updater.asked.get(), 1);
    }

    #[gpui_kit::test]
    fn the_page_doesnt_check_while_settings_say_not_to(cx: &mut TestAppContext) {
        let updater = Answers::new([]);
        NOW.set(Some(T0));
        with(updater.clone(), cx);
        cx.update(|cx| {
            cx.set_global(gitviber_ui::Settings {
                auto_update: false,
                ..Default::default()
            })
        });
        open_page(cx);
        assert_eq!(updater.asked.get(), 0);
    }

    #[gpui_kit::test]
    fn without_an_updater_the_page_asks_nothing(cx: &mut TestAppContext) {
        NOW.set(Some(T0));
        cx.update(|cx| {
            cx.set_global(gitviber_ui::Settings::default());
            page_opened(cx);
            assert!(!checking(cx));
        });
    }

    #[gpui_kit::test]
    fn the_badge_follows_the_update_from_found_to_restart(cx: &mut TestAppContext) {
        gitviber_ui::test_support::config_dir("update-badge");
        cx.update(gitviber_ui::init);
        with(Answers::new([Ok(Some("0.1.21"))]), cx);
        cx.update(|cx| {
            assert_eq!(badge(cx), None, "hidden while there's nothing new");
            assert!(badge_button(cx).is_none());
        });
        check_now(cx);
        cx.update(|cx| {
            let b = badge(cx).unwrap();
            assert_eq!(b, Badge::Available("0.1.21".into()));
            assert_eq!(b.label(), "Update Available: 0.1.21");
            assert!(b.tip().contains("0.1.21"));
            let name = badge_button(cx).unwrap().accessibility_name().unwrap();
            assert!(
                name.starts_with("Update Available: 0.1.21. GitViber 0.1.21"),
                "{name}"
            );
            Updates::get(cx).download = Some(Download::Running(Some(42)));
            let b = badge(cx).unwrap();
            assert_eq!(b.label(), "Downloading 0.1.21… 42%");
            Updates::get(cx).download = Some(Download::Running(None));
            assert_eq!(badge(cx).unwrap().label(), "Downloading 0.1.21…");
            Updates::get(cx).download = None;
            download(cx);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            let b = badge(cx).unwrap();
            assert_eq!(b.label(), "Restart to Update");
            assert!(
                b.tip().contains("0.1.21"),
                "the version shows in the tooltip"
            );
            Updates::get(cx).restarting = true;
            assert_eq!(badge(cx).unwrap().label(), "Restarting…");
        });
    }

    #[gpui_kit::test]
    fn a_failed_download_turns_the_badge_into_a_retry(cx: &mut TestAppContext) {
        let updater = Answers::new([Ok(Some("0.1.21")), Ok(Some("0.1.21")), Ok(Some("0.1.22"))]);
        *updater.fail_download.borrow_mut() = Some("disk full");
        with(updater.clone(), cx);
        check_now(cx);
        cx.update(download);
        cx.run_until_parked();
        cx.update(|cx| {
            let b = badge(cx).unwrap();
            assert_eq!(
                b,
                Badge::Failed {
                    version: "0.1.21".into(),
                    error: "disk full".into()
                }
            );
            assert_eq!(b.label(), "Retry Update");
            assert!(b.tip().contains("disk full"));
        });
        // The same release found again keeps the retry; a newer one starts over.
        check_now(cx);
        cx.update(|cx| assert!(matches!(badge(cx), Some(Badge::Failed { .. }))));
        check_now(cx);
        cx.update(|cx| assert_eq!(badge(cx), Some(Badge::Available("0.1.22".into()))));
        // A retry that works ends ready.
        *updater.fail_download.borrow_mut() = Some("disk full");
        cx.update(download);
        cx.run_until_parked();
        cx.update(download);
        cx.run_until_parked();
        cx.update(|cx| assert!(matches!(badge(cx), Some(Badge::Ready { .. }))));
    }
}
