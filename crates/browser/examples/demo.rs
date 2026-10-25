//! The browser tabs in a workspace's center, with a row of its tabs above, and a terminal
//! session agents' commands can come from: `browser-demo [PROJECT]`.
//!
//! - BROWSER_DEMO_ENV=<file>: a shell session is started in the project, and the env its pane
//!   gives `gitviber browser` written there (`. <file>; browser-demo --browser tab new URL`).
//! - BROWSER_DEMO_URL=<url> opens a tab of it; with BROWSER_DEMO_CYCLE=<n>, opens and closes
//!   n tabs of it, twice, printing the footprint between, then quits.
//!
//! The pages keep nothing on disk (keep_data_in_memory): a demo never sees the user's data.
//! Run it with HOME and TMPDIR in a scratch folder, so neither the skill nor the socket is the
//! user's.

use std::time::Duration;

use gitviber_ui::dev;
use gitviber_ui::workspace::{Selection, Workspace, WorkspaceEvent};
use gitviber_ui::ActiveAppTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, v_flex, Selectable as _, Sizable as _};
use gpui_kit::{
    div, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Subscription, Task, Window,
};

struct Center {
    workspace: Entity<Workspace>,
    _changes: Subscription,
    _script: Task<()>,
}

fn footprint() -> String {
    gitviber_ui::memory::footprint()
        .map(gitviber_ui::memory::format_bytes)
        .unwrap_or_default()
}

fn page(url: &str) -> Selection {
    Selection::Browser {
        id: gitviber_browser::ui::new_id(),
        url: url.into(),
        title: None,
        device: None,
        zoom: None,
        scheme: None,
    }
}

impl Center {
    fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let changes = cx.subscribe(&workspace, |_, _, _: &WorkspaceEvent, cx| cx.notify());
        let url = std::env::var("BROWSER_DEMO_URL").ok();
        let cycles: usize = std::env::var("BROWSER_DEMO_CYCLE")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        let script = match url {
            Some(url) if cycles > 0 => Self::cycle(url, cycles, window, cx),
            Some(url) => {
                let ws = workspace.clone();
                cx.spawn_in(window, async move |_, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(500))
                        .await;
                    let _ = cx.update(|window, cx| {
                        ws.update(cx, |ws, cx| ws.open(page(&url), false, window, cx))
                    });
                })
            }
            None => Task::ready(()),
        };
        Self {
            workspace,
            _changes: changes,
            _script: script,
        }
    }

    /// Opens and closes `n` tabs of `url`, twice, the footprint printed before and after each round.
    fn cycle(url: String, n: usize, window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            let executor = cx.background_executor().clone();
            let wait = |ms| executor.timer(Duration::from_millis(ms));
            wait(2000).await;
            eprintln!("browser-demo: before {}", footprint());
            for round in 0..2 {
                for _ in 0..n {
                    let opened = this.update_in(cx, |this, window, cx| {
                        let sel = page(&url);
                        let key = gitviber_ui::workspace::selection_key(&sel);
                        this.workspace
                            .update(cx, |ws, cx| ws.open(sel, false, window, cx));
                        // A window behind others isn't drawn: the view is made here instead.
                        let tab = this.workspace.read(cx).tabs().get(&key).and_then(|t| {
                            t.item
                                .handle()?
                                .view()
                                .downcast::<gitviber_browser::ui::BrowserTab>()
                                .ok()
                        });
                        if let Some(tab) = tab {
                            tab.update(cx, |tab, cx| tab.load_now(window, cx));
                        }
                        key
                    });
                    let Ok(key) = opened else { return };
                    wait(1500).await;
                    let _ = this.update(cx, |this, cx| {
                        this.workspace.update(cx, |ws, cx| ws.close_tab(&key, cx))
                    });
                    wait(500).await;
                }
                wait(3000).await;
                eprintln!(
                    "browser-demo: after round {} of {n} tabs {}",
                    round + 1,
                    footprint()
                );
            }
            let _ = cx.update(|_, cx| cx.quit());
        })
    }
}

impl Render for Center {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let ws = self.workspace.clone();
        let (tabs, active): (Vec<(SharedString, SharedString)>, Option<SharedString>) = {
            let w = ws.read(cx);
            (
                w.tabs()
                    .tabs()
                    .iter()
                    .map(|e| (e.key.clone(), e.item.title(cx)))
                    .collect(),
                w.tabs().active_key().cloned(),
            )
        };
        let mut row = h_flex()
            .h(gpui_kit::px(30.))
            .flex_none()
            .gap(gpui_kit::px(2.))
            .px(gpui_kit::px(6.))
            .border_b_1()
            .border_color(t.border);
        for (i, (key, title)) in tabs.into_iter().enumerate() {
            let ws = ws.clone();
            let k = key.clone();
            row = row.child(
                Button::new(("tab", i))
                    .ghost()
                    .xsmall()
                    .label(title)
                    .selected(active.as_ref() == Some(&key))
                    .on_click(move |_, window, cx| {
                        ws.update(cx, |ws, cx| ws.activate_tab(&k, window, cx))
                    }),
            );
        }
        let new_ws = ws.clone();
        row =
            row.child(Button::new("new").ghost().xsmall().label("+").on_click(
                move |_, window, cx| {
                    let _ = &new_ws;
                    gitviber_browser::ui::open_new_tab(window, cx)
                },
            ))
            .child({
                let ws = ws.clone();
                Button::new("close")
                    .ghost()
                    .xsmall()
                    .label("×")
                    .on_click(move |_, _, cx| ws.update(cx, |ws, cx| ws.close_active_tab(cx)))
            });
        let view = ws.update(cx, |ws, cx| ws.active_view(window, cx));
        v_flex()
            .size_full()
            .child(row)
            .child(div().flex_1().min_h_0().children(view))
    }
}

/// A shell in `root` as a terminal pane would start, and the env it gives `gitviber browser`.
fn pane_env(root: &std::path::Path, out: &std::path::Path, cx: &mut gpui_kit::App) {
    let core = gitviber_ui::core(cx);
    let spawned = gitviber_core::api::app::pty_spawn(
        &core,
        root.to_string_lossy().into_owned(),
        80,
        24,
        false,
        None,
        |_| true,
        |_| {},
    );
    match spawned {
        Ok(s) => {
            let lines: String = gitviber_browser::server::env(s.id)
                .into_iter()
                .map(|(k, v)| format!("export {k}='{v}'\n"))
                .collect();
            if let Err(e) = std::fs::write(out, lines) {
                eprintln!("browser-demo: couldn't write {}: {e}", out.display());
            }
        }
        Err(e) => eprintln!("browser-demo: no shell: {e}"),
    }
}

fn main() {
    // `browser-demo --browser …`: the CLI, as `gitviber browser` runs it.
    if let Some(code) = gitviber_browser::client::helper() {
        std::process::exit(code);
    }
    gitviber_browser::keep_data_in_memory();
    dev::run(|window, cx| {
        gitviber_browser::init(cx);
        if let Some(out) = std::env::var_os("BROWSER_DEMO_ENV") {
            pane_env(&dev::project_arg(), std::path::Path::new(&out), cx);
        }
        let workspace = Workspace::for_window(window, cx).expect("the dev host's workspace");
        cx.new(|cx| Center::new(workspace, window, cx)).into()
    });
}
