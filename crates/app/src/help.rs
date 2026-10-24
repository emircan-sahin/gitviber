//! The Help menu's commands and the app's links: the readme, a bug report with the setup filled
//! in, diagnostics on the clipboard, the logs, release notes, the license, and installing the
//! `gitviber` command. Global, so they work from any window and with no project open.

use gitviber_core::api;
use gitviber_ui::actions::{app, help};
use gitviber_ui::workspace::Toast;
use gpui_kit::{App, AppContext as _, ClipboardItem};

pub const REPO: &str = "https://github.com/emircan-sahin/gitviber";

/// Shows `toast` in the window in front, once the update that asked for it is done (a window
/// being updated can't take one).
pub fn toast(toast: Toast, cx: &mut App) {
    cx.defer(move |cx| {
        let Some(window) = cx
            .active_window()
            .or_else(|| cx.windows().into_iter().next())
        else {
            return;
        };
        let _ = window.update(cx, |_, window, cx| toast.show(window, cx));
    });
}

/// Opens `url` in the user's browser.
pub fn open_url(url: impl Into<String>, cx: &mut App) {
    let url = url.into();
    let core = gitviber_ui::core(cx);
    let opened = cx.background_spawn(async move { api::app::open_url(&core, url) });
    cx.spawn(async move |cx| {
        if let Err(err) = opened.await {
            cx.update(|cx| toast(Toast::error(err).title("Couldn't open the link"), cx));
        }
    })
    .detach();
}

/// What a bug report needs to know about the setup, in one line.
pub fn diagnostics(a: &api::app::About) -> String {
    let build = if a.commit.is_empty() {
        String::new()
    } else {
        format!(" ({})", a.commit)
    };
    [
        Some(format!("GitViber {}{build}", a.version)),
        Some(format!("{} {}", a.os, a.arch)),
        Some(format!("git {}", a.git.as_deref().unwrap_or("not found"))),
        Some(format!("gh {}", a.gh.as_deref().unwrap_or("not found"))),
        a.webview.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}

/// The new-issue page with the bug report template, its version, OS and git filled in.
pub fn bug_report_url(a: Option<&api::app::About>) -> String {
    let mut q = vec![("template", "bug_report.yml".to_string())];
    if let Some(a) = a {
        let version = if a.commit.is_empty() {
            a.version.clone()
        } else {
            format!("{} ({})", a.version, a.commit)
        };
        q.push(("version", version));
        q.push(("os", format!("{} {}", a.os, a.arch)));
        if let Some(git) = &a.git {
            q.push(("git-version", format!("git version {git}")));
        }
    }
    let query: Vec<String> = q
        .into_iter()
        .map(|(k, v)| format!("{k}={}", encode(&v)))
        .collect();
    format!("{REPO}/issues/new?{}", query.join("&"))
}

/// A URL query value, as URLSearchParams writes it.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Version, commit, OS, git and gh: asked off the UI thread, as `gh --version` can take a moment.
pub fn with_about(cx: &mut App, then: impl FnOnce(Option<api::app::About>, &mut App) + 'static) {
    let core = gitviber_ui::core(cx);
    let about = cx.background_spawn(async move { api::app::about(&core).ok() });
    cx.spawn(async move |cx| {
        let about = about.await;
        cx.update(|cx| then(about, cx));
    })
    .detach();
}

fn install_cli(cx: &mut App) {
    let core = gitviber_ui::core(cx);
    let installed = cx.background_spawn(async move { api::app::install_cli(&core) });
    cx.spawn(async move |cx| {
        let shown = match installed.await {
            Ok(path) => Toast::success(format!(
                "{path}: run gitviber in a repository to open it here."
            ))
            .title("Installed the gitviber command"),
            Err(err) => Toast::error(err).title("Couldn't install the command"),
        };
        cx.update(|cx| toast(shown, cx));
    })
    .detach();
}

pub fn install(cx: &mut App) {
    cx.on_action(|_: &help::Readme, cx| open_url(format!("{REPO}#readme"), cx));
    cx.on_action(|_: &help::ReleaseNotes, cx| open_url(format!("{REPO}/releases"), cx));
    cx.on_action(|_: &help::License, cx| open_url(format!("{REPO}/blob/main/LICENSE"), cx));
    cx.on_action(|_: &help::ReportBug, cx| {
        with_about(cx, |about, cx| open_url(bug_report_url(about.as_ref()), cx))
    });
    cx.on_action(|_: &help::CopyDiagnostics, cx| {
        with_about(cx, |about, cx| match about {
            Some(about) => {
                let line = diagnostics(&about);
                cx.write_to_clipboard(ClipboardItem::new_string(line.clone()));
                toast(Toast::success(line).title("Diagnostics copied"), cx);
            }
            None => toast(Toast::error("The version info couldn't be read."), cx),
        })
    });
    cx.on_action(|_: &help::ShowLogs, cx| {
        let core = gitviber_ui::core(cx);
        let shown = cx.background_spawn(async move { api::app::show_logs(&core) });
        cx.spawn(async move |cx| {
            if let Err(err) = shown.await {
                cx.update(|cx| toast(Toast::error(err).title("Couldn't show the logs"), cx));
            }
        })
        .detach();
    });
    cx.on_action(|_: &app::InstallCli, cx| install_cli(cx));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn about() -> api::app::About {
        api::app::About {
            version: "0.1.17".into(),
            commit: "abc1234".into(),
            os: "macOS 26.4".into(),
            arch: "aarch64".into(),
            git: Some("2.50.1 (Apple Git-155)".into()),
            gh: None,
            webview: None,
        }
    }

    #[test]
    fn diagnostics_read_as_one_line() {
        assert_eq!(
            diagnostics(&about()),
            "GitViber 0.1.17 (abc1234) · macOS 26.4 aarch64 · git 2.50.1 (Apple Git-155) · gh not found"
        );
    }

    #[test]
    fn a_bug_report_names_the_setup() {
        let url = bug_report_url(Some(&about()));
        assert!(url.starts_with(&format!("{REPO}/issues/new?template=bug_report.yml")));
        assert!(url.contains("version=0.1.17+%28abc1234%29"));
        assert!(url.contains("os=macOS+26.4+aarch64"));
        assert!(url.contains("git-version=git+version+2.50.1+%28Apple+Git-155%29"));
        assert_eq!(
            bug_report_url(None),
            format!("{REPO}/issues/new?template=bug_report.yml")
        );
    }
}
