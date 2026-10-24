//! History on a repo, beside the center tab it opens, for screenshots and memory checks.
//!
//! history-demo [REPO] [--all] [--search QUERY] [--open] [--compare] [--to-end] [--cycles N]
//!
//! `--open`: the first commit opened, as a click does (its tab shows on the right).
//! `--compare`: the Compare screen on the right. `--open-all`: the first commit's tab of all files. `--to-end`: every page of the history loaded.
//! `--cycles N`: the panel built, scrolled to the end and closed N times, with the process's
//! memory printed before, after the first and after the last.

use std::time::Duration;

use gitviber_history::HistoryPanel;
use gitviber_ui::memory::footprint;
use gitviber_ui::workspace::{PanelContext, Workspace};
use gitviber_ui::{ActiveAppTheme, dev};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Task, Window, div, px,
};

struct Args {
    all: bool,
    search: Option<String>,
    open: bool,
    compare: bool,
    open_all: bool,
    to_end: bool,
    cycles: usize,
}

fn args() -> Args {
    let mut a = Args {
        all: false,
        search: None,
        open: false,
        compare: false,
        open_all: false,
        to_end: false,
        cycles: 0,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--all" => a.all = true,
            "--search" => a.search = it.next(),
            "--open" => a.open = true,
            "--compare" => a.compare = true,
            "--open-all" => a.open_all = true,
            "--to-end" => a.to_end = true,
            "--cycles" => a.cycles = it.next().and_then(|n| n.parse().ok()).unwrap_or(0),
            _ => {}
        }
    }
    a
}

fn mb() -> String {
    footprint()
        .map(|b| format!("{}MB", b / 1_048_576))
        .unwrap_or_default()
}

struct Demo {
    workspace: Entity<Workspace>,
    panel: Option<Entity<HistoryPanel>>,
    _task: Task<()>,
    _ws: Subscription,
}

impl Render for Demo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let tab = self
            .workspace
            .read(cx)
            .tabs()
            .active()
            .and_then(|t| t.item.handle().map(|h| h.view()));
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .w(px(380.))
                    .flex_none()
                    .h_full()
                    .border_r_1()
                    .border_color(t.border)
                    .bg(t.sidebar)
                    .children(self.panel.clone()),
            )
            .child(div().flex_1().min_w_0().h_full().children(tab))
    }
}

fn panel(
    workspace: &Entity<Workspace>,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) -> Option<Entity<HistoryPanel>> {
    let project = workspace.read(cx).project().cloned()?;
    let ctx = PanelContext {
        workspace: workspace.downgrade(),
        project: Some(project.clone()),
    };
    Some(cx.new(|cx| HistoryPanel::new(&ctx, project, window, cx)))
}

fn main() {
    let a = args();
    dev::run(move |window, cx| {
        gitviber_diff::init(cx);
        gitviber_diff::set_core(gitviber_ui::core(cx), cx);
        gitviber_refs::init(cx);
        gitviber_history::init(cx);
        // Set either way, so one --all run doesn't stay on for the next.
        gitviber_history::set_all_branches(a.all, cx);
        let workspace =
            Workspace::for_window(window, cx).expect("dev::run registers the workspace");
        let first = panel(&workspace, window, cx);
        cx.new(|cx: &mut Context<Demo>| {
            let ws = cx.observe(&workspace, |_, _, cx| cx.notify());
            let (search, open, compare, to_end, cycles, open_all) = (
                a.search.clone(),
                a.open,
                a.compare,
                a.to_end,
                a.cycles,
                a.open_all,
            );
            let task = cx.spawn_in(window, async move |this, cx| {
                let exec = cx.background_executor().clone();
                let wait = |ms| exec.timer(Duration::from_millis(ms));
                wait(1500).await;
                let _ = this.update_in(cx, |this, window, cx| {
                    let Some(p) = this.panel.clone() else { return };
                    p.update(cx, |p, cx| {
                        if let Some(q) = &search {
                            p.search_for(q.clone(), window, cx);
                        }
                        if to_end {
                            p.load_to_end(cx);
                        }
                        if open {
                            p.open_first(window, cx);
                        }
                        if open_all {
                            p.open_first_all(window, cx);
                        }
                        if compare {
                            p.open_compare(window, cx);
                        }
                    });
                });
                if cycles == 0 {
                    return;
                }
                eprintln!("demo: before {}", mb());
                for i in 0..cycles {
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.panel = panel(&this.workspace, window, cx);
                        if let Some(p) = &this.panel {
                            p.update(cx, |p, cx| p.load_to_end(cx));
                        }
                        cx.notify();
                    });
                    // Every page of this repo's history, then the panel closed.
                    for _ in 0..200 {
                        wait(200).await;
                        let done = this
                            .update(cx, |this, cx| {
                                this.panel
                                    .as_ref()
                                    .is_some_and(|p| p.read(cx).loaded_to_end(cx))
                            })
                            .unwrap_or(true);
                        if done {
                            break;
                        }
                    }
                    if i == 0 {
                        let n = this
                            .update(cx, |this, cx| {
                                this.panel.as_ref().map_or(0, |p| p.read(cx).loaded(cx))
                            })
                            .unwrap_or(0);
                        eprintln!("demo: first open, {n} commits listed: {}", mb());
                    }
                    let _ = this.update(cx, |this, cx| {
                        this.panel = None;
                        cx.notify();
                    });
                    wait(300).await;
                }
                eprintln!("demo: after {cycles} open-close cycles {}", mb());
                eprintln!("demo: live {:?}", gitviber_ui::memory::live_counts());
            });
            Demo {
                workspace: workspace.clone(),
                panel: first,
                _task: task,
                _ws: ws,
            }
        })
        .into()
    });
}
