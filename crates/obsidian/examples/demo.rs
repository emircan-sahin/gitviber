//! The Obsidian panel beside a vault file's tab, for screenshots and memory checks.
//!
//! obsidian-demo [REPO] [--vault PATH] [--open FILE[#anchor]]... [--code] [--zoom F]
//!               [--cycles N]
//!
//! The vault must be one Obsidian lists (its obsidian.json under $HOME). `--open` opens vault
//! files once the vault list is read, the last one shown; `--code` switches it to its source;
//! `--zoom` zooms a canvas by F. `--cycles N` opens and closes each `--open` file N times and
//! prints the process's memory before and after.

use std::time::Duration;

use gitviber_obsidian::{Mode, VaultTab, Vaults, VaultsEvent};
use gitviber_ui::workspace::Workspace;
use gitviber_ui::{ActiveAppTheme as _, dev, memory};
use gpui_kit::{
    AnyView, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Task, Window, div, px,
};

#[derive(Clone, Default)]
struct Args {
    vault: Option<String>,
    open: Vec<String>,
    code: bool,
    zoom: Option<f64>,
    cycles: usize,
}

fn args() -> Args {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--vault" => a.vault = it.next(),
            "--open" => a.open.extend(it.next()),
            "--code" => a.code = true,
            "--zoom" => a.zoom = it.next().and_then(|z| z.parse().ok()),
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
    started: bool,
    _subs: Vec<Subscription>,
    _cycle: Task<()>,
}

impl Host {
    fn start(&mut self, a: &Args, window: &mut Window, cx: &mut Context<Self>) {
        let Some(vault) = Vaults::global(cx).read(cx).current(cx).map(|v| v.path) else {
            return;
        };
        if self.started {
            return;
        }
        self.started = true;
        let ws = self.workspace.downgrade();
        if a.cycles > 0 {
            let (files, cycles) = (a.open.clone(), a.cycles);
            self._cycle = cx.spawn_in(window, async move |_, cx| {
                eprintln!("demo: before {}", footprint());
                for i in 0..=cycles {
                    for file in &files {
                        let (path, anchor) = file.split_once('#').unwrap_or((file, ""));
                        _ = cx.update(|window, cx| {
                            gitviber_obsidian::open_vault_file(
                                &ws, &vault, path, anchor, false, window, cx,
                            )
                        });
                        cx.background_executor()
                            .timer(Duration::from_millis(1500))
                            .await;
                        if i < cycles {
                            _ = cx.update(|_, cx| ws.update(cx, |ws, cx| ws.close_active_tab(cx)));
                            cx.background_executor()
                                .timer(Duration::from_millis(200))
                                .await;
                        }
                    }
                    eprintln!("demo: cycle {i} {}", footprint());
                }
                eprintln!("demo: after {cycles} open-close cycles {}", footprint());
                eprintln!("demo: live {:?}", memory::live_counts());
            });
            return;
        }
        for file in &a.open {
            let (path, anchor) = file.split_once('#').unwrap_or((file, ""));
            gitviber_obsidian::open_vault_file(&ws, &vault, path, anchor, false, window, cx);
        }
        let (code, zoom, workspace) = (a.code, a.zoom, self.workspace.clone());
        // Once the file is read and drawn.
        self._cycle = cx.spawn_in(window, async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(600))
                .await;
            _ = cx.update(|window, cx| {
                let tab = workspace
                    .read(cx)
                    .tabs()
                    .active()
                    .and_then(|t| t.item.handle())
                    .and_then(|h| h.view().downcast::<VaultTab>().ok());
                if let Some(tab) = tab {
                    if code {
                        tab.update(cx, |t, cx| t.set_mode(Mode::Code, window, cx));
                    }
                    if let Some(z) = zoom {
                        tab.update(cx, |t, cx| t.zoom_canvas(z, window, cx));
                    }
                }
            });
        });
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.app_theme().clone();
        // Built now if it was restored or let go, as the app's center does.
        let active = self
            .workspace
            .update(cx, |ws, cx| ws.active_view(window, cx));
        let ws = self.workspace.read(cx);
        let titles: Vec<_> = ws
            .tabs()
            .tabs()
            .iter()
            .map(|tab| {
                let on = ws.tabs().active_key() == Some(&tab.key);
                let title = tab.item.title(cx);
                let d = div()
                    .px_3()
                    .h_full()
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .border_r_1()
                    .border_color(t.border)
                    .text_color(if on { t.foreground } else { t.muted_foreground });
                if on { d.bg(t.background) } else { d }.child(title)
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
                    .w(px(260.))
                    .flex_none()
                    .h_full()
                    .bg(t.sidebar)
                    .border_l_1()
                    .border_color(t.border)
                    .child(self.side.clone()),
            )
    }
}

fn main() {
    let a = args();
    dev::run(move |window, cx| {
        gitviber_files::init(cx);
        gitviber_obsidian::init(cx);
        let workspace = Workspace::for_window(window, cx).expect("dev::run's workspace");
        let model = Vaults::global(cx);
        if let Some(v) = &a.vault {
            model.update(cx, |m, cx| m.pick(v, cx));
        }
        let side = Workspace::ensure_panel(&workspace, gitviber_obsidian::PANEL_ID, window, cx)
            .expect("the panel")
            .view();
        let a2 = a.clone();
        cx.new(|cx| {
            let subs = vec![
                cx.subscribe_in(
                    &model,
                    window,
                    move |host: &mut Host, _, e: &VaultsEvent, window, cx| {
                        if *e == VaultsEvent::ListChanged {
                            host.start(&a2, window, cx);
                        }
                    },
                ),
                cx.observe(&workspace, |_, _, cx| cx.notify()),
            ];
            Host {
                workspace: workspace.clone(),
                side,
                started: false,
                _subs: subs,
                _cycle: Task::ready(()),
            }
        })
        .into()
    });
}
