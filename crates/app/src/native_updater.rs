//! The installed app's updater (gitviber-updater) behind updates.rs's `Updater`: its blocking
//! check, download and install run on the background executor, their results come back here.

use std::cell::RefCell;
use std::rc::Rc;

use futures::StreamExt as _;
use gitviber_ui::workspace::Toast;
use gitviber_updater as updater;
use gpui_kit::{App, AppContext as _, Task};

use crate::help::toast;
use crate::updates::{Mode, Progress, Release, Updater, set_updater};

/// Plugs it in when this build updates itself: an installed .app or AppImage, a .deb or .rpm
/// (which only points at the Releases page). Not in a development build, nor a snapshot.
pub fn install(cx: &mut App) {
    if std::env::var_os("GITVIBER_SNAPSHOT").is_some() {
        return;
    }
    let Some(mode) = updater::mode() else {
        return;
    };
    let mode = match mode {
        updater::Mode::Install => Mode::Install,
        updater::Mode::Download => Mode::Download,
    };
    let native = Native {
        mode,
        found: Rc::default(),
        downloaded: Rc::default(),
        _restart: RefCell::default(),
    };
    set_updater(Rc::new(native), cx);
}

struct Native {
    mode: Mode,
    /// What the last check found: what a download fetches.
    found: Rc<RefCell<Option<updater::Update>>>,
    /// Verified and waiting for the restart; its file goes when it's dropped.
    downloaded: Rc<RefCell<Option<updater::Download>>>,
    _restart: RefCell<Option<Task<()>>>,
}

impl Updater for Native {
    fn mode(&self) -> Mode {
        self.mode
    }

    fn check(&self, cx: &mut App) -> Task<Result<Option<Release>, String>> {
        let (found, downloaded) = (self.found.clone(), self.downloaded.clone());
        let asked = cx.background_spawn(async { updater::check() });
        cx.spawn(async move |_| {
            let update = asked.await.map_err(|e| e.to_string())?;
            let release = update.as_ref().map(|u| Release {
                version: u.version.clone(),
                date: u.date.clone(),
                notes: u.notes.clone(),
            });
            // A downloaded update stays the one a restart installs.
            if downloaded.borrow().is_none() {
                *found.borrow_mut() = update;
            }
            Ok(release)
        })
    }

    fn download(
        &self,
        release: &Release,
        progress: Progress,
        cx: &mut App,
    ) -> Task<Result<(), String>> {
        let update = self.found.borrow().clone();
        let Some(update) = update.filter(|u| u.version == release.version) else {
            return Task::ready(Err("Check for updates again first".into()));
        };
        // Only a new percent crosses to the UI thread, not every network chunk.
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let fetching = cx.background_spawn(async move {
            let mut last = None;
            update.download(|received, total| {
                let percent = total
                    .filter(|t| *t > 0)
                    .map(|t| (received * 100 / t).min(100) as u8);
                if percent != last {
                    last = percent;
                    let _ = tx.unbounded_send(percent);
                }
            })
        });
        let downloaded = self.downloaded.clone();
        cx.spawn(async move |cx| {
            while let Some(percent) = rx.next().await {
                cx.update(|cx| progress(percent, cx));
            }
            let download = fetching.await.map_err(|e| e.to_string())?;
            *downloaded.borrow_mut() = Some(download);
            Ok(())
        })
    }

    /// Installs, has the new version start once this one is gone, then quits the usual way
    /// (which asks first while something runs).
    fn restart(&self, cx: &mut App) {
        let Some(download) = self.downloaded.borrow_mut().take() else {
            return;
        };
        let downloaded = self.downloaded.clone();
        let installing = cx.background_spawn(async move {
            match download.install() {
                Ok(()) => updater::relaunch().map_err(|e| (e.to_string(), None)),
                // The download stays, so another click tries again.
                Err(err) => Err((err.to_string(), Some(download))),
            }
        });
        let task = cx.spawn(async move |cx| {
            let result = installing.await;
            cx.update(|cx| match result {
                Ok(()) => crate::quit::request(cx),
                Err((err, Some(download))) => {
                    *downloaded.borrow_mut() = Some(download);
                    toast(Toast::error(err).title("Couldn't install the update"), cx);
                }
                Err((err, None)) => toast(
                    Toast::error(format!(
                        "The update is installed: quit and reopen GitViber to start it. {err}"
                    ))
                    .title("Couldn't restart GitViber"),
                    cx,
                ),
            });
        });
        *self._restart.borrow_mut() = Some(task);
    }
}
