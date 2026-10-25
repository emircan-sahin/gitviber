//! The explorer beside a file tab of a repo, for screenshots.
//!
//! files-demo [REPO] [--file PATH] [--md] [--search TEXT] [--reveal LINE] [--image FILE [--zoom N]]
//!
//! Opens PATH (crates/core/src/patch.rs) once the repo is open; `--md` opens README.md after it,
//! rendered; `--search` shows the Search panel with TEXT searched instead of the explorer;
//! `--image` shows FILE's image preview in place of the tabs, at N% (fitted without `--zoom`).
//! GITVIBER_BACKGROUND=1 opens the window behind the others without taking the keyboard. Memory
//! over open-close cycles: examples/cycles.rs.

use gitviber_files::{ImagePreview, OpenOptions, Reveal, SearchPanel};
use gitviber_ui::workspace::{Project, ProjectEvent, Workspace};
use gitviber_ui::{ActiveAppTheme as _, dev};
use gpui_kit::{
    AnyView, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Window, div, px,
};

#[derive(Clone)]
struct Args {
    file: String,
    md: bool,
    search: Option<String>,
    reveal: Option<u32>,
    image: Option<String>,
    zoom: Option<f32>,
}

fn args() -> Args {
    let mut a = Args {
        file: "crates/core/src/patch.rs".into(),
        md: false,
        search: None,
        reveal: None,
        image: None,
        zoom: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--file" => a.file = it.next().unwrap_or_default(),
            "--md" => a.md = true,
            "--search" => a.search = it.next(),
            "--reveal" => a.reveal = it.next().and_then(|n| n.parse().ok()),
            "--image" => a.image = it.next(),
            "--zoom" => a.zoom = it.next().and_then(|n| n.parse().ok()),
            _ => {}
        }
    }
    a
}

struct Host {
    workspace: Entity<Workspace>,
    side: AnyView,
    image: Option<Entity<ImagePreview>>,
    _subs: Vec<Subscription>,
}

impl Host {
    fn opened(&mut self, a: &Args, window: &mut Window, cx: &mut Context<Self>) {
        let ws = self.workspace.downgrade();
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
        let active = match &self.image {
            Some(image) => Some(image.clone().into()),
            None => ws
                .tabs()
                .active()
                .and_then(|tab| tab.item.handle())
                .map(|h| h.view()),
        };
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
        let (image_path, zoom) = (a.image.clone(), a.zoom);
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
            let image = image_path.map(|path| {
                let image = cx.new(|cx| ImagePreview::new(path.into(), cx));
                let scale = zoom.map(|n| n / 100.);
                image.update(cx, |i, cx| i.zoom_to(scale, cx));
                image
            });
            Host {
                workspace: workspace.clone(),
                side,
                image,
                _subs: subs,
            }
        })
        .into()
    });
}
