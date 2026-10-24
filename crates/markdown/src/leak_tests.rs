//! A rendered document closed gives its parse, its text views and its images back.

use gpui_kit::{
    AnyView, AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, Window, div,
};

use crate::{MarkdownBase, MarkdownView};

struct Host(Option<AnyView>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().children(self.0.clone())
    }
}

const DOC: &str = "---\ntitle: Notes\ntags: a\n---\n# Title\n\n> [!NOTE]\n> An **alert** with [a link](other.md).\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n\n```mermaid\ngraph TD; A-->B\n```\n\n$$\nx^2\n$$\n\n```rust\nfn main() {}\n```\n\n![logo](assets/logo.png)\n";

#[gpui_kit::test]
fn closing_a_document_gives_its_entities_back(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("gitviber-markdown-test-{}", std::process::id()));
    // SAFETY: the tests that read the config dir all point it at a temporary folder.
    unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
    cx.update(gitviber_ui::init);
    let (host, cx) = cx.add_window_view(|_, _| Host(None));
    let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
    for round in 0..4 {
        cx.update(|window, cx| {
            let base = MarkdownBase::file(std::env::temp_dir(), "docs/notes.md");
            let view = cx.new(|cx| {
                if round % 2 == 0 {
                    MarkdownView::new(DOC, base, window, cx)
                } else {
                    MarkdownView::inline(DOC, base, window, cx)
                }
            });
            host.update(cx, |h, cx| {
                h.0 = Some(view.into());
                cx.notify();
            });
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|window, cx| {
            host.update(cx, |h, cx| {
                h.0 = None;
                cx.notify();
            });
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }
    cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
}
