//! ⌘F on a PR's or an issue's page: one find box over its title, description and comments,
//! stepping through their matches in the page's order.

use std::ops::Range;

use gitviber_markdown::find::{MAX_MATCHES, find_matches};
use gitviber_markdown::{FindBar, FindBarEvent, FindChanged, FindQuery, MarkdownView};
use gitviber_ui::ActiveAppTheme as _;
use gpui_kit::{
    App, AppContext as _, Context, Div, Entity, FocusHandle, HighlightStyle, ParentElement as _,
    ScrollHandle, SharedString, Styled as _, StyledText, Subscription, Window, div, point, px,
};

/// A page's find, built on its first ⌘F.
#[derive(Default)]
pub struct PageFind(Option<(Entity<Finder>, Subscription)>);

impl PageFind {
    /// ⌘F: the box over the page, focused with its text selected; the last query marked again.
    /// `scroll`: the page's, which a match revealed scrolls; `page`: focused when the box closes.
    /// `title` and `views`: what the page shows, in its order.
    pub fn open<V: 'static>(
        &mut self,
        scroll: &ScrollHandle,
        page: &FocusHandle,
        title: &str,
        views: Vec<Entity<MarkdownView>>,
        window: &mut Window,
        cx: &mut Context<V>,
    ) {
        let (finder, _) = self.0.get_or_insert_with(|| {
            let finder = cx.new(|cx| Finder::new(scroll.clone(), page.clone(), window, cx));
            let redraw = cx.observe(&finder, |_, _, cx| cx.notify());
            (finder, redraw)
        });
        finder.update(cx, |f, cx| f.open(title, views, window, cx));
    }

    /// What the page shows now, read only while the box is open: looked through again where it
    /// changed (a comment came, the title was edited).
    pub fn sync(&self, parts: impl FnOnce() -> (String, Vec<Entity<MarkdownView>>), cx: &mut App) {
        let Some((finder, _)) = &self.0 else { return };
        if !finder.read(cx).open {
            return;
        }
        let (title, views) = parts();
        finder.update(cx, |f, cx| {
            if f.take_parts(&title, views, cx) {
                f.run(false, cx);
            }
        });
    }

    /// The title, its matches washed while the box is open: it's plain text, not a markdown view.
    pub fn title(&self, title: impl Into<SharedString>, cx: &App) -> StyledText {
        let title = title.into();
        let Some(f) = self
            .0
            .as_ref()
            .map(|(f, _)| f.read(cx))
            .filter(|f| f.open && f.title == *title)
        else {
            return StyledText::new(title);
        };
        let t = cx.app_theme();
        let marks = f.title_marks.iter().enumerate().map(|(i, r)| {
            let color = if f.current == Some(i) {
                t.find_current
            } else {
                t.find_match
            };
            let style = HighlightStyle {
                background_color: Some(color),
                ..Default::default()
            };
            (r.clone(), style)
        });
        StyledText::new(title).with_highlights(marks.collect::<Vec<_>>())
    }

    /// The box, over the top right of the page, while it's open.
    pub fn bar(&self, cx: &App) -> Option<Div> {
        let f = self.0.as_ref()?.0.read(cx);
        f.open.then(|| {
            div()
                .absolute()
                .top(px(8.))
                .right(px(16.))
                .child(f.bar.clone())
        })
    }

    #[cfg(test)]
    fn finder(&self) -> Option<Entity<Finder>> {
        self.0.as_ref().map(|(f, _)| f.clone())
    }
}

pub struct Finder {
    bar: Entity<FindBar>,
    open: bool,
    scroll: ScrollHandle,
    page: FocusHandle,
    title: String,
    /// The title's matches (bytes); the views keep their own.
    title_marks: Vec<Range<usize>>,
    views: Vec<Entity<MarkdownView>>,
    /// The current match over the whole page: the title's first, then each view's in turn.
    current: Option<usize>,
    error: Option<String>,
    _bar: Subscription,
    /// Each view's text changing under the find (a comment edited): the count follows.
    _views: Vec<Subscription>,
}

impl Finder {
    fn new(
        scroll: ScrollHandle,
        page: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let bar = cx.new(|cx| FindBar::new(window, cx));
        let sub = cx.subscribe_in(
            &bar,
            window,
            |this, _, e: &FindBarEvent, window, cx| match e {
                FindBarEvent::Changed => this.run(true, cx),
                FindBarEvent::Step(dir) => this.step(*dir, cx),
                FindBarEvent::Close => this.close(window, cx),
            },
        );
        Finder {
            bar,
            open: false,
            scroll,
            page,
            title: String::new(),
            title_marks: vec![],
            views: vec![],
            current: None,
            error: None,
            _bar: sub,
            _views: vec![],
        }
    }

    fn open(
        &mut self,
        title: &str,
        views: Vec<Entity<MarkdownView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.open {
            self.open = true;
            self.take_parts(title, views, cx);
            self.run(true, cx);
        }
        self.bar.update(cx, |b, cx| b.focus(window, cx));
        cx.notify();
    }

    /// Takes what the page shows; false when it's what the find already looked through.
    fn take_parts(
        &mut self,
        title: &str,
        views: Vec<Entity<MarkdownView>>,
        cx: &mut Context<Self>,
    ) -> bool {
        let same = self.views == views;
        if same && self.title == title {
            return false;
        }
        if !same {
            // A view the page no longer shows (the description while it's edited) keeps no marks.
            let none = FindQuery::default();
            for gone in self.views.iter().filter(|v| !views.contains(v)) {
                gone.update(cx, |v, cx| _ = v.find(&none, cx));
            }
            self._views = views
                .iter()
                .map(|v| {
                    v.update(cx, |v, _| v.reveal_in(self.scroll.clone()));
                    cx.subscribe(v, |this, _, _: &FindChanged, cx| this.apply(false, cx))
                })
                .collect();
            self.views = views;
        }
        title.clone_into(&mut self.title);
        true
    }

    /// Looks for the box's query everywhere. `fresh` (typed, opened): from the first match,
    /// revealed; else the current one stays where it can.
    fn run(&mut self, fresh: bool, cx: &mut Context<Self>) {
        let query = self.bar.read(cx).query().clone();
        let mut error = None;
        self.title_marks = find_matches(&self.title, &query, MAX_MATCHES).unwrap_or_else(|e| {
            error = Some(e);
            vec![]
        });
        for v in &self.views {
            if let Err(e) = v.update(cx, |v, cx| v.find(&query, cx)) {
                error = Some(e);
            }
        }
        self.error = error;
        if fresh {
            self.current = Some(0);
        }
        self.apply(fresh, cx);
    }

    /// Marks the current match where it falls, none elsewhere, and reveals it if `reveal`.
    fn apply(&mut self, reveal: bool, cx: &mut Context<Self>) {
        let counts: Vec<usize> = self.views.iter().map(|v| v.read(cx).matches().1).collect();
        let total = self.title_marks.len() + counts.iter().sum::<usize>();
        self.current = self.current.filter(|_| total > 0).map(|c| c.min(total - 1));
        let mut base = self.title_marks.len();
        // The title heads the page.
        if reveal && self.current.is_some_and(|c| c < base) {
            self.scroll.set_offset(point(px(0.), px(0.)));
        }
        for (v, n) in self.views.iter().zip(counts) {
            let local = self
                .current
                .filter(|c| (base..base + n).contains(c))
                .map(|c| c - base);
            v.update(cx, |v, cx| {
                v.select_match(local, reveal && local.is_some(), cx)
            });
            base += n;
        }
        let found = match &self.error {
            Some(e) => Err(e.clone()),
            None => Ok((self.current, total)),
        };
        self.bar.update(cx, |b, cx| b.set_found(found, cx));
        cx.notify();
    }

    /// The next match (1) or the one before (-1), round the page; the first or the last when
    /// none is current.
    fn step(&mut self, dir: isize, cx: &mut Context<Self>) {
        let total = self.title_marks.len()
            + self
                .views
                .iter()
                .map(|v| v.read(cx).matches().1)
                .sum::<usize>();
        if total == 0 {
            return;
        }
        self.current = Some(match self.current {
            Some(c) => (c as isize + dir).rem_euclid(total as isize) as usize,
            None if dir < 0 => total - 1,
            None => 0,
        });
        self.apply(true, cx);
    }

    /// Esc or ×: the marks go, the query stays for the next ⌘F, focus goes back to the page.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.open) {
            return;
        }
        let none = FindQuery::default();
        for v in self.views.drain(..) {
            v.update(cx, |v, cx| _ = v.find(&none, cx));
        }
        self._views.clear();
        self.title.clear();
        self.title_marks.clear();
        self.current = None;
        self.page.focus(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use gitviber_markdown::MarkdownBase;
    use gpui_kit::{
        InteractiveElement as _, IntoElement, Render, StatefulInteractiveElement as _,
        TestAppContext,
    };

    use super::*;

    struct Host {
        find: PageFind,
        title: String,
        views: Vec<Entity<MarkdownView>>,
        scroll: ScrollHandle,
        focus: FocusHandle,
    }

    impl Host {
        fn parts(&self) -> (String, Vec<Entity<MarkdownView>>) {
            (self.title.clone(), self.views.clone())
        }
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.find.sync(|| self.parts(), cx);
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("page")
                        .track_focus(&self.focus)
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .child(self.find.title(self.title.clone(), cx))
                        .children(self.views.clone()),
                )
                .children(self.find.bar(cx))
        }
    }

    #[gpui_kit::test]
    fn one_find_steps_through_title_body_and_comments(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("gitviber-github-test-{}", std::process::id()));
        // SAFETY: the tests that read the config dir all point it at a temporary folder.
        unsafe { std::env::set_var("GITVIBER_CONFIG_DIR", &dir) };
        cx.update(gitviber_ui::init);
        let (host, cx) = cx.add_window_view(|window, cx| {
            let view = |text: &str, window: &mut Window, cx: &mut Context<Host>| {
                cx.new(|cx| MarkdownView::inline(text, MarkdownBase::default(), window, cx))
            };
            Host {
                find: PageFind::default(),
                title: "Fix the foo".into(),
                views: vec![
                    view("The **foo** body", window, cx),
                    view("A foo, and a Foo", window, cx),
                ],
                scroll: ScrollHandle::new(),
                focus: cx.focus_handle(),
            }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            host.update(cx, |h, cx| {
                let (title, views) = h.parts();
                let (scroll, focus) = (h.scroll.clone(), h.focus.clone());
                h.find.open(&scroll, &focus, &title, views, window, cx);
            })
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.simulate_input("foo");
        cx.run_until_parked();
        let finder = host.read_with(cx, |h, _| h.find.finder()).expect("open");
        let views = host.read_with(cx, |h, _| h.views.clone());
        let state = |cx: &mut gpui_kit::VisualTestContext| {
            let current = finder.read_with(cx, |f, _| f.current);
            let marks: Vec<_> = views
                .iter()
                .map(|v| v.read_with(cx, |v, _| v.matches()))
                .collect();
            (current, marks)
        };
        // Typed: the first match, the title's.
        assert_eq!(state(cx), (Some(0), vec![(None, 1), (None, 2)]));
        let step = |dir, cx: &mut gpui_kit::VisualTestContext| {
            finder.update(cx, |f, cx| f.step(dir, cx));
        };
        step(1, cx);
        assert_eq!(state(cx), (Some(1), vec![(Some(0), 1), (None, 2)]));
        step(1, cx);
        step(1, cx);
        assert_eq!(state(cx), (Some(3), vec![(None, 1), (Some(1), 2)]));
        step(1, cx);
        assert_eq!(state(cx), (Some(0), vec![(None, 1), (None, 2)]));
        step(-1, cx);
        assert_eq!(state(cx).0, Some(3));
        // A comment edited under the find: the current one stays where it can.
        views[1].update(cx, |v, cx| v.set_source("Nothing here", cx));
        cx.run_until_parked();
        assert_eq!(state(cx), (Some(1), vec![(Some(0), 1), (None, 0)]));
        // The title changed: looked through again.
        host.update(cx, |h, cx| {
            h.title = "Foo foo".into();
            cx.notify();
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(finder.read_with(cx, |f, _| f.title_marks.len()), 2);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(state(cx), (None, vec![(None, 0), (None, 0)]));
        assert!(host.read_with(cx, |h, cx| h.find.bar(cx).is_none()));
        drop((finder, views));

        // A page closed with its find open gives the box and the marks back.
        let round = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| {
                host.update(cx, |h, cx| {
                    h.views = vec![cx.new(|cx| {
                        MarkdownView::inline("foo", MarkdownBase::default(), window, cx)
                    })];
                    let (title, views) = h.parts();
                    let (scroll, focus) = (h.scroll.clone(), h.focus.clone());
                    h.find.open(&scroll, &focus, &title, views, window, cx);
                })
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            cx.simulate_input("o");
            host.update(cx, |h, _| {
                h.find = PageFind::default();
                h.views.clear();
            });
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
