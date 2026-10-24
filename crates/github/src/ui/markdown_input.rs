//! A textarea for PR or issue markdown with GitHub's Write/Preview switch: comments, a review,
//! a line comment, the description of an issue or a pull request being made or edited.

use std::cell::Cell;
use std::rc::Rc;

use gitviber_markdown::{Callouts, MarkdownBase, MarkdownView};
use gitviber_ui::memory::Live;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, canvas, div, prelude::FluentBuilder as _, px, rems,
};

use crate::logic::text::github_markdown;
use crate::ui::common::theme;

pub struct MarkdownInput {
    /// Kept while the preview shows, so the text keeps its undo history and cursor.
    input: Entity<TextareaState>,
    /// https://github.com/owner/name, and the page relative links resolve against: the PR's or
    /// issue's, the repo's for one not made yet.
    repo: String,
    page: String,
    small: bool,
    previewing: bool,
    /// The draft rendered, built on the switch and dropped on Write; None for an empty draft.
    preview: Option<Entity<MarkdownView>>,
    /// The box's height as last drawn: the preview takes it, so nothing around it moves.
    height: Rc<Cell<Pixels>>,
    /// The preview's: keys go on reaching the host (⌘↵ posts, Esc cancels) while it shows.
    focus: FocusHandle,
    _live: Live,
}

impl MarkdownInput {
    /// `build` sets the box up (rows, placeholder, its first text).
    pub fn new(
        repo: String,
        page: String,
        window: &mut Window,
        cx: &mut Context<Self>,
        build: impl FnOnce(TextareaState) -> TextareaState,
    ) -> Self {
        MarkdownInput {
            input: cx.new(|cx| build(TextareaState::new(window, cx))),
            repo,
            page,
            small: false,
            previewing: false,
            preview: None,
            height: Rc::default(),
            focus: cx.focus_handle(),
            _live: Live::new("MarkdownInput"),
        }
    }

    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    /// The box itself, for its events.
    pub fn input(&self) -> &Entity<TextareaState> {
        &self.input
    }

    pub fn value(&self, cx: &App) -> SharedString {
        self.input.read(cx).value()
    }

    /// Other text in the box (emptied after a post, a draft filled in), shown to edit.
    pub fn set_value(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |i, cx| i.set_value(text, window, cx));
        self.preview(false, window, cx);
    }

    /// Back to Write, with the caret in the box.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview(false, window, cx);
        self.input.update(cx, |i, cx| i.focus(window, cx));
    }

    fn preview(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.previewing == on {
            return;
        }
        self.previewing = on;
        if !on {
            self.preview = None;
            // Only from the preview: a draft filled in mustn't take focus from the title.
            if self.focus.is_focused(window) {
                self.input.update(cx, |i, cx| i.focus(window, cx));
            }
            cx.notify();
            return;
        }
        let text = self.value(cx);
        self.preview = (!text.trim().is_empty()).then(|| {
            let source = github_markdown(&text, &self.repo, &self.page, &|_| None);
            let base = MarkdownBase {
                root: None,
                file: None,
                callouts: Callouts::GitHub,
            };
            cx.new(|cx| MarkdownView::inline(&source, base, window, cx))
        });
        self.focus.focus(window, cx);
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn previewing(&self) -> (bool, Option<&Entity<MarkdownView>>) {
        (self.previewing, self.preview.as_ref())
    }
}

impl Render for MarkdownInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let on = self.previewing;
        let tab = |id: &'static str, icon: IconName, label: &'static str, preview: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(Icon::new(icon))
                .label(label)
                .toggled(on == preview)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if preview {
                        this.preview(true, window, cx)
                    } else {
                        this.focus(window, cx)
                    }
                }))
        };
        let tabs = div()
            .flex()
            .gap_0p5()
            .child(tab("md-write", IconName::Pencil, "Write", false))
            .child(tab("md-preview", IconName::Eye, "Preview", true));
        let body = if on {
            let height = self.height.get();
            div()
                .id("md-preview-body")
                .track_focus(&self.focus)
                .when(height > px(0.), |d| d.h(height))
                .min_h(px(48.))
                .overflow_y_scroll()
                .rounded(px(4.))
                .border_1()
                .border_color(t.border_strong)
                .bg(t.background)
                .px(px(12.))
                .py(px(8.))
                .map(|d| match &self.preview {
                    Some(view) => d.child(view.clone()),
                    None => d
                        .text_size(rems(0.75))
                        .text_color(t.subtle)
                        .italic()
                        .child("Nothing to preview"),
                })
                .into_any_element()
        } else {
            let height = self.height.clone();
            let input = Textarea::new(&self.input);
            div()
                .relative()
                .child(if self.small { input.small() } else { input })
                .child(
                    canvas(move |b, _, _| height.set(b.size.height), |_, _, _, _| {})
                        .absolute()
                        .size_full(),
                )
                .into_any_element()
        };
        div().flex().flex_col().gap(px(6.)).child(tabs).child(body)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{TestAppContext, VisualTestContext};

    use super::*;

    struct Host(Entity<MarkdownInput>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(400.)).child(self.0.clone())
        }
    }

    fn init(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("gitviber-github-test-{}", std::process::id()));
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(gitviber_ui::init);
    }

    fn host(cx: &mut TestAppContext) -> (Entity<MarkdownInput>, &mut VisualTestContext) {
        init(cx);
        let (host, cx) = cx.add_window_view(|window, cx| {
            Host(cx.new(|cx| {
                MarkdownInput::new(
                    "https://github.com/o/r".into(),
                    "https://github.com/o/r/issues/1".into(),
                    window,
                    cx,
                    |t| t.rows(4),
                )
            }))
        });
        let input = host.read_with(cx, |h, _| h.0.clone());
        (input, cx)
    }

    #[gpui_kit::test]
    fn preview_renders_the_draft_only_while_it_shows(cx: &mut TestAppContext) {
        let (input, cx) = host(cx);
        cx.update(|window, cx| window.draw(cx).clear(cx));
        // An empty draft has nothing to render.
        cx.update(|window, cx| input.update(cx, |i, cx| i.preview(true, window, cx)));
        assert!(input.read_with(cx, |i, _| i.previewing() == (true, None)));
        cx.update(|window, cx| {
            input.update(cx, |i, cx| {
                i.set_value("Fixes **#2**", window, cx);
                assert_eq!(i.previewing(), (false, None));
                i.preview(true, window, cx);
            })
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        let view = input.read_with(cx, |i, _| i.previewing().1.cloned());
        let view = view.expect("a draft with text is rendered");
        let weak = view.downgrade();
        let query = gitviber_markdown::FindQuery {
            text: "Fixes #2".into(),
            ..Default::default()
        };
        assert_eq!(view.update(cx, |v, cx| v.find(&query, cx)), Ok(1));
        drop(view);
        // Back to Write: the rendered copy goes, the text stays.
        cx.update(|window, cx| input.update(cx, |i, cx| i.focus(window, cx)));
        input.read_with(cx, |i, cx| {
            assert_eq!(i.previewing(), (false, None));
            assert_eq!(i.value(cx), "Fixes **#2**");
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        assert!(weak.upgrade().is_none());
    }

    struct Slot(Option<Entity<MarkdownInput>>);

    impl Render for Slot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(400.)).children(self.0.clone())
        }
    }

    #[gpui_kit::test]
    fn a_box_closed_while_previewing_leaves_nothing_behind(cx: &mut TestAppContext) {
        init(cx);
        let (slot, cx) = cx.add_window_view(|_, _| Slot(None));
        let round = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let input = cx.new(|cx| {
                    MarkdownInput::new(String::new(), String::new(), window, cx, |t| t.rows(3))
                });
                input.update(cx, |i, cx| {
                    i.set_value("- [ ] a **draft**", window, cx);
                    i.preview(true, window, cx);
                });
                slot.update(cx, |s, _| s.0 = Some(input));
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
            slot.update(cx, |s, _| s.0 = None);
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.run_until_parked();
        };
        round(cx);
        let snapshot = cx.update(|_, cx| cx.leak_detector_snapshot());
        for _ in 0..4 {
            round(cx);
        }
        cx.update(|_, cx| cx.assert_no_new_leaks(&snapshot));
    }
}
