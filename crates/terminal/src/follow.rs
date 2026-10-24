//! What a terminal link opens: a page in a browser tab (this machine's) or the default browser, a
//! file in its tab at its line, a folder in the explorer or the file manager, a commit's tab, a
//! pull request's or an issue's.

use std::sync::Arc;

use gitviber_core::{Core, api};
use gitviber_files::{OpenOptions, Reveal};
use gitviber_ui::workspace::{ChangesOf, Selection, Toast, Workspace};
use gpui::{App, AppContext as _, Task, WeakEntity, Window};

use crate::links::{Target, is_local_url};

fn at_line(line: Option<u32>, column: Option<u32>) -> OpenOptions {
    OpenOptions {
        preview: false,
        // The editor's columns count from 0, a printed one from 1.
        reveal: line.map(|line| Reveal::Line {
            line,
            column: column.unwrap_or(1).saturating_sub(1),
        }),
        focus: true,
    }
}

fn open_url(core: Arc<Core>, url: String, cx: &App) {
    cx.background_spawn(async move {
        let _ = api::app::open_url(&core, url);
    })
    .detach();
}

/// A new browser tab's id, as the browser's own.
fn page_id() -> String {
    let hex = gitviber_core::local_socket::random_hex(16).unwrap_or_else(|| {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        format!("{n:032x}")
    });
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Follows `target`; what has to ask git or GitHub first is the task, which drops with it.
pub(crate) fn follow(
    target: Target,
    ws: &WeakEntity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) -> Option<Task<()>> {
    let core = gitviber_ui::core(cx);
    match target {
        Target::Url(url) => {
            // Until the browser registers its opener, a page opens in the default browser.
            let page = Selection::Browser {
                id: page_id(),
                url: url.clone(),
                title: None,
                device: None,
                zoom: None,
                scheme: None,
            };
            let shown = is_local_url(&url)
                && ws
                    .update(cx, |ws, cx| ws.open(page, false, window, cx))
                    .unwrap_or(false);
            if !shown {
                open_url(core, url, cx);
            }
        }
        Target::Path {
            path, dir: true, ..
        } => gitviber_files::reveal_in_explorer(ws, &path, window, cx),
        Target::Path {
            path, line, column, ..
        } => gitviber_files::open_file(ws, &path, at_line(line, column), window, cx),
        Target::External {
            path, dir: true, ..
        } => {
            cx.background_spawn(async move {
                let _ = api::files::external_reveal(&core, path);
            })
            .detach();
        }
        Target::External {
            path, line, column, ..
        } => gitviber_files::open_external(ws, &path, at_line(line, column), window, cx),
        Target::Commit(sha) => {
            let ws = ws.clone();
            return Some(window.spawn(cx, async move |cx| {
                let found = cx
                    .background_spawn(async move { api::history::find_commit(&core, sha) })
                    .await;
                let _ = cx.update(|window, cx| match found {
                    Ok(Some(commit)) => {
                        let sel = Selection::Changes {
                            of: ChangesOf::Commit {
                                commit: Arc::new(commit),
                                url: None,
                            },
                        };
                        let _ = ws.update(cx, |ws, cx| {
                            ws.show_panel("history", cx);
                            ws.open(sel, false, window, cx);
                        });
                    }
                    _ => Toast::error("Could not open the commit").show(window, cx),
                });
            }));
        }
        Target::Item { number, pull, repo } => {
            let ws = ws.clone();
            return Some(window.spawn(cx, async move |cx| {
                let name = repo.trim_start_matches("https://github.com/").to_string();
                // A pull request unless GitHub says there's none of that number, then an issue
                // (whose read would take a pull request for one). Not signed in, or GitHub
                // unreachable: the page, in the browser.
                let asked = cx
                    .background_spawn({
                        let core = core.clone();
                        async move {
                            if api::github::pr_detail(&core, Some(name.clone()), number).is_ok() {
                                return Some(None);
                            }
                            if pull {
                                return None;
                            }
                            api::github::issue_detail(&core, Some(name), number)
                                .ok()
                                .map(|d| Some(d.issue))
                        }
                    })
                    .await;
                let _ = cx.update(|window, cx| match asked {
                    Some(None) => gitviber_refs::hooks::open_pull(
                        &format!("{repo}/pull/{number}"),
                        window,
                        cx,
                    ),
                    Some(Some(issue)) => {
                        let _ = ws.update(cx, |ws, cx| {
                            ws.open(
                                Selection::Issue {
                                    issue: Arc::new(issue),
                                },
                                false,
                                window,
                                cx,
                            )
                        });
                    }
                    None => open_url(
                        core,
                        format!("{repo}/{}/{number}", if pull { "pull" } else { "issues" }),
                        cx,
                    ),
                });
            }));
        }
    }
    None
}
