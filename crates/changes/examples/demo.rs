//! The Changes panel beside the center's tabs, on the repo given (else the current folder),
//! read-only unless you act in it. `--open` also opens the first change's diff, `--all` the
//! uncommitted list as one scroll, `--tree` / `--recent` set the list's view, `--find` opens
//! the filter row as ⌘F in the panel does, `--cycles N` opens and closes the panel N times
//! first and prints the memory before and after.
//!
//! cargo build -p gitviber-changes --example changes-demo
//! snap.sh target/debug/examples/changes-demo out.png 5 /path/to/repo --open

use gitviber_changes::{ChangesPanel, ChangesView};
use gitviber_ui::workspace::{ChangesOf, Selection, Workspace};
use gpui_kit::{
    AnyView, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _,
    Render, Styled as _, Task, Window, div, px,
};
use std::time::Duration;

struct Demo {
    workspace: Entity<Workspace>,
    panel: Option<Entity<ChangesPanel>>,
    _cycles: Task<()>,
}

impl Render for Demo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let center = self
            .workspace
            .update(cx, |ws, cx| ws.active_view(window, cx));
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .w(px(340.))
                    .h_full()
                    .flex_none()
                    .children(self.panel.clone()),
            )
            .child(div().flex_1().min_w_0().h_full().children(center))
    }
}

fn footprint() -> String {
    let pid = std::process::id().to_string();
    std::process::Command::new("footprint")
        .args(["-p", &pid])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            s.lines()
                .find(|l| l.contains("phys_footprint:"))
                .map(|l| l.trim().to_string())
        })
        .unwrap_or_default()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let (open, all, find) = (has("--open"), has("--all"), has("--find"));
    let view_of = ChangesView {
        tree: has("--tree"),
        recent: has("--recent"),
    };
    let cycles: usize = args
        .iter()
        .position(|a| a == "--cycles")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    gitviber_ui::dev::run(move |window, cx| {
        gitviber_changes::init(cx);
        let workspace = Workspace::for_window(window, cx).expect("a workspace");
        let ctx = workspace.update(cx, |ws, cx| ws.context(cx));
        let panel = cx.new(|cx| ChangesPanel::new(&ctx, window, cx));
        panel.update(cx, |p, cx| p.set_view(view_of, cx));
        let view: Entity<Demo> = cx.new(|cx| {
            let weak = workspace.downgrade();
            let cycles_task = cx.spawn_in(window, async move |this, cx| {
                // Let the status land, then open what was asked.
                cx.background_executor()
                    .timer(Duration::from_millis(1200))
                    .await;
                this.update_in(cx, |demo: &mut Demo, window, cx| {
                    let Some(ws) = weak.upgrade() else { return };
                    let first = demo.panel.as_ref().and_then(|p| {
                        p.read(cx)
                            .walk_list(&Selection::File {
                                path: String::new(),
                            })
                            .into_iter()
                            .next()
                    });
                    if all {
                        ws.update(cx, |ws, cx| {
                            ws.open(
                                Selection::Changes {
                                    of: ChangesOf::Unstaged,
                                },
                                false,
                                window,
                                cx,
                            )
                        });
                    } else if open && let Some(first) = first {
                        ws.update(cx, |ws, cx| ws.open(first, false, window, cx));
                    }
                    if find && let Some(panel) = &demo.panel {
                        panel.focus_handle(cx).focus(window, cx);
                        window.dispatch_action(Box::new(gitviber_ui::keymap::editor::Find), cx);
                    }
                    cx.notify();
                })
                .ok();
                if cycles == 0 {
                    return;
                }
                eprintln!("demo: before {}", footprint());
                for i in 1..=cycles {
                    this.update_in(cx, |demo: &mut Demo, _, cx| {
                        demo.panel = None;
                        cx.notify();
                    })
                    .ok();
                    cx.background_executor()
                        .timer(Duration::from_millis(150))
                        .await;
                    this.update_in(cx, |demo: &mut Demo, window, cx| {
                        let ctx = demo.workspace.update(cx, |ws, cx| ws.context(cx));
                        let panel = cx.new(|cx| ChangesPanel::new(&ctx, window, cx));
                        panel.update(cx, |p, cx| p.set_view(view_of, cx));
                        demo.panel = Some(panel);
                        cx.notify();
                    })
                    .ok();
                    cx.background_executor()
                        .timer(Duration::from_millis(400))
                        .await;
                    if i % 5 == 0 || i == cycles {
                        eprintln!("demo: after {i} open-close cycles {}", footprint());
                    }
                }
            });
            Demo {
                workspace: workspace.clone(),
                panel: Some(panel),
                _cycles: cycles_task,
            }
        });
        AnyView::from(view)
    });
}
