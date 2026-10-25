//! File → Clone Repository…, the welcome screen and the project switcher: clones a URL (or a
//! GitHub owner/name, or one of the signed-in account's repositories picked from a list) into a
//! folder, with git's progress and Cancel, then opens it. git's questions (a login, a key's
//! passphrase, a new host) are asked over it by gitviber-refs' askpass dialogs.

use std::sync::Arc;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gitviber_core::api;
use gitviber_core::network::{CANCELLED, Progress};
use gitviber_ui::actions::file;
use gitviber_ui::memory::Live;
use gitviber_ui::workspace::Toast;
use gitviber_ui::{ActiveAppTheme, Settings, Workspace};
use gpui_kit::assets::IconName;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, PathPromptOptions, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, prelude::FluentBuilder as _, px, relative, rems,
};

use crate::help::toast;

/// The URL field as git gets it: `owner/name` is GitHub shorthand.
pub fn clone_url(input: &str) -> String {
    let s = input.trim();
    let shorthand = s.split_once('/').is_some_and(|(owner, name)| {
        let word = |t: &str, first_dot: bool| {
            !t.is_empty()
                && t.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
                && (first_dot || !t.starts_with('.'))
        };
        word(owner, false) && word(name, true) && !name.contains('/')
    });
    if shorthand {
        format!(
            "https://github.com/{}.git",
            s.strip_suffix(".git").unwrap_or(s)
        )
    } else {
        s.to_string()
    }
}

/// The folder a clone of `url` goes in, as `git clone` names it: …/owner/name.git → name.
pub fn clone_folder_name(url: &str) -> String {
    let path = url.trim();
    let path = path.split(['?', '#']).next().unwrap_or("");
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix("/.git").unwrap_or(path);
    let path = path.strip_suffix(".git").unwrap_or(path);
    let name = path.rsplit(['/', ':']).next().unwrap_or("");
    if name == "." || name == ".." {
        String::new()
    } else {
        name.to_string()
    }
}

struct OwnRepo {
    full_name: String,
    description: String,
    private: bool,
}

pub struct CloneDialog {
    url: Entity<InputState>,
    name: Entity<InputState>,
    /// The folder name typed by the user; until then it follows the URL.
    named: bool,
    parent: Option<String>,
    repos: Option<Vec<OwnRepo>>,
    /// The clone running: core's id for Cancel.
    op: Option<String>,
    progress: Option<Progress>,
    error: Option<SharedString>,
    _clone: Option<Task<()>>,
    _repos: Option<Task<()>>,
    _pick: Option<Task<()>>,
    _live: Live,
    _subs: Vec<Subscription>,
}

pub fn install(cx: &mut App) {
    cx.on_action(|_: &file::CloneRepo, cx| {
        cx.defer(|cx| {
            let Some(handle) =
                crate::workspace_view::main_window(cx).or_else(|| cx.active_window())
            else {
                return;
            };
            let _ = handle.update(cx, |_, window, cx| {
                window.activate_window();
                open(window, cx)
            });
        })
    });
}

pub fn open(window: &mut Window, cx: &mut App) {
    let dialog = cx.new(|cx| CloneDialog::new(window, cx));
    let url = dialog.read(cx).url.clone();
    let first = std::cell::Cell::new(true);
    window.open_dialog(cx, move |d, window, cx| {
        if first.replace(false) {
            url.update(cx, |i, cx| i.focus(window, cx));
        }
        let busy = dialog.read(cx).op.is_some();
        let ready = dialog.read(cx).ready(cx);
        let cancellable = dialog
            .read(cx)
            .progress
            .as_ref()
            .is_none_or(|p| p.cancellable);
        let (cancel, go) = (dialog.downgrade(), dialog.downgrade());
        d.title("Clone Repository")
            .w(px(512.))
            // While cloning, only Cancel closes it: an Escape shouldn't silently stop a long clone.
            .keyboard(!busy)
            .overlay_closable(!busy)
            .close_button(!busy)
            .child(dialog.clone())
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("clone-cancel")
                            .label("Cancel")
                            // Past the transfer (checking out the files) it runs to the end.
                            .disabled(!cancellable)
                            .on_click(move |_, window, cx| {
                                let _ = cancel.update(cx, |d, cx| d.cancel(window, cx));
                            }),
                    )
                    .child(
                        Button::new("clone-go")
                            .primary()
                            .label(if busy { "Cloning…" } else { "Clone" })
                            .disabled(!ready)
                            .on_click(move |_, window, cx| {
                                let _ = go.update(cx, |d, cx| d.clone_repo(window, cx));
                            }),
                    ),
            )
    });
}

impl CloneDialog {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://github.com/owner/name.git, or search your repositories")
        });
        let name = cx.new(|cx| InputState::new(window, cx));
        let subs = vec![
            cx.subscribe_in(
                &url,
                window,
                |this, _, e: &InputEvent, window, cx| match e {
                    InputEvent::Change => {
                        if !this.named {
                            let folder = clone_folder_name(&clone_url(&this.url.read(cx).value()));
                            this.name
                                .update(cx, |n, cx| n.set_value(folder, window, cx));
                        }
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.clone_repo(window, cx),
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &name,
                window,
                |this, input, e: &InputEvent, window, cx| match e {
                    InputEvent::Change => {
                        let typed = input.read(cx).value();
                        let follows = clone_folder_name(&clone_url(&this.url.read(cx).value()));
                        if typed.as_ref() != follows {
                            this.named = true;
                        }
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.clone_repo(window, cx),
                    _ => {}
                },
            ),
        ];
        // The last clone's folder, else the home folder.
        let parent = Settings::get(cx)
            .clone_parent
            .clone()
            .or_else(|| std::env::var("HOME").ok());
        let mut this = Self {
            url,
            name,
            named: false,
            parent,
            repos: None,
            op: None,
            progress: None,
            error: None,
            _clone: None,
            _repos: None,
            _pick: None,
            _live: Live::new("CloneDialog"),
            _subs: subs,
        };
        this.load_repos(cx);
        this
    }

    /// The account's repositories, to pick from instead of typing; none without an account.
    fn load_repos(&mut self, cx: &mut Context<Self>) {
        if !gitviber_ui::has_core(cx) {
            return;
        }
        let core = gitviber_ui::core(cx);
        let listed = cx.background_spawn(async move {
            api::github::gh_own_repos(&core)
                .unwrap_or_default()
                .into_iter()
                .map(|r| OwnRepo {
                    full_name: r.full_name,
                    description: r.description,
                    private: r.private,
                })
                .collect::<Vec<_>>()
        });
        self._repos = Some(cx.spawn(async move |this, cx| {
            let repos = listed.await;
            let _ = this.update(cx, |this, cx| {
                this.repos = Some(repos);
                cx.notify();
            });
        }));
    }

    fn ready(&self, cx: &App) -> bool {
        self.op.is_none()
            && !self.url.read(cx).value().trim().is_empty()
            && !self.name.read(cx).value().trim().is_empty()
            && self.parent.is_some()
    }

    fn choose(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Clone Here".into()),
        });
        self._pick = Some(cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(dir) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| {
                    this.parent = Some(dir.to_string_lossy().into_owned());
                    cx.notify();
                });
            }
        }));
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.op {
            Some(op) => {
                let (core, op) = (gitviber_ui::core(cx), op.clone());
                cx.background_spawn(async move { api::sync::cancel_network(&core, op) })
                    .detach();
            }
            None => window.close_dialog(cx),
        }
    }

    fn clone_repo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready(cx) || !gitviber_ui::has_core(cx) {
            return;
        }
        let Some(parent) = self.parent.clone() else {
            return;
        };
        let url = clone_url(&self.url.read(cx).value());
        let folder = self.name.read(cx).value().trim().to_string();
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let op = format!(
            "clone-{}",
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        self.op = Some(op.clone());
        self.progress = None;
        self.error = None;
        let (tx, mut rx) = mpsc::unbounded::<Progress>();
        let sink = Arc::new(move |p: Progress| {
            let _ = tx.unbounded_send(p);
        });
        let core = gitviber_ui::core(cx);
        let (parent_c, folder_c) = (parent.clone(), folder.clone());
        let cloned = cx.background_spawn(async move {
            api::repo::clone_repo(&core, url, parent_c, folder_c, op, move |p| sink(p))
        });
        let handle = window.window_handle();
        self._clone = Some(cx.spawn(async move |this, cx| {
            // Ends as the clone does: its progress callback, the sender, goes with it.
            let weak = this.clone();
            let progress = async {
                while let Some(p) = rx.next().await {
                    let shown = weak.update(cx, |d: &mut CloneDialog, cx| {
                        d.progress = Some(p);
                        cx.notify();
                    });
                    if shown.is_err() {
                        break;
                    }
                }
            };
            let (done, ()) = futures::join!(cloned, progress);
            let _ = this.update(cx, |this, cx| {
                this.op = None;
                match &done {
                    Ok(_) => {}
                    Err(e) if e == CANCELLED => {}
                    Err(e) => this.error = Some(e.clone().into()),
                }
                cx.notify();
            });
            cx.update(|cx| match done {
                Ok(path) => {
                    notify_if_away("Clone finished".into(), folder, cx);
                    Settings::update(cx, |s| s.clone_parent = Some(parent));
                    let _ = handle.update(cx, |_, window, cx| {
                        // A prompt still over it (a late fetch's) closes first.
                        gitviber_refs::askpass::close_form(window, cx);
                        if let Some(ws) = Workspace::for_window(window, cx) {
                            ws.update(cx, |ws, cx| ws.open_project(path.into(), window, cx));
                        }
                    });
                }
                Err(e) if e == CANCELLED => toast(Toast::info("Clone cancelled"), cx),
                Err(e) => notify_if_away("Clone failed".into(), e, cx),
            });
        }));
        cx.notify();
    }
}

/// "A git command ends" in Settings → Notifications, while the user isn't looking at the app.
fn notify_if_away(title: String, body: String, cx: &mut App) {
    gitviber_ui::notify_if_away(|s| s.notify_git, title, body, None, cx);
}

impl Render for CloneDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let typed = self.url.read(cx).value().trim().to_lowercase();
        // What's typed narrows the list, until it's a URL.
        let picking = !typed.contains([':', '@']);
        let exact = self
            .repos
            .iter()
            .flatten()
            .any(|r| r.full_name.to_lowercase() == typed);
        let matches: Vec<(String, String, bool)> = if picking && !exact && self.op.is_none() {
            self.repos
                .iter()
                .flatten()
                .filter(|r| r.full_name.to_lowercase().contains(&typed))
                .take(50)
                .map(|r| (r.full_name.clone(), r.description.clone(), r.private))
                .collect()
        } else {
            Vec::new()
        };
        let busy = self.op.is_some();
        let _ = window;
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_color(t.muted_foreground)
                    .child("A URL git can clone, or owner/name for a GitHub repository."),
            )
            .child(Input::new(&self.url).disabled(busy))
            .when(!matches.is_empty(), |d| {
                d.child(
                    div()
                        .id("own-repos")
                        .max_h(px(176.))
                        .overflow_y_scroll()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(t.border)
                        .children(matches.into_iter().enumerate().map(
                            |(i, (name, about, private))| {
                                let pick = name.clone();
                                div()
                                    .id(("own-repo", i))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .h(px(28.))
                                    .px_2p5()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(t.hover))
                                    .text_size(rems(0.75))
                                    .child(
                                        div()
                                            .flex_none()
                                            .font_family(t.fonts.code.clone())
                                            .child(name),
                                    )
                                    .when(private, |d| {
                                        d.child(
                                            Icon::new(IconName::Lock).xsmall().text_color(t.subtle),
                                        )
                                    })
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(rems(0.6875))
                                            .text_color(t.subtle)
                                            .child(about),
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let v = pick.clone();
                                        this.url.update(cx, |u, cx| u.set_value(v, window, cx));
                                    }))
                            },
                        )),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .text_size(rems(0.71875))
                    .text_color(t.muted_foreground)
                    .child("Clone into")
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(t.fonts.code.clone())
                                    .text_color(t.foreground)
                                    .child(self.parent.clone().unwrap_or_else(|| "…".into())),
                            )
                            .child(
                                Button::new("clone-choose")
                                    .small()
                                    .label("Choose…")
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, _, cx| this.choose(cx))),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .text_size(rems(0.71875))
                    .text_color(t.muted_foreground)
                    .child("Folder name")
                    .child(Input::new(&self.name).disabled(busy)),
            )
            .when(busy, |d| {
                let p = self.progress.clone();
                let percent = p.as_ref().and_then(|p| p.percent).unwrap_or(0);
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div().h(px(4.)).w_full().rounded_full().bg(t.border).child(
                                div()
                                    .h_full()
                                    .rounded_full()
                                    .bg(t.primary)
                                    .w(relative(percent as f32 / 100.)),
                            ),
                        )
                        .child(
                            div()
                                .text_size(rems(0.6875))
                                .text_color(t.subtle)
                                .child(match p {
                                    Some(p) => match p.percent {
                                        Some(n) => format!("{} {n}%", p.phase),
                                        None => p.phase,
                                    },
                                    None => "Connecting…".into(),
                                }),
                        ),
                )
            })
            .when_some(self.error.clone(), |d, e| {
                d.child(
                    div()
                        .id("clone-error")
                        .max_h(px(160.))
                        .overflow_y_scroll()
                        .text_size(rems(0.71875))
                        .text_color(t.destructive)
                        .child(e),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorthand_is_github_and_folders_are_named_as_git_names_them() {
        assert_eq!(
            clone_url(" owner/name "),
            "https://github.com/owner/name.git"
        );
        assert_eq!(
            clone_url("owner/name.git"),
            "https://github.com/owner/name.git"
        );
        assert_eq!(
            clone_url("git@github.com:a/b.git"),
            "git@github.com:a/b.git"
        );
        // Only the last .git is the URL's: a repository named repo.git keeps its own.
        assert_eq!(
            clone_url("me/repo.git.git"),
            "https://github.com/me/repo.git.git"
        );
        assert_eq!(
            clone_url("me/repo.js.git"),
            "https://github.com/me/repo.js.git"
        );
        assert_eq!(clone_url("a/b/c"), "a/b/c");
        assert_eq!(clone_url("../x"), "../x");
        assert_eq!(clone_url("https://x.org/a/b"), "https://x.org/a/b");
        assert_eq!(clone_url("./a"), "./a");
        assert_eq!(
            clone_folder_name("https://github.com/owner/name.git"),
            "name"
        );
        assert_eq!(clone_folder_name("git@github.com:owner/name.git"), "name");
        assert_eq!(clone_folder_name("https://x.org/a/b/?q=1"), "b");
        assert_eq!(clone_folder_name("/srv/repo/.git"), "repo");
        assert_eq!(clone_folder_name("https://x.org/.."), "");
        assert_eq!(clone_folder_name("git@host:repo.git"), "repo");
        assert_eq!(
            clone_folder_name("ssh://git@host:2222/me/my.repo.git"),
            "my.repo"
        );
        assert_eq!(clone_folder_name("file:///tmp/work/.git"), "work");
        assert_eq!(clone_folder_name(""), "");
    }
}
