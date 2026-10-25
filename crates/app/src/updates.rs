//! Updates as the app shows them: a newer release's notes, its download and the restart, the
//! Settings page's status and the status bar's button. The updater itself comes with packaging
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
use gpui_kit::base::text::TextView;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::{
    App, AppContext as _, Context, Global, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Task,
    Window, div, prelude::FluentBuilder as _, px, relative, rems,
};

use crate::help::{REPO, open_url, toast};

/// How often a running app looks again, besides at launch and when the Updates page opens.
const EVERY: Duration = Duration::from_secs(4 * 60 * 60);
/// After a check that got no answer (the network not up yet at launch, a wake from sleep): again
/// this soon, twice as long after each one that fails, up to EVERY.
const RETRY: Duration = Duration::from_secs(10);

/// The wait before the next timed check: EVERY once one answered, else the retry's.
fn next_wait(answered: bool, failed_before: u32) -> Duration {
    if answered {
        return EVERY;
    }
    RETRY.saturating_mul(1 << failed_before.min(16)).min(EVERY)
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
    download: Option<Download>,
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
}

/// Plugs the updater in: Check for Updates applies from then on, the app checks at launch and
/// every few hours while Settings allows.
pub fn set_updater(updater: Rc<dyn Updater>, cx: &mut App) {
    let first = cx.default_global::<Updates>().updater.is_none();
    cx.global_mut::<Updates>().updater = Some(updater);
    if first {
        cx.on_action(|_: &app::CheckForUpdates, cx| check(true, cx));
        let timer = cx.spawn(async move |cx| {
            let mut failed = 0;
            loop {
                let answer = cx.update(|cx| {
                    Settings::get(cx).auto_update.then(|| {
                        let (tx, rx) = oneshot::channel();
                        run_check(false, Some(tx), cx);
                        rx
                    })
                });
                // Not asked (off, or one already running): taken as answered.
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
        format!(
            "Up to date, checked {}.",
            relative_time(at, gitviber_ui::widgets::now_unix())
        )
    } else {
        "Not checked yet.".into()
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
    Updates::get(cx).checking = true;
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
                    u.checked_at = Some(gitviber_ui::widgets::now_unix());
                    let found = release.is_some();
                    u.release = release;
                    if found && told {
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

/// The status bar's version turns into this once a newer release is out.
pub fn status_button(cx: &App) -> Option<Button> {
    let u = cx.try_global::<Updates>()?;
    let release = u.release.as_ref()?;
    let label = match u.download {
        Some(Download::Ready) if u.restarting => "Restarting…".to_string(),
        Some(Download::Ready) => "Restart to Update".to_string(),
        Some(Download::Running(Some(p))) => format!("Downloading update… {p}%"),
        Some(Download::Running(None)) => "Downloading update…".to_string(),
        None => format!("Update to v{}", release.version),
    };
    let ready = u.download == Some(Download::Ready);
    Some(
        Button::new("status-update")
            .ghost()
            .xsmall()
            .label(label)
            .disabled(u.restarting)
            .on_click(move |_, _, cx| if ready { restart(cx) } else { show(cx) }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;

    struct Fake;

    impl Updater for Fake {
        fn mode(&self) -> Mode {
            Mode::Install
        }
        fn check(&self, _: &mut App) -> Task<Result<Option<Release>, String>> {
            Task::ready(Ok(Some(Release {
                version: "9.9.9".into(),
                date: Some("2026-10-01T00:00:00Z".into()),
                notes: "- **New** things".into(),
            })))
        }
        fn download(
            &self,
            _: &Release,
            progress: Progress,
            cx: &mut App,
        ) -> Task<Result<(), String>> {
            progress(Some(50), cx);
            Task::ready(Ok(()))
        }
        fn restart(&self, _: &mut App) {}
    }

    /// Fails its first check (offline at launch), answers after.
    struct Flaky(Rc<Cell<u32>>);

    impl Updater for Flaky {
        fn mode(&self) -> Mode {
            Mode::Install
        }
        fn check(&self, _: &mut App) -> Task<Result<Option<Release>, String>> {
            self.0.set(self.0.get() + 1);
            Task::ready(if self.0.get() == 1 {
                Err("offline".into())
            } else {
                Ok(None)
            })
        }
        fn download(&self, _: &Release, _: Progress, _: &mut App) -> Task<Result<(), String>> {
            Task::ready(Ok(()))
        }
        fn restart(&self, _: &mut App) {}
    }

    #[test]
    fn failed_checks_retry_soon_then_less_often() {
        assert_eq!(next_wait(false, 0), RETRY);
        assert_eq!(next_wait(false, 1), RETRY * 2);
        assert_eq!(next_wait(false, 40), EVERY);
        assert_eq!(next_wait(true, 3), EVERY);
    }

    #[gpui_kit::test]
    fn a_launch_check_that_failed_is_tried_again_in_seconds(cx: &mut TestAppContext) {
        let checks = Rc::new(Cell::new(0));
        cx.update(|cx| {
            cx.set_global(gitviber_ui::Settings::default());
            set_updater(Rc::new(Flaky(checks.clone())), cx);
        });
        cx.run_until_parked();
        assert_eq!(checks.get(), 1);
        cx.executor().advance_clock(RETRY);
        cx.run_until_parked();
        assert_eq!(checks.get(), 2, "not four hours later");
        // Answered: the next one is the usual few hours away.
        cx.executor().advance_clock(RETRY * 8);
        cx.run_until_parked();
        assert_eq!(checks.get(), 2);
        cx.executor().advance_clock(EVERY);
        cx.run_until_parked();
        assert_eq!(checks.get(), 3);
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
    fn a_found_release_downloads_then_waits_for_the_restart(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(gitviber_ui::Settings::default());
            Updates::get(cx).updater = Some(Rc::new(Fake));
            check(false, cx);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(status_line(cx), "GitViber 9.9.9 is available.");
            download(cx);
        });
        cx.run_until_parked();
        cx.update(|cx| assert_eq!(Updates::get(cx).download, Some(Download::Ready)));
    }
}
