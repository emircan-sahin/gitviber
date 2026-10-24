//! The explorer beside a file tab of a repo, for screenshots and memory checks.
//!
//! files-demo [REPO] [--file PATH] [--md] [--search TEXT] [--reveal LINE] [--cycles N]
//!
//! Opens PATH (crates/core/src/patch.rs) once the repo is open; `--md` opens README.md after it,
//! rendered; `--search` shows the Search panel with TEXT searched instead of the explorer.
//! `--cycles N` opens and closes PATH's tab N times and prints the process's memory before and
//! after. GITVIBER_BACKGROUND=1 opens the window behind the others without taking the keyboard.

use std::time::Duration;

use gitviber_files::{OpenOptions, Reveal, SearchPanel};
use gitviber_ui::workspace::{Project, ProjectEvent, Workspace};
use gitviber_ui::{ActiveAppTheme as _, dev, memory};
use gpui_kit::{
    AnyView, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Task, Window, div, px,
};

#[derive(Clone)]
struct Args {
    file: String,
    md: bool,
    search: Option<String>,
    reveal: Option<u32>,
    cycles: usize,
}

fn args() -> Args {
    let mut a = Args {
        file: "crates/core/src/patch.rs".into(),
        md: false,
        search: None,
        reveal: None,
        cycles: 0,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--file" => a.file = it.next().unwrap_or_default(),
            "--md" => a.md = true,
            "--search" => a.search = it.next(),
            "--reveal" => a.reveal = it.next().and_then(|n| n.parse().ok()),
            "--cycles" => a.cycles = it.next().and_then(|n| n.parse().ok()).unwrap_or(0),
            _ => {}
        }
    }
    a
}

fn footprint() -> String {
    memory::footprint()
        .map(memory::format_bytes)
        .unwrap_or_else(|| "?".into())
}

struct Host {
    workspace: Entity<Workspace>,
    side: AnyView,
    _subs: Vec<Subscription>,
    _cycle: Task<()>,
}

impl Host {
    fn opened(&mut self, a: &Args, window: &mut Window, cx: &mut Context<Self>) {
        let ws = self.workspace.downgrade();
        if a.cycles > 0 {
            let (file, cycles) = (a.file.clone(), a.cycles);
            self._cycle = cx.spawn_in(window, async move |_, cx| {
                eprintln!("demo: before {}", footprint());
                for i in 0..=cycles {
                    _ = cx.update(|window, cx| {
                        gitviber_files::open_file(&ws, &file, OpenOptions::default(), window, cx)
                    });
                    cx.background_executor()
                        .timer(Duration::from_millis(2500))
                        .await;
                    eprintln!("demo: open {i} {}", footprint());
                    if i < cycles {
                        _ = cx.update(|_, cx| ws.update(cx, |ws, cx| ws.close_active_tab(cx)));
                        cx.background_executor()
                            .timer(Duration::from_millis(200))
                            .await;
                    }
                }
                eprintln!("demo: after {cycles} open-close cycles {}", footprint());
                eprintln!("demo: live {:?}", memory::live_counts());
            });
            return;
        }
        let reveal = a.reveal.map(|line| Reveal::Line { line, column: 0 });
        gitviber_files::open_file(
            &ws,
            &a.file,
            OpenOptions {
                preview: false,
                reveal,
                focus: false,
            },
            window,
            cx,
        );
        if a.md {
            gitviber_files::open_file(
                &ws,
                "README.md",
                OpenOptions {
                    preview: false,
                    ..Default::default()
                },
                window,
                cx,
            );
        }
        if let (Some(text), Ok(search)) = (
            a.search.clone(),
            self.side.clone().downcast::<SearchPanel>(),
        ) {
            search.update(cx, |s, cx| s.ask(Some(text), window, cx));
        }
    }
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        let ws = self.workspace.read(cx);
        let active = ws
            .tabs()
            .active()
            .and_then(|tab| tab.item.handle())
            .map(|h| h.view());
        let titles: Vec<_> = ws
            .tabs()
            .tabs()
            .iter()
            .map(|tab| {
                let on = ws.tabs().active_key() == Some(&tab.key);
                let title = tab.item.handle().map(|h| h.title(cx)).unwrap_or_default();
                div()
                    .px_3()
                    .h_full()
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .border_r_1()
                    .border_color(t.border)
                    .text_color(if on { t.foreground } else { t.muted_foreground })
                    .when_bg(on, t.background)
                    .child(title)
            })
            .collect();
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex_none()
                            .h(px(34.))
                            .flex()
                            .bg(t.panel)
                            .border_b_1()
                            .border_color(t.border)
                            .children(titles),
                    )
                    .child(div().flex_1().min_h_0().children(active)),
            )
            .child(
                div()
                    .w(px(280.))
                    .flex_none()
                    .h_full()
                    .bg(t.sidebar)
                    .border_l_1()
                    .border_color(t.border)
                    .child(self.side.clone()),
            )
    }
}

trait WhenBg {
    fn when_bg(self, on: bool, color: gpui_kit::Hsla) -> Self;
}

impl WhenBg for gpui_kit::Div {
    fn when_bg(self, on: bool, color: gpui_kit::Hsla) -> Self {
        if on { self.bg(color) } else { self }
    }
}

fn main() {
    let a = args();
    dev::run(move |window, cx| {
        gitviber_files::init(cx);
        gitviber_diff::init(cx);
        let workspace = Workspace::for_window(window, cx).expect("dev::run's workspace");
        let side_id = if a.search.is_some() {
            gitviber_files::search::PANEL_ID
        } else {
            gitviber_files::explorer::PANEL_ID
        };
        let side = workspace
            .update(cx, |ws, cx| ws.panel(side_id, window, cx))
            .expect("the panel")
            .view();
        let project: Option<Entity<Project>> = workspace.read(cx).project().cloned();
        let a2 = a.clone();
        cx.new(|cx| {
            let mut subs = Vec::new();
            if let Some(project) = &project {
                subs.push(cx.subscribe_in(
                    project,
                    window,
                    move |host: &mut Host, _, e: &ProjectEvent, window, cx| {
                        if matches!(e, ProjectEvent::Opened) {
                            host.opened(&a2, window, cx);
                        }
                    },
                ));
            }
            subs.push(cx.observe(&workspace, |_, _, cx| cx.notify()));
            Host {
                workspace: workspace.clone(),
                side,
                _subs: subs,
                _cycle: Task::ready(()),
            }
        })
        .into()
    });
}
