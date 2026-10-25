//! Footprint of opening and closing a rendered document again and again, drawn off screen:
//! whatever the markdown path keeps after its view is gone shows up as growth.
//!
//! markdown-cycles [N] — MD=full|prose|code|table|alert picks what the documents hold; run with
//! MallocLargeCache=0 so freed big blocks don't count.

use gitviber_markdown::{MarkdownBase, MarkdownView};
use gitviber_ui::memory;
use gpui_kit::{
    AnyView, AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _,
    Window, div, px, size,
};
use std::time::Duration;

struct Host(Option<AnyView>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(self.0.clone())
    }
}

/// A document with `i` in it, so no two are the same text.
fn doc(i: usize, kind: &str) -> String {
    let prose = format!(
        "# Notes {i}\n\nParagraph {i} with **bold {i}**, `code {i}` and a [link {i}](other{i}.md).\n\n"
    );
    let code = format!(
        "```rust\nfn f{i}(x: u32) -> u32 {{\n    x + {i} // {i}\n}}\n```\n\n```ts\nconst a{i} = {i};\n```\n\n"
    );
    let table = format!("| a{i} | b |\n|---|---|\n| {i} | {} |\n\n", i * 2);
    let alert = format!("> [!NOTE]\n> Alert {i} with *text*.\n\n");
    match kind {
        "prose" => prose.repeat(3),
        "code" => code.repeat(2),
        "table" => table.repeat(3),
        "alert" => alert.repeat(3),
        _ => format!("{prose}{code}{table}{alert}- [x] done {i}\n- [ ] todo\n"),
    }
}

fn footprint() -> String {
    memory::footprint()
        .map(memory::format_bytes)
        .unwrap_or_default()
}

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(100);
    let kind = std::env::var("MD").unwrap_or_default();
    let text_system = gpui_snapshot_platform::current_platform(true).text_system();
    let mut cx = gpui_kit::HeadlessAppContext::with_platform(
        text_system,
        std::sync::Arc::new(gitviber_ui::assets::Assets),
        gpui_snapshot_platform::current_headless_renderer,
    );
    cx.update(gitviber_ui::init);
    let slot: std::rc::Rc<std::cell::RefCell<Option<gpui_kit::Entity<Host>>>> = Default::default();
    let made = slot.clone();
    let window = cx
        .open_window(size(px(1000.), px(800.)), move |window, cx| {
            let host = cx.new(|_| Host(None));
            *made.borrow_mut() = Some(host.clone());
            cx.new(|cx| gpui_kit::base::Root::new(AnyView::from(host), window, cx))
        })
        .expect("a window");
    let tick = Duration::from_millis(16);
    let frame = |cx: &mut gpui_kit::HeadlessAppContext| {
        cx.run_until_parked();
        cx.advance_clock(tick);
        _ = cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx));
        _ = cx.capture_screenshot(window.into());
    };
    let host = slot.borrow_mut().take().expect("the host");
    eprintln!("cycles: before {}", footprint());
    for i in 0..=n {
        let text = doc(i, &kind);
        _ = cx.update_window(window.into(), |_, window, cx| {
            if kind == "none" {
                return;
            }
            let base = MarkdownBase::file(std::env::temp_dir(), "docs/notes.md");
            let view = cx.new(|cx| MarkdownView::new(&text, base, window, cx));
            host.update(cx, |h, cx| {
                h.0 = Some(view.into());
                cx.notify();
            });
        });
        for _ in 0..3 {
            frame(&mut cx);
        }
        cx.update(|cx| {
            host.update(cx, |h, cx| {
                h.0 = None;
                cx.notify();
            })
        });
        frame(&mut cx);
        frame(&mut cx);
        if i % 10 == 0 {
            eprintln!("cycles: {i} {} live={}", footprint(), memory::live_total());
        }
    }
}
